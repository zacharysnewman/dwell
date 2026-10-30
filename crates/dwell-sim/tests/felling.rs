//! Felling through the server (docs/FELLING.md): cutting a trunk sends the cleared cells, the
//! piece's spawn and its poses; at rest it settles into blocks lying on the ground.

mod common;

use common::*;
use dwell_protocol::*;
use dwell_sim::server::SessionId;
use dwell_world::{Chunk, ChunkCoord, FLAT_TOP_Y, M, chunk_hash, generate_flat_chunk};

/// The flat world's first air row.
const G: i32 = FLAT_TOP_Y;

fn log() -> u16 {
    dwell_blocks::parse_state("dwell:log[axis=y]").expect("log")
}
fn is_log(m: u16) -> bool {
    dwell_blocks::info(m).name.starts_with("dwell:log")
}

fn join_streaming(f: &mut Fixture, id: SessionId) {
    let w = f.join(id, &Client::new(id as u8));
    let mut v = Chunk::default();
    generate_flat_chunk(ChunkCoord::from_array(w.verification_chunk), &mut v);
    f.send(id, Message::WorldgenCheck(chunk_hash(&v)));
}

#[test]
fn a_cut_trunk_falls_tips_and_lies_on_the_ground_as_logs() {
    // face-a-only: the flat test world is face A's; the support rule has no up (tests in
    // dwell-world), and pieces fall by the physics' face gravity, tested there.
    fell_and_settle(false);
}

#[test]
fn a_piece_far_from_every_player_still_lands_on_the_ground() {
    // face-a-only: as above. The terrain collides only round bodies that need it: a trunk falling
    // with nobody near must still meet the ground, not fall through the world.
    fell_and_settle(true);
}

fn fell_and_settle(walk_away: bool) {
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // A trunk 14 rows (7 m) tall at (1, 1), a few leaves round its top; leaning a little
    // (its top a cell over) so it tips one way.
    let (x, z) = (1, 1);
    {
        let w = f.server.world();
        for y in G..G + 14 {
            w.set_voxel(x, y, z, log());
        }
        w.set_voxel(x + 1, G + 13, z, log());
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            w.set_voxel(
                x + dx,
                G + 12,
                z + dz,
                dwell_blocks::require("dwell:leaves"),
            );
        }
    }
    // Player 1 (at (−1, ·, −0.25)) breaks the trunk one row above the ground, on its −x face.
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [x, G + 1, z],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    let out = f.take_all();
    let spawns: Vec<&EntitySpawn> = out[&1]
        .iter()
        .filter_map(|m| match m {
            Message::EntitySpawn(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(spawns.len(), 1, "one piece falls");
    assert_eq!(
        spawns[0].carriers, 13,
        "the trunk over the cut and its branch"
    );
    let collapse = out[&1].iter().any(|m| {
        matches!(m, Message::VoxelModification(v) if v.reason == VoxelModificationReason::Collapse)
    });
    assert!(collapse, "its cells cleared");
    assert_eq!(f.server.world().get_voxel(x, G + 5, z), M.air);
    assert!(
        is_log(f.server.world().get_voxel(x, G, z)),
        "the stump stands"
    );
    if walk_away {
        f.move_to(1, -200.0, f64::from(G) * dwell_world::CELL_H, -200.0);
    }
    // It falls and is seen falling; within 15 s it is at rest and settled, and so is anything
    // that settled on nothing and fell again (a trunk resting on its leaves).
    let mut posed = false;
    let mut despawned = false;
    let mut live: std::collections::HashSet<u32> = [spawns[0].id].into();
    for _ in 0..(60 * 15) {
        f.server.step();
        // Datagrams too (the snapshots): the whole outbox.
        for o in f
            .server
            .take_outbox()
            .into_iter()
            .filter(|o| o.session == 1)
        {
            match Message::decode(&o.bytes) {
                Some(Message::PhysicsSnapshot(s)) if !s.pieces.is_empty() => posed = true,
                Some(Message::EntityDespawn(id)) => {
                    despawned |= id == spawns[0].id;
                    live.remove(&id);
                }
                Some(Message::EntitySpawn(e)) => {
                    live.insert(e.id);
                }
                _ => {}
            }
        }
        if despawned && live.is_empty() {
            break;
        }
    }
    assert!(posed, "snapshots carried its pose");
    assert!(despawned, "it settled");
    assert!(live.is_empty(), "everything that fell settled: {live:?}");
    // It lies on the ground: logs along x or z on the first rows over the floor, none upright
    // over the stump's height, and about as many as fell (½ m rows lying as 1 m cells: half).
    let w = f.server.world();
    let mut lying = 0;
    let mut high = 0;
    for zz in z - 10..=z + 10 {
        for xx in x - 10..=x + 10 {
            for y in G..G + 14 {
                let m = w.get_voxel(xx, y, zz);
                if !is_log(m) || (xx, zz) == (x, z) && y == G {
                    continue;
                }
                if y > G + 3 {
                    high += 1;
                }
                if dwell_blocks::info(m).axis != dwell_blocks::Axis::Y {
                    lying += 1;
                }
            }
        }
    }
    assert_eq!(high, 0, "nothing left standing in the air");
    assert!(lying >= 5, "the trunk lies along x or z: {lying} cells");
}

#[test]
fn a_falling_trunk_hurts_a_player_it_lands_on_and_not_the_one_who_cut_it() {
    // face-a-only: as above (the hit test has no up: dwell-sim felling.rs's unit tests).
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    join_streaming(&mut f, 2);
    f.server.step();
    f.take_all();
    // A trunk 16 rows (8 m) tall at (1, 1), cut on its −x face by player 1, standing at −x: it
    // tips toward +x, onto player 2 standing 4 m out under it.
    let (x, z) = (1, 1);
    for y in G..G + 16 {
        f.server.world().set_voxel(x, y, z, log());
    }
    let feet = f64::from(G) * dwell_world::CELL_H;
    f.move_to(1, -1.0, feet, 1.5);
    f.move_to(2, 5.5, feet, 1.5);
    f.server.step();
    let full = f.server.health_of(1);
    assert_eq!(f.server.health_of(2), full);
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [x, G + 1, z],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let mut hurt = false;
    for _ in 0..(60 * 6) {
        f.server.step();
        for o in f.server.take_outbox() {
            if let Some(Message::PlayerEvent(e)) = Message::decode(&o.bytes)
                && e.kind == PlayerEventKind::Damage
                && e.cause == DamageCause::Crush
            {
                assert_eq!(e.player_id, 2, "only the player it lands on");
                hurt = true;
            }
        }
    }
    assert!(hurt, "player 2 was hit: health {}", f.server.health_of(2));
    assert!(f.server.health_of(2) < full);
    assert_eq!(f.server.health_of(1), full);
    // Hit, not crushed through the floor: a heavy piece is held from players' pushes only while
    // it is slow (a falling log held fast pressed player 2 through the ground to a deadly fall).
    assert!(f.server.health_of(2) > 0, "alive");
    let y = f.server.feet_of(2).expect("player 2")[1];
    assert!(y > feet - 0.1, "on the floor: feet at {y}, floor {feet}");
}

/// Breaks `cell` as player 1 (on its −x face, as `fell_and_settle` does), then steps until `done`
/// holds (at most 15 s), true if it did.
fn break_and_wait(
    f: &mut Fixture,
    cell: [i32; 3],
    done: impl Fn(&mut dwell_world::VoxelWorld) -> bool,
) -> bool {
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell,
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    for _ in 0..(60 * 15) {
        f.server.step();
        f.take_all();
        if done(f.server.world()) {
            return true;
        }
    }
    false
}

#[test]
fn a_leaf_no_tree_holds_falls_and_lands_as_a_leaf() {
    // face-a-only: the flat test world is face A's (the rule's both faces: dwell-world tests).
    // Owner, 2026-10-28: leaves no log holds fall to the ground and stay there, solid to the
    // world (never to players). Red before: it fell through the ground and was gone.
    let leaves = dwell_blocks::require("dwell:leaves");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    {
        let w = f.server.world();
        for y in G..G + 3 {
            w.set_voxel(1, y, 1, M.stone);
        }
        // A leaf beside the stone, a row over the ground.
        w.set_voxel(2, G + 1, 1, leaves);
    }
    let landed = break_and_wait(&mut f, [1, G + 1, 1], |w| {
        w.get_voxel(2, G, 1) == leaves && w.get_voxel(2, G + 1, 1) == M.air
    });
    assert!(landed, "the leaf lies on the ground");
    // Far from every player too: the terrain collides round a leaves-only piece (red before: it
    // was left out, so the leaf fell through the world).
    {
        let w = f.server.world();
        for y in G..G + 6 {
            w.set_voxel(1, y, 3, M.stone);
        }
        w.set_voxel(2, G + 5, 3, leaves);
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [1, G + 5, 3],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    f.server.step();
    f.move_to(1, -200.0, f64::from(G) * dwell_world::CELL_H, -200.0);
    let mut far = false;
    for _ in 0..(60 * 15) {
        f.server.step();
        f.take_all();
        let w = f.server.world();
        // Within a cell of where it fell (it may bounce over one).
        let near = (1..=3).any(|x| (2..=4).any(|z| w.get_voxel(x, G, z) == leaves));
        if near {
            far = true;
            break;
        }
    }
    assert!(
        far,
        "the far leaf lies on the ground, within a cell of where it fell"
    );
}

#[test]
fn sand_with_nothing_under_it_falls_and_lands_as_sand() {
    // face-a-only: as above. Owner, 2026-10-28: sand falls, with the sand on it. Red before:
    // sand stood in the air.
    let sand = dwell_blocks::require("dwell:sand");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    {
        let w = f.server.world();
        w.set_voxel(1, G, 1, M.stone);
        for y in G + 1..G + 4 {
            w.set_voxel(1, y, 1, sand);
        }
    }
    let landed = break_and_wait(&mut f, [1, G, 1], |w| {
        (G..G + 3).all(|y| w.get_voxel(1, y, 1) == sand) && w.get_voxel(1, G + 3, 1) == M.air
    });
    assert!(landed, "the column lies on the ground, a row lower");
}

#[test]
fn a_leaf_placed_against_a_side_in_the_air_falls_and_lands() {
    // face-a-only: as above. Owner, 2026-10-28: "placing leaves by themselves fall through the
    // world". Placed against a pillar's side two metres up, no tree near: it lands at its foot.
    let leaves = dwell_blocks::require("dwell:leaves");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    {
        let w = f.server.world();
        for y in G..G + 5 {
            w.set_voxel(1, y, 1, M.stone);
        }
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [1, G + 3, 1],
            face: 1,
            material: leaves,
            rows: 1,
            down: false,
        }),
    );
    let mut landed = false;
    let mut placed = false;
    for _ in 0..(60 * 15) {
        f.server.step();
        f.take_all();
        let w = f.server.world();
        placed |= w.get_voxel(0, G + 3, 1) == leaves;
        if (-1..=1).any(|x| (0..=2).any(|z| w.get_voxel(x, G, z) == leaves)) {
            landed = true;
            break;
        }
    }
    assert!(
        landed,
        "the leaf lies at the pillar's foot (placed at all: {placed})"
    );
}

#[test]
fn sand_falling_on_a_player_passes_them_and_lands_beside_them() {
    // face-a-only: as above. Owner, 2026-10-28 ("slide off for now"): falling sand may hurt a
    // player but never settles on or in one. Red before: it came to rest on the head (then hung
    // in the air), or in the player's own cells it was destroyed.
    let sand = dwell_blocks::require("dwell:sand");
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    // Player 1 stands at (−1, ·, −0.25): a stone over their head, two sand on it.
    {
        let w = f.server.world();
        w.set_voxel(-1, G + 5, -1, M.stone);
        w.set_voxel(-1, G + 6, -1, sand);
        w.set_voxel(-1, G + 7, -1, sand);
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [-1, G + 5, -1],
            face: 3,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let in_body = |x: i32, z: i32| (x == -1 || x == -2) && (z == -1 || z == 0);
    let mut done = false;
    for _ in 0..(60 * 15) {
        f.server.step();
        f.take_all();
        let w = f.server.world();
        let mut cells = Vec::new();
        for x in -4..=2 {
            for z in -4..=2 {
                for y in G..G + 9 {
                    if w.get_voxel(x, y, z) == sand {
                        cells.push([x, y, z]);
                    }
                }
            }
        }
        if cells.len() == 2 && cells.iter().all(|c| c[1] < G + 2) {
            assert!(
                cells.iter().all(|c| !in_body(c[0], c[2])),
                "landed in the player's cells: {cells:?}"
            );
            done = true;
            break;
        }
    }
    assert!(done, "both sand blocks lie on the ground beside the player");
}

#[test]
fn cutting_a_tall_trunk_loads_terrain_collision_round_it_not_a_cube_of_its_length() {
    // face-a-only: as above (the flat test world is face A's).
    // Owner, 2026-10-30: a column of 40 logs (20 m) cut at its foot stalled the game. The server
    // loaded terrain collision in a cube round the piece's foot, half as wide as its length plus
    // 2 m (44 m across): ~36 chunks in the cut's tick. Now the piece's own bounds (a 1 × 20 × 1 m
    // stick, plus a tick's travel), at most `terrainChunksPerTick` new chunks a tick, nearest
    // first. Pinned by the chunk count (deterministic); the time is printed to compare.
    let mut f = Fixture::new(flat());
    join_streaming(&mut f, 1);
    f.server.step();
    f.take_all();
    let (x, z) = (1, 1);
    {
        let w = f.server.world();
        for y in G..G + 40 {
            w.set_voxel(x, y, z, log());
        }
        w.set_voxel(x + 1, G + 39, z, log()); // leaning: it tips
    }
    for _ in 0..30 {
        f.server.step();
    }
    f.take_all();
    let before = f
        .server
        .players()
        .terrain()
        .map_or(0, |t| t.loaded_chunks());
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Break,
            cell: [x, G, z],
            face: 1,
            material: 0,
            rows: 1,
            down: false,
        }),
    );
    let t = std::time::Instant::now();
    f.server.step();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let after = f
        .server
        .players()
        .terrain()
        .map_or(0, |t| t.loaded_chunks());
    let budget = dwell_sim::felling::FELLING.terrain_chunks_per_tick as usize;
    eprintln!(
        "cut tick: {ms:.1} ms, {} terrain chunks loaded",
        after - before
    );
    assert!(
        after - before <= budget,
        "the cut's tick loaded {} terrain chunks (at most {budget})",
        after - before
    );
    // It still falls, tips and settles on the ground: nothing lost through unloaded terrain.
    let mut settled = false;
    for _ in 0..(60 * 20) {
        f.server.step();
        for o in f.server.take_outbox() {
            if let Some(Message::EntityDespawn(_)) = Message::decode(&o.bytes) {
                settled = true;
            }
        }
    }
    assert!(settled, "it settled");
    let w = f.server.world();
    let logs = (x - 30..=x + 30)
        .flat_map(|xx| (z - 30..=z + 30).map(move |zz| (xx, zz)))
        .flat_map(|(xx, zz)| (G - 2..G + 50).map(move |y| (xx, y, zz)))
        .filter(|&(xx, y, zz)| is_log(w.get_voxel(xx, y, zz)))
        .count();
    assert!(logs >= 40, "the trunk lies on the ground: {logs} logs");
}
