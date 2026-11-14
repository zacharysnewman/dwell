//! Dynamite through the server (docs/EXPLOSIVES.md): a torch held against it lights it, its fuse
//! runs, and the blast destroys the breakable blocks within its destroy radius and flings the rest
//! in its radius as pieces, knocks back players by their distance and hurts those within the
//! destroy radius, lights other dynamite, blows out water; on both faces.

mod common;

use common::*;
use dwell_blocks::{StateId, info, require};
use dwell_physics::{DVec3, Vec3};
use dwell_protocol::*;
use dwell_sim::server::{ServerConfig, SessionId};
use dwell_world::explosives::{EXPLOSIVES, ExplosiveData, cells_within};

/// A cell offset's distance (m) from the blast's cell (rows are `CELL_H` tall).
fn dist(o: [i32; 3]) -> f64 {
    (f64::from(o[0]).powi(2) + (f64::from(o[1]) * CELL_H).powi(2) + f64::from(o[2]).powi(2)).sqrt()
}
use dwell_world::{CELL_H, FLAT_TOP_Y, FaceFrame, M, empty_generator, m_of_row};

/// The scene's ground level (face-local row): stone below it.
const BASE: i32 = FLAT_TOP_Y + 40;

fn dynamite() -> ExplosiveData {
    *EXPLOSIVES
        .iter()
        .find(|e| e.name == "dwell:dynamite")
        .expect("dynamite")
}

/// A server on an empty world with a stone ground in face `f`'s frame (x, z −14..=14, six rows
/// deep under `BASE`), the players joined on it.
struct Scene {
    f: FaceFrame,
    fx: Fixture,
    base: i32,
}

impl Scene {
    /// In adventure, where blasts hurt (ADR 0065).
    fn new(f: FaceFrame, players: usize) -> Self {
        Self::in_mode(f, players, "adventure")
    }
    fn in_mode(f: FaceFrame, players: usize, mode: &str) -> Self {
        Self::in_config(f, players, mode, |_| {})
    }
    fn in_config(
        f: FaceFrame,
        players: usize,
        mode: &str,
        tweak: impl FnOnce(&mut ServerConfig),
    ) -> Self {
        let mut config = ServerConfig {
            generator_override: Some(empty_generator()),
            spawn: Some(f.point([0.5, m_of_row(BASE), -10.5])),
            mode: mode.into(),
            ..flat()
        };
        tweak(&mut config);
        let fx = Fixture::new(config);
        let mut s = Self { f, fx, base: BASE };
        // A blast's pieces all spawn on its tick here: these tests read that tick; the spreading
        // over ticks (`limits.piecesPerTick`) is pinned by its own test, which sets it back.
        s.fx.server.set_pieces_per_tick(usize::MAX);
        let stone = require("dwell:stone");
        for x in -14..=14 {
            for z in -14..=14 {
                for r in -6..0 {
                    s.set([x, r, z], stone);
                }
            }
        }
        for id in 1..=players as SessionId {
            s.fx.join(id, &Client::new(id as u8));
            // Adventure uses items up (ADR 0066): a torch to light with, dynamite to place.
            s.fx.server.give(id as u16, "dwell:torch", 64);
            s.fx.server.give(id as u16, "dwell:dynamite", 64);
        }
        for _ in 0..5 {
            s.fx.server.step();
        }
        s.fx.take_all();
        s
    }
    /// On the flat world (face A's, endless ground: what flies far lands), its ground the scene's.
    fn flat(players: usize) -> Self {
        let mut s = Self {
            f: FaceFrame::A,
            fx: Fixture::new(flat()),
            base: FLAT_TOP_Y,
        };
        s.fx.server.set_pieces_per_tick(usize::MAX);
        for id in 1..=players as SessionId {
            s.fx.join(id, &Client::new(id as u8));
        }
        for _ in 0..5 {
            s.fx.server.step();
        }
        s.fx.take_all();
        s
    }
    /// World cell of face-local (x, row over `BASE`, z).
    fn cell(&self, [x, r, z]: [i32; 3]) -> [i32; 3] {
        self.f.cell([x, self.base + r, z])
    }
    fn set(&mut self, at: [i32; 3], m: StateId) {
        let [x, y, z] = self.cell(at);
        let m = self.f.state(m);
        self.fx.server.world().set_voxel(x, y, z, m);
    }
    fn get(&mut self, at: [i32; 3]) -> StateId {
        let c = self.cell(at);
        self.fx.server.world().get_voxel_if_loaded(c)
    }
    /// Puts player `id`'s feet at face-local (x, `BASE`'s floor, z), at rest.
    fn put(&mut self, id: u16, x: f64, z: f64) {
        let h = self.fx.server.player_handle_of(id).expect("a live player");
        let f = self.f;
        let (_, physics, players) = self.fx.server.sim();
        let half = f64::from(players.half_height(h));
        let centre = f.point([x, m_of_row(self.base) + half, z]);
        players.teleport(physics, h, DVec3::from(centre), Vec3::ZERO);
    }
    /// Player 1, standing at face-local (0.5, 3.5 m back along −z), lights the dynamite at `at`
    /// with a torch held against its −z face.
    fn light(&mut self, at: [i32; 3]) {
        let face = self.f.cell_face(5) as u8;
        let cell = self.cell(at);
        self.fx.send(
            1,
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Place,
                cell,
                face,
                material: require("dwell:torch"),
                rows: 1,
                down: false,
                point: None,
            }),
        );
    }
    /// Steps `n` ticks, every message kept.
    fn run(&mut self, n: usize) -> Vec<Message> {
        let mut out = Vec::new();
        for _ in 0..n {
            self.fx.server.step();
            for o in self.fx.server.take_outbox() {
                if let Some(m) = Message::decode(&o.bytes) {
                    out.push(m);
                }
            }
        }
        out
    }
}

fn spawns(out: &[Message]) -> Vec<&EntitySpawn> {
    let mut seen = std::collections::HashSet::new();
    out.iter()
        .filter_map(|m| match m {
            Message::EntitySpawn(e) if seen.insert(e.id) => Some(e),
            _ => None,
        })
        .collect()
}

dwell_world::both_faces!(
    fn lit_its_blast_destroys_its_core_and_flings_the_rest(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 1);
        s.put(1, 0.5, -3.5);
        s.run(2);
        let at = [0, 0, 0];
        s.set(at, require("dwell:dynamite"));
        // A log and leaves over it, a bedrock block (indestructible) beside it in the ground.
        for r in 1..=3 {
            s.set([2, r, 0], require("dwell:log[axis=y]"));
        }
        s.set([2, 4, 0], require("dwell:leaves"));
        s.set([1, -1, 1], require("dwell:bedrock"));
        s.light(at);
        s.run(1);
        assert_eq!(
            s.get(at),
            f.state(require("dwell:lit_dynamite")),
            "lit, in its own cell"
        );
        let fuse = s.fx.server.fuses();
        assert_eq!(fuse.len(), 1);
        let ticks = fuse[0].1;
        assert!((179..=180).contains(&ticks), "a 3 s fuse: {ticks}");
        // Out of the blast's reach while it burns.
        s.put(1, 0.5, -12.5);
        let what: Vec<([i32; 3], StateId)> = cells_within(e.radius_m)
            .into_iter()
            .skip(1)
            .map(|o| (o, s.get(o)))
            .filter(|&(_, m)| m != M.air && !info(m).indestructible)
            .collect();
        let before = s.run(ticks as usize - 1);
        assert!(
            spawns(&before).is_empty(),
            "nothing blows before the fuse is out"
        );
        assert_eq!(s.get(at), f.state(require("dwell:lit_dynamite")));
        let out = s.run(1);
        assert_eq!(s.get(at), M.air, "it blew");
        let blast = out.iter().any(|m| {
            matches!(m, Message::VoxelModification(v) if v.reason == VoxelModificationReason::Explosion)
        });
        assert!(
            blast
                || out
                    .iter()
                    .all(|m| !matches!(m, Message::VoxelModification(_)))
        );
        let pieces = spawns(&out);
        assert_eq!(
            e.loosen_radius_m, e.radius_m,
            "the shipped dynamite loosens nothing past its force"
        );
        let (core, shell): (Vec<&([i32; 3], StateId)>, Vec<_>) = what
            .iter()
            .partition(|&&(o, _)| dist(o) <= e.destroy_radius_m);
        assert!(core.len() > 20, "a core of ground: {}", core.len());
        // Only the shell cells with a way out fly (docs/EXPLOSIVES.md §3): those pushed into the
        // ground below stay as terrain.
        assert!(
            pieces.len() < shell.len() && pieces.len() > 80,
            "{} pieces of a {}-cell shell",
            pieces.len(),
            shell.len()
        );
        assert!(shell.len() > 150, "a crater's worth: {}", shell.len());
        for &(o, _) in &core {
            assert_eq!(s.get(*o), M.air, "{o:?} cleared");
        }
        let left = shell.iter().filter(|&&&(o, m)| s.get(o) == m).count();
        assert_eq!(
            left + pieces.len(),
            shell.len(),
            "the rest stay as they were"
        );
        // No piece came from the core: each starts at its cell's centre, past the destroy radius.
        let centre = DVec3::from(dwell_sim::felling::cell_centre(s.cell(at)));
        for p in &pieces {
            let d = (DVec3::from(p.origin) - centre).length();
            assert!(
                d > e.destroy_radius_m - 1e-6 && d <= e.radius_m + 1e-6,
                "a piece {d:.2} m out"
            );
        }
        assert_eq!(
            s.get([1, -1, 1]),
            require("dwell:bedrock"),
            "the bedrock stays"
        );
        // Flung out of the crater (face-locally up), near and far alike (`flingFalloff` 0). A few
        // are knocked down by their packed neighbours in the step they spawned in.
        let up = DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let mut by_d: Vec<(f64, f64)> = Vec::new();
        let mut upward = 0;
        for p in &pieces {
            let h = s.fx.server.body_handle(p.id).expect("a body");
            let v = s.fx.server.physics().velocity(h);
            let pos = DVec3::from(p.origin);
            let d = (pos - centre).length();
            if v.dot(up) > 0.0 {
                upward += 1;
            }
            if dwell_blocks::info(p.cells[0].state)
                .name
                .starts_with("dwell:stone")
            {
                by_d.push((d, v.length()));
            }
        }
        by_d.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n = by_d.len();
        let near: f64 = by_d[..n / 4].iter().map(|x| x.1).sum::<f64>() / (n / 4) as f64;
        let far: f64 = by_d[3 * n / 4..].iter().map(|x| x.1).sum::<f64>() / (n - 3 * n / 4) as f64;
        assert!(
            upward * 10 >= pieces.len() * 9,
            "{upward} of {} flung up and out",
            pieces.len()
        );
        assert!(
            far > 0.4 * near && far > 5.0,
            "the far ones fly too: {near:.2} vs {far:.2} m/s"
        );
        // The log over the radius fell (the blast cut it) or flew.
        let log = f.state(require("dwell:log[axis=y]"));
        assert!((1..=3).all(|r| s.get([2, r, 0]) != log));
    }
);

dwell_world::both_faces!(
    fn players_are_hurt_only_within_the_destroy_radius_and_pushed_past_it(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 3);
        for id in 1..=3 {
            s.put(id, 0.5 + f64::from(id) - 1.0, -3.5);
        }
        s.run(2);
        let at = [0, 0, 0];
        s.set(at, require("dwell:dynamite"));
        s.light(at);
        s.run(1);
        // From the blast's centre to the nearest of each body (radius 0.3 m): player 1 0.5 m,
        // player 2 1.2 m (both within the 2 m destroy radius), player 3 4 m (past it, within the
        // 7 m reach).
        s.put(1, 1.3, 0.5);
        s.put(2, 2.0, 0.5);
        s.put(3, 4.8, 0.5);
        assert!(4.0 > e.destroy_radius_m && 4.0 < e.reach_m);
        let out = s.run(181);
        let mut damage = [0i32; 4];
        let mut push = [0f32; 4];
        let mut dead = [false; 4];
        for m in &out {
            if let Message::PlayerEvent(p) = m {
                let k = usize::from(p.player_id);
                match p.kind {
                    PlayerEventKind::Damage if p.cause == DamageCause::Explosion => {
                        damage[k] += i32::from(p.amount)
                    }
                    PlayerEventKind::Knockback => {
                        let v = Vec3::from(p.vector);
                        push[k] = push[k].max(v.length());
                        let local = f.vector_f32(p.vector);
                        assert!(local[0] > 0.0 && local[1] > 0.0, "out and up: {local:?}");
                    }
                    PlayerEventKind::Death => dead[k] = true,
                    _ => {}
                }
            }
        }
        assert!(dead[1], "beside it kills: {damage:?}");
        assert!(!dead[2] && damage[2] > 0, "1.2 m off is hurt: {damage:?}");
        assert!(damage[2] < damage[1], "{damage:?}");
        assert_eq!(damage[3], 0, "4 m off, past the destroy radius, is unhurt");
        assert!(!dead[3]);
        // Killed by it, player 1 is gone before its push would land; the others are pushed.
        assert!(push[2] > push[3] && push[3] > 0.5, "{push:?}");
    }
);

dwell_world::both_faces!(
    fn in_creative_a_blast_hurts_no_one_but_still_pushes(f: FaceFrame) {
        // ADR 0065: creative has no `damage` ability; the knockback is not damage.
        let mut s = Scene::in_mode(f, 2, "creative");
        for id in 1..=2 {
            s.put(id, 0.5 + f64::from(id) - 1.0, -3.5);
        }
        s.run(2);
        let at = [0, 0, 0];
        s.set(at, require("dwell:dynamite"));
        s.light(at);
        s.run(1);
        s.put(1, 1.3, 0.5);
        s.put(2, 2.0, 0.5);
        let out = s.run(181);
        let mut pushed = 0;
        for m in &out {
            if let Message::PlayerEvent(p) = m {
                assert!(
                    !matches!(p.kind, PlayerEventKind::Damage | PlayerEventKind::Death),
                    "{p:?}"
                );
                if p.kind == PlayerEventKind::Knockback {
                    pushed += 1;
                }
            }
        }
        assert_eq!(s.get(at), M.air, "it blew");
        assert!(pushed > 0, "pushed, not hurt");
        assert_eq!(s.fx.server.health_of(1), players::MAX_HEALTH as i32);
    }
);

dwell_world::both_faces!(
    fn dynamite_caught_in_a_blast_goes_off_after_a_short_random_delay(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 1);
        s.put(1, 0.5, -3.5);
        s.run(2);
        s.set([0, 0, 0], require("dwell:dynamite"));
        s.set([3, 0, 0], require("dwell:dynamite"));
        s.light([0, 0, 0]);
        s.run(1);
        s.put(1, 0.5, -13.0);
        s.run(180);
        assert_eq!(s.get([0, 0, 0]), M.air, "the first blew");
        assert_eq!(
            s.get([3, 0, 0]),
            f.state(require("dwell:lit_dynamite")),
            "the second is lit"
        );
        let fuse = s.fx.server.fuses();
        assert_eq!(fuse.len(), 1);
        let (lo, hi) = (
            (e.chain_delay_seconds[0] * 60.0).floor() as u32,
            (e.chain_delay_seconds[1] * 60.0).ceil() as u32,
        );
        assert!(
            (lo..=hi).contains(&(fuse[0].1 + 1)),
            "a chain delay: {} ticks",
            fuse[0].1
        );
        s.run(fuse[0].1 as usize);
        assert_eq!(s.get([3, 0, 0]), M.air, "the second blew after it");
        assert!(s.fx.server.fuses().is_empty());
    }
);

dwell_world::both_faces!(
    fn water_near_the_blast_is_blown_out_and_farther_water_stays(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 1);
        s.put(1, 0.5, -3.5);
        s.run(2);
        // A pool round it: x 1..=6, z −2..=2, two rows deep in the ground.
        for x in 1..=6 {
            for z in -2..=2 {
                for r in -2..0 {
                    s.set([x, r, z], M.water);
                }
            }
        }
        s.set([0, 0, 0], require("dwell:dynamite"));
        s.light([0, 0, 0]);
        s.run(1);
        s.put(1, 0.5, -13.0);
        s.run(180);
        let near = [1, -1, 0];
        let far = [6, -1, 2];
        assert!(dist(near) < e.water_radius_m && dist(far) > e.water_radius_m);
        assert_eq!(s.get(near), M.air, "blown out");
        assert!(info(s.get(far)).liquid, "beyond the water radius it stays");
    }
);

#[test]
fn a_fixture_explosive_by_data_alone_blasts_by_its_own_numbers() {
    // face-a-only: the data's numbers are face-free; the blast's frames are tested on both above.
    let mut s = Scene::new(FaceFrame::A, 1);
    s.fx.server.set_explosives(vec![ExplosiveData {
        name: "test:charge",
        fuse_seconds: 0.5,
        radius_m: 2.0,
        destroy_radius_m: 1.0,
        loosen_radius_m: 2.0,
        // Every shell cell flies (no path test): the count below is the shell's.
        open_path_m: 0.0,
        ..dynamite()
    }]);
    s.put(1, 0.5, -3.5);
    s.run(2);
    s.set([0, 0, 0], require("dwell:dynamite"));
    s.light([0, 0, 0]);
    s.run(1);
    s.put(1, 0.5, -13.0);
    let out = s.run(30);
    let n = cells_within(2.0)
        .into_iter()
        .skip(1)
        .filter(|&o| o[1] < 0 && dist(o) > 1.0)
        .count();
    assert_eq!(s.get([0, 0, 0]), M.air, "a 0.5 s fuse");
    assert_eq!(
        spawns(&out).len(),
        n,
        "its 2 m ball of the ground, but its 1 m core, destroyed"
    );
    assert_eq!(s.get([0, -2, 0]), M.air, "1 m down: destroyed");
    assert_eq!(
        s.get([0, -1, 3]),
        require("dwell:stone"),
        "past 2 m it stays"
    );
}

/// A fixture blast on the flat world, buried in stone: lights it, runs to the blast, returns the
/// blast tick's spawns (the scene is left just after it).
fn buried_blast(s: &mut Scene, e: ExplosiveData) -> Vec<EntitySpawn> {
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -3.5);
    s.run(2);
    s.set([0, 0, 0], require("dwell:dynamite"));
    s.light([0, 0, 0]);
    s.run(1);
    s.put(1, 0.5, -13.0);
    for o in cells_within(e.loosen_radius_m)
        .into_iter()
        .skip(1)
        .filter(|o| o[1] >= 0)
    {
        s.set(o, require("dwell:stone"));
    }
    let fuse = s.fx.server.fuses()[0].1 as usize;
    s.run(fuse - 1);
    spawns(&s.run(1)).into_iter().cloned().collect()
}

#[test]
fn the_loosen_shell_past_the_radius_becomes_pieces_given_no_impulse() {
    // face-a-only: the data's radii are face-free; the push's frame is tested on both faces above.
    // Owner (2026-11-06): "dislodge (make physical) blocks in a 5m radius, but only exert force
    // on 4m". A fixture: force to 2 m, its 1 m core destroyed, loosened to 3 m.
    let e = ExplosiveData {
        radius_m: 2.0,
        destroy_radius_m: 1.0,
        loosen_radius_m: 3.0,
        // Buried, nothing would fly: every cell does (no path test).
        open_path_m: 0.0,
        ..dynamite()
    };
    let mut s = Scene::flat(1);
    let pieces = buried_blast(&mut s, e);
    let shell = cells_within(3.0)
        .into_iter()
        .filter(|&o| dist(o) > 1.0)
        .count();
    assert_eq!(pieces.len(), shell, "every cell from 1 to 3 m is a piece");
    let centre = DVec3::from(dwell_sim::felling::cell_centre(s.cell([0, 0, 0])));
    let (mut loose, mut flung) = (0, 0);
    for p in &pieces {
        let d = (DVec3::from(p.origin) - centre).length();
        let h = s.fx.server.body_handle(p.id).expect("a body");
        let v = s.fx.server.physics().velocity(h).length();
        if d > e.radius_m + 1e-6 {
            // One step of gravity at most: no impulse.
            assert!(v < 0.5, "a loosened piece {d:.2} m out moves at {v:.2} m/s");
            loose += 1;
        } else if d < 1.6 {
            assert!(v > 1.0, "a piece {d:.2} m out is flung: {v:.2} m/s");
            flung += 1;
        }
    }
    assert!(loose > 100 && flung > 10, "{loose} loosened, {flung} flung");
}

#[test]
fn a_blasts_pieces_touch_no_other_piece_in_its_window_and_do_after_it() {
    // face-a-only: contacts between pieces are face-free; the blast's frames are tested on both
    // faces above. Owner (2026-11-06): "what if all the physics blocks couldn't collide with each
    // other for the first 1-2s? ... keep it down to 1s".
    // Not the shipped default (0: measured in Chrome, the window cost more than it saved,
    // docs/EXPLOSIVES.md §3), but available by data.
    let e = ExplosiveData {
        no_piece_contact_s: 1.0,
        // Buried, nothing would fly: every cell does (no path test).
        open_path_m: 0.0,
        ..dynamite()
    };
    // Without the window the packed pieces touch at once (the check below would see it).
    let mut s = Scene::flat(1);
    buried_blast(
        &mut s,
        ExplosiveData {
            no_piece_contact_s: 0.0,
            ..e
        },
    );
    s.run(1);
    let touching = s.fx.server.physics().piece_contact_pairs();
    assert!(touching > 50, "no window: {touching} piece pairs touch");
    assert_eq!(s.fx.server.apart_pieces(), 0);

    let mut s = Scene::flat(1);
    let pieces = buried_blast(&mut s, e);
    assert_eq!(s.fx.server.apart_pieces(), pieces.len());
    for t in 0..58 {
        s.run(1);
        let touching = s.fx.server.physics().piece_contact_pairs();
        assert_eq!(
            touching, 0,
            "tick {t} of the window: {touching} piece pairs touch"
        );
    }
    // After it, the pieces rejoin as they come apart, and touch again: the piles they land in.
    let mut most = 0;
    for _ in 0..6 * 60 {
        s.run(1);
        most = most.max(s.fx.server.physics().piece_contact_pairs());
    }
    assert!(
        most > 10,
        "after the window pieces touch again: {most} pairs at most"
    );
    assert!(
        s.fx.server.apart_pieces() < pieces.len() / 4,
        "most rejoined: {} still apart",
        s.fx.server.apart_pieces()
    );
}

/// A charge on the flat world's ground (face A's, endless: what flies far lands), blown by `e`:
/// its pieces, the body-ticks and ticks until every piece is a block again (at most 20 s), the
/// worst tick (ms).
fn surface_settle(e: ExplosiveData) -> (usize, usize, u32, f64) {
    let mut s = Scene::flat(1);
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -3.5);
    s.run(2);
    s.set([0, 0, 0], require(e.block));
    s.light([0, 0, 0]);
    s.run(1);
    s.put(1, 0.5, -13.0);
    let fuse = s.fx.server.fuses()[0].1 as usize;
    s.run(fuse - 1);
    let ids: Vec<u32> = spawns(&s.run(1)).iter().map(|p| p.id).collect();
    let n = ids.len();
    let (mut body_ticks, mut ticks, mut worst) = (0, 0u32, 0.0f64);
    while s.fx.server.body_count() > 0 && ticks < 20 * 60 {
        let t = std::time::Instant::now();
        s.run(1);
        worst = worst.max(t.elapsed().as_secs_f64() * 1000.0);
        body_ticks += s.fx.server.body_count();
        ticks += 1;
    }
    let hs: Vec<_> = ids
        .iter()
        .filter_map(|&id| s.fx.server.body_handle(id))
        .collect();
    let ph = s.fx.server.physics();
    let left: Vec<(DVec3, DVec3)> = hs
        .iter()
        .map(|&h| (ph.position(h), ph.velocity(h)))
        .collect();
    assert_eq!(
        s.fx.server.body_count(),
        0,
        "every piece settled within 20 s: left {left:?}"
    );
    (n, body_ticks, ticks, worst)
}

#[test]
fn a_surface_blast_flings_about_half_the_pieces_out_of_its_crater_and_they_settle() {
    // face-a-only: a cost bound on the flat world (face A's); the blast's directions are tested on
    // both faces below. Owner (2026-11-06): pushed radially, half the shell was driven into the
    // ground, "where it jams and takes 10–30 s to settle". Before: every shell cell flew along its
    // line from the blast (path 0, no blend).
    let now = surface_settle(dynamite());
    let before = surface_settle(ExplosiveData {
        open_path_m: 0.0,
        open_blend: 0.0,
        ..before_the_flush_fix()
    });
    println!("now {now:?}, before {before:?} (pieces, body-ticks, ticks, worst ms)");
    // Natively on the flat ground the pieces once driven into it came to rest at once; flung up
    // out of the crater they fly and land: fewer bodies, not fewer body-ticks (docs/EXPLOSIVES.md
    // §3.2). At 5 m, every piece flung alike at twice the impulse (§3.3): ~186 pieces against
    // ~442, ~49,000 body-ticks (~11 s).
    assert!(now.0 < before.0 * 3 / 5, "pieces {} vs {}", now.0, before.0);
    assert!(
        now.1 <= 60_000,
        "body-ticks {} (before {})",
        now.1,
        before.1
    );
}

/// Blows dynamite in face `f`'s scene after `shape` has laid out the rock round it (face-local
/// cells; the charge's at the origin): the mean of its pieces' velocities, face-local, and their
/// count.
fn blast_mean(f: FaceFrame, shape: fn([i32; 3]) -> bool) -> ([f64; 3], usize) {
    let mut s = Scene::new(f, 1);
    s.put(1, 0.5, -12.5);
    s.run(2);
    let stone = require("dwell:stone");
    for o in cells_within(6.0) {
        let m = if shape(o) { stone } else { M.air };
        s.set(o, m);
    }
    let lit = dwell_world::explosives::lit_state(&dynamite()).expect("lit");
    s.set([0, 0, 0], lit);
    let at = s.cell([0, 0, 0]);
    s.fx.server.light_fuse(at, 0, 0.0);
    let out = s.run(2);
    let pieces = spawns(&out);
    let mut sum = DVec3::ZERO;
    for p in &pieces {
        let h = s.fx.server.body_handle(p.id).expect("a body");
        sum += s.fx.server.physics().velocity(h);
    }
    let mean = sum / pieces.len().max(1) as f64;
    (f.vector(mean.to_array()), pieces.len())
}

/// A named face-local fixture (which cells are solid) and the way its pieces should leave.
type Case = (&'static str, fn([i32; 3]) -> bool, [f64; 3]);

dwell_world::both_faces!(
    fn the_pieces_leave_through_the_open_side(f: FaceFrame) {
        // Owner (2026-11-06): "putting dynamite on the side of a cliff or the underside of a cave
        // roof would blast out or down".
        let cases: [Case; 3] = [
            ("flat ground", |o| o[1] < 0, [0.0, 1.0, 0.0]),
            ("a cliff face", |o| o[0] < 0, [1.0, 0.0, 0.0]),
            ("a cave roof", |o| o[1] > 0, [0.0, -1.0, 0.0]),
        ];
        for (name, shape, want) in cases {
            let (mean, n) = blast_mean(f, shape);
            let v = DVec3::from(mean);
            let cos = v.normalize_or_zero().dot(DVec3::from(want));
            println!("{name}: {n} pieces, mean {v:.2?}, cos {cos:.2}");
            assert!(n > 30, "{name}: {n} pieces");
            assert!(cos > 0.7, "{name}: mean velocity {v:.2?} (cos {cos:.2})");
        }
        // Buried: no way out, so the shell stays (a cavity), nothing pushed one way.
        let (mean, n) = blast_mean(f, |o| o != [0, 0, 0]);
        assert!(
            n == 0 || DVec3::from(mean).length() < 1.0,
            "buried: {n}, {mean:?}"
        );
    }
);

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test explosives measure_dynamite_configs -- --ignored --nocapture"]
fn measure_dynamite_configs() {
    // face-a-only: a measurement on the flat world. The owner's four (2026-11-06): A 4 m ×1.5,
    // B A + no contact 1 s, C loosened to 5 m, D C + no contact 1 s.
    let a = ExplosiveData {
        no_piece_contact_s: 0.0,
        loosen_radius_m: 4.0,
        ..dynamite()
    };
    for (name, e) in [
        ("A", a),
        (
            "B",
            ExplosiveData {
                no_piece_contact_s: 1.0,
                ..a
            },
        ),
        (
            "C",
            ExplosiveData {
                loosen_radius_m: 5.0,
                ..a
            },
        ),
        (
            "D",
            ExplosiveData {
                loosen_radius_m: 5.0,
                no_piece_contact_s: 1.0,
                ..a
            },
        ),
    ] {
        let mut s = Scene::flat(1);
        let n = buried_blast(&mut s, e).len();
        let (mut body_ticks, mut ticks, mut worst, mut first_s) = (0, 0, 0.0f64, 0.0);
        let mut physics = 0.0;
        while s.fx.server.body_count() > 0 && ticks < 30 * 60 {
            s.fx.server.tick_phases();
            let t = std::time::Instant::now();
            s.run(1);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let ph = s.fx.server.tick_phases();
            physics += ph
                .iter()
                .find(|(n, _)| *n == "physics")
                .map_or(0.0, |p| p.1);
            if ticks % 120 == 60 {
                println!(
                    "  {name} {} s: {} bodies, {} apart, {} piece pairs, {ms:.1} ms: {:?}",
                    ticks / 60,
                    s.fx.server.body_count(),
                    s.fx.server.apart_pieces(),
                    s.fx.server.physics().piece_contact_pairs(),
                    ph.iter().filter(|p| p.1 > 0.3).collect::<Vec<_>>()
                );
            }
            worst = worst.max(ms);
            if ticks < 60 {
                first_s += ms;
            }
            body_ticks += s.fx.server.body_count();
            ticks += 1;
        }
        println!(
            "{name}: {n} pieces, first second {first_s:.0} ms, worst tick {worst:.1} ms, physics {physics:.0} ms, settled in {:.1} s ({} left), {body_ticks} body-ticks",
            f64::from(ticks) / 60.0,
            s.fx.server.body_count()
        );
    }
}

#[test]
fn a_world_saved_with_a_fuse_burning_lights_it_again_when_it_loads() {
    // face-a-only: saving is face-free; the flat test world is face A's.
    let dir = std::env::temp_dir().join(format!("dwell-explosives-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join(format!("fuse{}", dwell_storage::WORLD_EXTENSION));
    let g = FLAT_TOP_Y;
    let place = |cell: [i32; 3], m: StateId| {
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell,
            face: 2,
            material: m,
            rows: 1,
            down: false,
            point: None,
        })
    };
    {
        let store = dwell_storage::WorldStore::open_file(&path).expect("opens");
        let mut f = Fixture::new(ServerConfig {
            store: Some(store),
            ..flat()
        });
        f.join(1, &Client::new(1));
        f.server.step();
        f.send(1, place([-1, g - 1, 2], require("dwell:dynamite")));
        f.server.step();
        f.send(1, place([-1, g, 2], require("dwell:torch")));
        for _ in 0..4 {
            f.server.step();
        }
        assert_eq!(
            f.server.world().get_voxel(-1, g, 2),
            require("dwell:lit_dynamite")
        );
        f.server.save_now();
        f.server.flush_store();
    }
    let store = dwell_storage::WorldStore::open_file(&path).expect("reopens");
    let mut f = Fixture::new(ServerConfig {
        store: Some(store),
        ..flat()
    });
    assert_eq!(
        f.server.world().get_voxel(-1, g, 2),
        require("dwell:lit_dynamite"),
        "saved lit"
    );
    f.server.step();
    for _ in 0..200 {
        f.server.step();
    }
    assert_eq!(
        f.server.world().get_voxel(-1, g, 2),
        M.air,
        "its fuse ran again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a charge set flush in the flat world's ground (its top level with the surface) does.
#[derive(Debug)]
struct Flush {
    /// The pieces it flung.
    pieces: usize,
    /// Of them, those that came to rest (their last pose) farther than `radiusM` across the ground
    /// from the charge: clear of the crater's rim.
    clear: usize,
    /// Seconds until every piece was a block again (at most 20).
    settled_s: f64,
    /// Air cells under the surface within `radiusM` of the charge once settled: the crater.
    crater: usize,
    /// Body-ticks until settled.
    body_ticks: usize,
}

/// Blows `e` set flush in the flat world's ground (face A's, endless: what flies far lands).
fn flush_blast(e: ExplosiveData) -> Flush {
    let mut s = Scene::flat(1);
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -24.0);
    s.run(2);
    // The charge's cell is the ground's top row (row −1): its top flush with the surface.
    let lit = dwell_world::explosives::lit_state(&e).expect("lit");
    s.set([0, -1, 0], lit);
    let at = s.cell([0, -1, 0]);
    s.fx.server.light_fuse(at, 0, 0.0);
    let ids: Vec<u32> = spawns(&s.run(2)).iter().map(|p| p.id).collect();
    let centre = DVec3::new(0.5, m_of_row(s.base - 1) + CELL_H / 2.0, 0.5);
    let mut last = vec![centre; ids.len()];
    let (mut ticks, mut body_ticks) = (0u32, 0);
    loop {
        for (k, &id) in ids.iter().enumerate() {
            if let Some(h) = s.fx.server.body_handle(id) {
                last[k] = s.fx.server.physics().position(h);
            }
        }
        if s.fx.server.body_count() == 0 || ticks >= 20 * 60 {
            break;
        }
        s.run(1);
        body_ticks += s.fx.server.body_count();
        ticks += 1;
    }
    let clear = last
        .iter()
        .filter(|p| (p.x - centre.x).hypot(p.z - centre.z) > e.radius_m)
        .count();
    let mut crater = 0;
    let w = e.radius_m.ceil() as i32 + 1;
    for x in -w..=w {
        for z in -w..=w {
            for r in -12..0 {
                if dist([x, r + 1, z]) <= e.radius_m && s.get([x, r, z]) == M.air {
                    crater += 1;
                }
            }
        }
    }
    Flush {
        pieces: ids.len(),
        clear,
        settled_s: f64::from(ticks) / 60.0,
        crater,
        body_ticks,
    }
}

/// 0.49.45's numbers at a 5 m radius: the blast the owner found "just cracks the surface".
fn before_the_flush_fix() -> ExplosiveData {
    ExplosiveData {
        impulse: 13_500.0,
        fling_falloff: 1.5,
        open_spread_min: 0.0,
        ..dynamite()
    }
}

#[test]
fn a_flush_charge_throws_its_debris_clear_and_leaves_a_crater() {
    // face-a-only: a measurement on the flat world (face A's); the pushes' directions are tested
    // on both faces above. Owner (2026-11-06): a charge level with flat ground "just cracks the
    // surface with little to no changes", and "most of the debris does fall right back in".
    let now = flush_blast(dynamite());
    let before = flush_blast(before_the_flush_fix());
    println!("now {now:?}\nbefore {before:?}");
    // Before: 250 pieces, 10 clear of the rim, a 43-cell crater (the 2 m core, part refilled).
    // Now (natively): ~214 pieces, ~90 clear, a ~125-cell crater, settled in ~10 s.
    assert!(now.clear * 100 >= now.pieces * 35, "{now:?}");
    assert!(now.clear >= before.clear * 4, "{now:?} vs {before:?}");
    assert!(now.crater * 2 >= before.crater * 5, "{now:?} vs {before:?}");
    assert!(now.settled_s <= 15.0, "{now:?}");
    assert!(now.body_ticks <= 80_000, "{now:?}");
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test explosives measure_flush_charge -- --ignored --nocapture"]
fn measure_flush_charge() {
    // face-a-only: a measurement on the flat world, the configurations compared in
    // docs/EXPLOSIVES.md §3.3: impulse ×k, fling falloff, open blend, sideways floor, no-contact s.
    let d = dynamite();
    let old = before_the_flush_fix();
    let mut configs = vec![("0.49.45 at 5 m".to_string(), old), ("now".to_string(), d)];
    for (k, falloff, blend, spread, nc) in [
        (2.0, 1.0, 0.6, 0.7, 0.0),
        (4.0, 1.0, 0.6, 0.7, 0.0),
        (2.0, 1.0, 0.6, 0.7, 0.5),
        (1.5, 0.0, 0.6, 0.7, 0.0),
        (2.0, 0.0, 0.6, 0.0, 0.0),
        (2.0, 0.0, 0.35, 0.7, 0.0),
        (3.0, 0.0, 0.6, 0.7, 0.0),
    ] {
        let e = ExplosiveData {
            impulse: old.impulse * k,
            max_speed_ms: old.max_speed_ms * k.min(2.0),
            fling_falloff: falloff,
            open_blend: blend,
            open_spread_min: spread,
            no_piece_contact_s: nc,
            ..d
        };
        let name =
            format!("x{k} fling falloff {falloff} blend {blend} spread {spread} no-contact {nc}");
        configs.push((name, e));
    }
    for (name, e) in configs {
        println!("{name}: {:?}", flush_blast(e));
    }
}

fn yellow() -> ExplosiveData {
    *EXPLOSIVES
        .iter()
        .find(|e| e.name == "dwell:dynamite_yellow")
        .expect("yellow dynamite")
}

/// Blows `e` (the table's only entry) in face `f`'s scene, the rock round it everything within
/// 7 m but a cliff open from face-local x = 5 on: the blast tick's pieces (each its face-local cell
/// and speed) and the scene after it.
fn cliff_blast(f: FaceFrame, e: ExplosiveData) -> (Vec<([i32; 3], f64)>, Scene) {
    let mut s = Scene::new(f, 1);
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -12.5);
    s.run(2);
    let stone = require("dwell:stone");
    for o in cells_within(e.radius_m.max(e.loosen_radius_m).max(7.0) + 2.0) {
        s.set(o, if o[0] >= 5 { M.air } else { stone });
    }
    let lit = dwell_world::explosives::lit_state(&e).expect("lit");
    s.set([0, 0, 0], lit);
    let at = s.cell([0, 0, 0]);
    s.fx.server.light_fuse(at, 0, 0.0);
    let out = s.run(2);
    let base = s.cell([0, 0, 0]);
    let pieces = spawns(&out)
        .into_iter()
        .map(|p| {
            // The piece's world cell less the charge's, back through the frame (face-local).
            let w = [
                p.origin[0].floor() as i32,
                (p.origin[1] / CELL_H).floor() as i32,
                p.origin[2].floor() as i32,
            ];
            let local = f.cell(w);
            let b = f.cell(base);
            let o = [local[0] - b[0], local[1] - b[1], local[2] - b[2]];
            let h = s.fx.server.body_handle(p.id).expect("a body");
            (o, s.fx.server.physics().velocity(h).length())
        })
        .collect();
    (pieces, s)
}

dwell_world::both_faces!(
    fn yellow_cracks_cells_open_above_or_below_and_not_those_open_only_to_a_side(f: FaceFrame) {
        // Owner (2026-11-07): "destroys 4m radius, and cracks anything touching air"; the roof over
        // the cleared core drops; then (2026-11-09): "anything touching air on top or bottom,
        // exclude blocks that are only touching 'side' air". A charge in rock beside a cliff open
        // from x 5: cells beside the cliff face are open only to their side, so they stay; the
        // roof over the cleared core still drops; cells pushed into rock stay.
        let e = yellow();
        assert_eq!(e.shell, dwell_world::explosives::ShellRule::Crack);
        assert!(e.crack_vertical_only);
        let (pieces, mut s) = cliff_blast(f, e);
        let stone = require("dwell:stone");
        let got: std::collections::HashMap<[i32; 3], f64> = pieces.iter().copied().collect();
        // The roof over the core cracks loose (crackOverCleared).
        assert!(
            got.contains_key(&[0, 9, 0]),
            "the roof over the core drops: {:?}",
            got.keys()
        );
        // Beside the open cliff, open only to the side: stays (was flung before 0.50.55).
        assert!(
            !got.contains_key(&[4, 0, 2]),
            "a cell open only to its side stays"
        );
        assert_eq!(s.get([4, 0, 2]), stone);
        // Every piece was open above or below before the blast, or sat on the cleared core: none
        // came from the cliff face's side cells.
        for o in got.keys() {
            assert!(o[0] < 4 || o[1] != 0, "{o:?} cracked from side air alone");
        }
        // Within 4 m: gone. Cells with no open side above or below: stay.
        assert_eq!(s.get([0, 7, 2]), M.air, "3.6 m: destroyed");
        for o in [[0, -9, 0], [0, 0, -5]] {
            assert!(!got.contains_key(&o), "{o:?} stays");
            assert_eq!(s.get(o), stone, "{o:?}: stays");
        }
    }
);

dwell_world::both_faces!(
    fn a_charge_touching_water_spares_the_blocks(f: FaceFrame) {
        // Owner (2026-11-07): "Make dynamite not destroy or dislodge (physics) blocks when it's
        // touching water." A charge in rock with one water cell beside it: no cell cleared, no
        // piece; the same charge dry blows its crater (the cliff fixture's pieces).
        let stone = require("dwell:stone");
        for e in dwell_world::explosives::EXPLOSIVES {
            assert!(e.water_spares_blocks, "{}: spares blocks by water", e.name);
            let mut s = Scene::new(f, 1);
            s.fx.server.set_explosives(vec![*e]);
            s.put(1, 0.5, -12.5);
            s.run(2);
            for o in cells_within(7.0) {
                s.set(o, if o[0] >= 5 { M.air } else { stone });
            }
            s.set([1, 0, 0], M.water);
            let lit = dwell_world::explosives::lit_state(e).expect("lit");
            s.set([0, 0, 0], lit);
            let at = s.cell([0, 0, 0]);
            s.fx.server.light_fuse(at, 0, 0.0);
            let out = s.run(2);
            assert!(spawns(&out).is_empty(), "{}: no piece by water", e.name);
            for o in [[2, 0, 0], [-1, 0, 0], [0, 3, 0], [0, -3, 0], [3, 0, 2]] {
                assert_eq!(s.get(o), stone, "{}: {o:?} stays by water", e.name);
            }
        }
        // Dry, the same charge blows (red's cliff blast flings pieces).
        let (pieces, _) = cliff_blast(f, dynamite());
        assert!(!pieces.is_empty(), "dry: it blows");
    }
);

dwell_world::both_faces!(
    fn water_shields_the_rock_behind_it(f: FaceFrame) {
        // Owner (2026-11-07): "it shouldn't destroy blocks or move them through water, so on the
        // other side of water blocks". Dynamite in rock with a wall of water 2 m to its +x (not
        // touching it): the rock behind the water stays; the other side is cleared. Blue (it
        // destroys everything within 4 m), so without the shield the rock 3 m out behind the water
        // would be destroyed.
        let stone = require("dwell:stone");
        let e = *dwell_world::explosives::EXPLOSIVES
            .iter()
            .find(|e| e.name == "dwell:dynamite_blue")
            .expect("blue dynamite");
        assert!(e.water_shields);
        let mut s = Scene::new(f, 1);
        s.fx.server.set_explosives(vec![e]);
        s.put(1, 0.5, -12.5);
        s.run(2);
        for o in cells_within(7.0) {
            s.set(o, if o[0] == 2 { M.water } else { stone });
        }
        let lit = dwell_world::explosives::lit_state(&e).expect("lit");
        s.set([0, 0, 0], lit);
        let at = s.cell([0, 0, 0]);
        s.fx.server.light_fuse(at, 0, 0.0);
        s.run(2);
        for o in [[3, 0, 0], [3, 1, 1], [3, -2, 0]] {
            assert_eq!(s.get(o), stone, "{o:?} behind the water stays");
        }
        // The other side: within the 4 m core, gone.
        for o in [[-1, 0, 0], [-3, 0, 0], [0, 0, -3]] {
            assert_ne!(s.get(o), stone, "{o:?} on the dry side is cleared");
        }
    }
);

/// Yellow on flat ground, flush with the surface: (cells destroyed, pieces flung, flung pieces by
/// whether they were surface cells).
fn flat_yellow(f: FaceFrame, e: ExplosiveData) -> (usize, Vec<[i32; 3]>) {
    let mut s = Scene::new(f, 1);
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -14.5);
    s.run(2);
    let stone = require("dwell:stone");
    let reach = e.radius_m.max(e.loosen_radius_m) + 2.0;
    for o in cells_within(reach) {
        s.set(o, if o[1] <= 0 { stone } else { M.air });
    }
    let lit = dwell_world::explosives::lit_state(&e).expect("lit");
    s.set([0, 0, 0], lit);
    let at = s.cell([0, 0, 0]);
    s.fx.server.light_fuse(at, 0, 0.0);
    let out = s.run(2);
    let base = f.cell(s.cell([0, 0, 0]));
    let flung: Vec<[i32; 3]> = spawns(&out)
        .into_iter()
        .map(|p| {
            let w = [
                p.origin[0].floor() as i32,
                (p.origin[1] / CELL_H).floor() as i32,
                p.origin[2].floor() as i32,
            ];
            let l = f.cell(w);
            [l[0] - base[0], l[1] - base[1], l[2] - base[2]]
        })
        .collect();
    let gone = cells_within(reach)
        .into_iter()
        .filter(|&o| o[1] <= 0 && o != [0, 0, 0] && s.get(o) != stone)
        .count();
    (gone - flung.len(), flung)
}

#[test]
#[ignore = "a measurement: prints yellow's flat-ground counts"]
fn measure_yellow_on_flat_ground() {
    // face-a-only: a printout, not an assertion.
    let (destroyed, flung) = flat_yellow(FaceFrame::A, yellow());
    let surface = flung.iter().filter(|o| o[1] == 0).count();
    let far = flung
        .iter()
        .map(|o| ((o[0] * o[0] + o[2] * o[2]) as f64).sqrt())
        .fold(0.0, f64::max);
    println!(
        "destroyed {destroyed}, flung {} ({surface} surface cells, farthest {far:.1} m out)",
        flung.len()
    );
}

dwell_world::both_faces!(
    fn yellow_cracks_the_surface_out_to_seven_metres(f: FaceFrame) {
        // Owner (2026-11-07): "increase the radius of the yellow dynamite by 2 meters, for cracking
        // range only". Flush on flat ground: the surface cracks loose out to 7 m (was 5 m), the
        // destroyed core stays 4 m, and nothing past 7 m moves.
        let e = yellow();
        assert_eq!((e.destroy_radius_m, e.loosen_radius_m), (4.0, 7.0));
        let (_, flung) = flat_yellow(f, e);
        let out = |o: &[i32; 3]| {
            let y = f64::from(o[1]) * CELL_H;
            (f64::from(o[0] * o[0] + o[2] * o[2]) + y * y).sqrt()
        };
        assert!(
            flung.iter().any(|o| o[1] == 0 && out(o) > 6.0),
            "a surface cell past 6 m cracks loose"
        );
        assert!(flung.iter().all(|o| out(o) <= 7.0), "none past 7 m");
        assert!(
            flung.iter().all(|o| out(o) > 3.0),
            "the core is destroyed, not flung"
        );
    }
);

dwell_world::both_faces!(
    fn yellow_does_not_free_cells_beside_its_destroyed_core(f: FaceFrame) {
        // Owner (2026-11-07): the cells below and beside the crater "aren't supposed to go live next
        // to the pieces that are destroyed": the air check is the world before the blast. Flush on
        // flat ground only the surface (the cells that touched air) cracks loose; 0.50.9 also flung
        // 266 cells under the ground (through the cleared core and behind the flying surface).
        let (destroyed, flung) = flat_yellow(f, yellow());
        assert_eq!(destroyed, 280, "the 4 m core");
        let under: Vec<_> = flung.iter().filter(|o| o[1] != 0).collect();
        assert!(
            under.is_empty(),
            "{} cells under the surface flung, e.g. {:?}",
            under.len(),
            under.first()
        );
        assert_eq!(flung.len(), 100, "the surface ring cracks");
    }
);

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test explosives measure_blast_cost -- --ignored --nocapture"]
fn measure_blast_cost() {
    // face-a-only: a measurement on the flat world (docs/DEV_TOOLS.md "Blasts and floods").
    let yellow = *EXPLOSIVES
        .iter()
        .find(|e| e.name == "dwell:dynamite_yellow")
        .expect("yellow");
    for (name, e, buried) in [
        ("red flush", dynamite(), false),
        ("red buried", dynamite(), true),
        ("yellow flush", yellow, false),
    ] {
        let e = ExplosiveData {
            no_piece_contact_s: std::env::var("DWELL_LEVER_NOCONTACT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(e.no_piece_contact_s),
            ..e
        };
        let mut s = Scene::flat(1);
        let ids: Vec<u32> = if buried {
            buried_blast(&mut s, e).iter().map(|p| p.id).collect()
        } else {
            s.fx.server.set_explosives(vec![e]);
            s.put(1, 0.5, -3.5);
            s.run(2);
            s.set([0, 0, 0], require(e.block));
            s.light([0, 0, 0]);
            s.run(1);
            s.put(1, 0.5, -13.0);
            let fuse = s.fx.server.fuses()[0].1 as usize;
            s.run(fuse - 1);
            spawns(&s.run(1)).iter().map(|p| p.id).collect()
        };
        let n = ids.len();
        let (mut ticks, mut worst, mut first_s, mut total) = (0u32, 0.0f64, 0.0, 0.0);
        let (mut physics, mut terrain) = (0.0, 0.0);
        let mut tick_ms = Vec::new();
        s.fx.server.tick_phases();
        while s.fx.server.body_count() > 0 && ticks < 30 * 60 {
            let t = std::time::Instant::now();
            s.run(1);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let ph = s.fx.server.tick_phases();
            let get = |k: &str| ph.iter().find(|(n, _)| *n == k).map_or(0.0, |p| p.1);
            physics += get("physics");
            terrain += get("terrain");
            worst = worst.max(ms);
            total += ms;
            tick_ms.push(ms);
            if ticks < 60 {
                first_s += ms;
            }
            ticks += 1;
        }
        tick_ms.sort_by(f64::total_cmp);
        let p95 = tick_ms
            .get(tick_ms.len() * 95 / 100)
            .copied()
            .unwrap_or(0.0);
        println!(
            "{name}: {n} pieces, tick mean {:.2} p95 {p95:.2} worst {worst:.1} ms, first second {first_s:.0} ms, physics {physics:.0} ms, terrain {terrain:.0} ms, settled in {:.1} s ({} left)",
            total / f64::from(ticks.max(1)),
            f64::from(ticks) / 60.0,
            s.fx.server.body_count()
        );
    }
}

#[test]
fn a_chained_column_of_yellow_destroys_the_pieces_left_in_each_later_blasts_core() {
    // face-a-only: the flat world's fixture; the destroy rule is a distance test in world space.
    // Owner (2026-11-09): "Dynamite doesn't destroy nearby physics blocks in its destroy area, so
    // chained dynamite still results in a ton of physics blocks that are affected by multiple
    // explosion forces. Make it so physics blocks in the destroy radius are destroyed too." The
    // owner's case: a 1-wide, 10-tall stack of yellow dynamite going down into the ground.
    let e = yellow();
    let mut s = Scene::flat(1);
    s.fx.server.set_explosives(vec![e]);
    s.put(1, 0.5, -3.5);
    s.run(2);
    let column: Vec<[i32; 3]> = (0..10).map(|k| [0, -2 * k, 0]).collect();
    for &c in &column {
        s.set(c, require(e.block));
    }
    s.light(column[0]);
    s.run(1);
    assert!(!s.fx.server.fuses().is_empty(), "the top charge is lit");
    s.put(1, 0.5, -30.0);
    let mut live: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut charges: Vec<[i32; 3]> = column.iter().map(|&c| s.cell(c)).collect();
    let mut survivors = 0;
    for _ in 0..(20 * 60) {
        // Where every live piece is just before this tick (a blast this tick judges them there).
        let before: Vec<(u32, DVec3)> = live
            .iter()
            .filter_map(|&id| {
                let h = s.fx.server.body_handle(id)?;
                Some((id, s.fx.server.physics().position(h)))
            })
            .collect();
        let out = s.run(1);
        for sp in spawns(&out) {
            live.insert(sp.id);
        }
        for m in &out {
            if let Message::EntityDespawn(id) = m {
                live.remove(id);
            }
        }
        // The charges that blew this tick: their cells no longer hold the explosive.
        let blown: Vec<[i32; 3]> = charges
            .iter()
            .copied()
            .filter(|&c| {
                let m = s.fx.server.world().get_voxel(c[0], c[1], c[2]);
                dwell_world::explosives::explosive_of(&[e], m).is_none()
            })
            .collect();
        charges.retain(|c| !blown.contains(c));
        for c in blown {
            let centre = dwell_sim::felling::cell_centre(c);
            survivors += before
                .iter()
                .filter(|(id, p)| (*p - centre).length() <= e.destroy_radius_m && live.contains(id))
                .count();
        }
        if charges.is_empty() {
            break;
        }
    }
    assert!(charges.is_empty(), "the whole column blew");
    assert_eq!(
        survivors, 0,
        "no piece inside a later blast's destroy radius survives it"
    );
}

/// A chain in face `f`'s scene (none: the flat world, face A's, endless ground): `charges` (face-local) of `e` set in stone 24 rows deep, the first
/// lit, run until every charge has blown, no piece is queued and none spawned for half a second, or
/// `max` ticks (with `settle`, until the pieces have settled too): each tick's (ms, pieces spawned, phases), the cells the chain cleared within 12 m and
/// the pieces spawned.
/// A tick's phases (`Server::tick_phases`).
type Phases = [(&'static str, f64); 14];

struct Chain {
    ticks: Vec<(f64, usize, Phases)>,
    cleared: usize,
    pieces: usize,
    /// Every cell the blasts' `Explosion` modifications set (chunk, index), and how many there were.
    exploded: std::collections::BTreeSet<([i32; 3], u16)>,
    booms: usize,
}

fn solid_round(s: &mut Scene) -> usize {
    let mut n = 0;
    for x in -12..=12 {
        for z in -12..=12 {
            for r in -24..4 {
                let m = s.get([x, r, z]);
                if m != M.air && !info(m).liquid {
                    n += 1;
                }
            }
        }
    }
    n
}

fn chain(
    f: Option<FaceFrame>,
    e: ExplosiveData,
    charges: &[[i32; 3]],
    max: usize,
    settle: bool,
    per_tick: Option<usize>,
) -> Chain {
    let mut s = match f {
        Some(f) => Scene::new(f, 1),
        None => Scene::flat(1),
    };
    s.fx.server.set_explosives(vec![e]);
    // None: the data's budget.
    s.fx.server.set_pieces_per_tick(
        per_tick.unwrap_or(dwell_world::explosives::LIMITS.pieces_per_tick as usize),
    );
    s.put(1, 0.5, -3.5);
    s.run(2);
    if f.is_some() {
        // Deeper stone than the scene's six rows, so a chain going down stays in rock.
        let stone = require("dwell:stone");
        for x in -14..=14 {
            for z in -14..=14 {
                for r in -24..-6 {
                    s.set([x, r, z], stone);
                }
            }
        }
    }
    for &c in charges {
        s.set(c, require(e.block));
    }
    let before = solid_round(&mut s);
    s.light(charges[0]);
    s.run(1);
    s.put(1, 0.5, -13.5);
    s.fx.server.tick_phases();
    let mut ticks = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut quiet = 0;
    let (mut exploded, mut booms) = (std::collections::BTreeSet::new(), 0);
    for _ in 0..max {
        let t = std::time::Instant::now();
        let out = s.run(1);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let new = spawns(&out).iter().filter(|p| ids.insert(p.id)).count();
        for m in &out {
            if let Message::VoxelModification(v) = m
                && v.reason == VoxelModificationReason::Explosion
            {
                booms += 1;
                for c in &v.chunks {
                    for ch in &c.changes {
                        exploded.insert((c.coord, ch.index));
                    }
                }
            }
        }
        ticks.push((ms, new, s.fx.server.tick_phases()));
        let done = s.fx.server.fuses().is_empty() && s.fx.server.queued_pieces() == 0;
        quiet = if done && new == 0 { quiet + 1 } else { 0 };
        if quiet > 30 && (!settle || s.fx.server.body_count() == 0) {
            break;
        }
    }
    let after = solid_round(&mut s);
    Chain {
        ticks,
        cleared: before - after,
        pieces: ids.len(),
        exploded,
        booms,
    }
}

/// The 3 × 9 block of red dynamite on the ground.
fn red_block() -> Vec<[i32; 3]> {
    let mut v = vec![[0, 0, 0]];
    for z in 0..9 {
        for x in -1..=1 {
            if [x, z] != [0, 0] {
                v.push([x, 0, z]);
            }
        }
    }
    v
}

/// The 10-tall column of yellow going down into the ground.
fn yellow_column() -> Vec<[i32; 3]> {
    (0..10).map(|k| [0, -2 * k, 0]).collect()
}

#[test]
#[ignore = "a measurement: cargo test --release -p dwell-sim --test explosives measure_chain_ticks -- --ignored --nocapture"]
fn measure_chain_ticks() {
    // face-a-only: a measurement; the parity is in the chained tests.
    let per_tick = std::env::var("DWELL_PIECES_PER_TICK")
        .ok()
        .and_then(|v| v.parse().ok());
    for (name, e, charges) in [
        ("red 3x9", dynamite(), red_block()),
        ("yellow column", yellow(), yellow_column()),
    ] {
        let c = chain(None, e, &charges, 30 * 60, true, per_tick);
        let (wi, w) = c
            .ticks
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.0.total_cmp(&b.1.0))
            .expect("ticks");
        let mut sorted: Vec<f64> = c.ticks.iter().map(|t| t.0).collect();
        sorted.sort_by(f64::total_cmp);
        let over = sorted.iter().filter(|&&m| m > 16.7).count();
        let most = c.ticks.iter().map(|t| t.1).max().unwrap_or(0);
        // The worst tick that spawned pieces, and the worst of the half second after one.
        let spawning = c.ticks.iter().filter(|t| t.1 > 0).map(|t| t.0);
        let worst_spawn = spawning.fold(0.0f64, f64::max);
        let near = (0..c.ticks.len())
            .filter(|&k| c.ticks[k.saturating_sub(30)..=k].iter().any(|t| t.1 > 0))
            .map(|k| c.ticks[k].0)
            .fold(0.0f64, f64::max);
        println!(
            "{name}: worst spawning tick {worst_spawn:.1} ms, worst within 0.5 s of a spawn {near:.1} ms"
        );
        let phases: Vec<String> =
            w.2.iter()
                .filter(|p| p.1 > 0.5)
                .map(|p| format!("{} {:.1}", p.0, p.1))
                .collect();
        println!(
            "{name}: {} pieces, {} cells cleared, {} ticks, worst {:.1} ms at tick {wi} ({} spawned; {}), p95 {:.1}, over 16.7 ms {over}, most spawned a tick {most}",
            c.pieces,
            c.cleared,
            c.ticks.len(),
            w.0,
            w.1,
            phases.join(", "),
            sorted[sorted.len() * 95 / 100],
        );
    }
}

dwell_world::both_faces!(
    fn a_chains_pieces_spawn_within_the_tick_budget_to_the_same_end(f: FaceFrame) {
        // A big chain's pieces are spread over the ticks (`limits.piecesPerTick`, docs/EXPLOSIVES.md
        // §3.12): no tick spawns more than the budget, and the chain clears the same cells, booms
        // the same blasts (one modification each) and spawns the same pieces as with no budget.
        let red = dynamite();
        let budget = dwell_world::explosives::LIMITS.pieces_per_tick as usize;
        let whole = chain(Some(f), red, &red_block(), 6 * 60, false, Some(usize::MAX));
        let spread = chain(Some(f), red, &red_block(), 6 * 60, false, Some(budget));
        let most = |c: &Chain| c.ticks.iter().map(|t| t.1).max().unwrap_or(0);
        assert!(
            most(&whole) > budget,
            "the fixture is over budget: {}",
            most(&whole)
        );
        assert!(
            most(&spread) <= budget,
            "a tick spawned {} pieces, over the budget {budget}",
            most(&spread)
        );
        assert_eq!(
            spread.booms, whole.booms,
            "the same blasts, each one modification"
        );
        assert_eq!(spread.exploded, whole.exploded, "the same cells cleared");
        // The same pieces, but those a later blast's core destroys while they wait (judged at their
        // cell, not where a spawned one would have flown in the ticks between): within 2%.
        let gap = spread.pieces.abs_diff(whole.pieces);
        assert!(
            gap * 50 <= whole.pieces,
            "pieces spawned {} spread, {} whole",
            spread.pieces,
            whole.pieces
        );
    }
);

// Attached blocks in a blast (BUGS.md #81; owner, 2026-11-13: "Dynamite blows up (physics) tall
// grass that's wrong. It should just be destroyed if the block under it is moved or destroyed."):
// a block with `needsSupport` (tall grass, a torch, a sapling, a ladder; the rule is the data's, no
// block is named in the code) is never a flung piece, and goes with the block holding it.

/// Sets off dynamite at face-local `at` now (its fuse a few ticks), every message of the blast kept.
fn blow(s: &mut Scene, at: [i32; 3]) -> Vec<Message> {
    s.set(at, require("dwell:lit_dynamite"));
    let cell = s.cell(at);
    let i = EXPLOSIVES
        .iter()
        .position(|e| e.name == "dwell:dynamite")
        .expect("dynamite");
    s.fx.server.light_fuse(cell, i, 0.05);
    s.run(12)
}

/// Whether `m` is an attached block, read from its data: `needsSupport`, or a part of one.
fn attached(m: StateId) -> bool {
    use dwell_blocks::Support;
    let s = info(m);
    s.needs_support != Support::None
        || s.part_of
            .and_then(dwell_blocks::state)
            .is_some_and(|p| info(p).needs_support != Support::None)
}

/// The blast's pieces that carry an attached block (block data `needsSupport`).
fn attached_pieces(out: &[Message]) -> usize {
    spawns(out)
        .iter()
        .filter(|p| p.cells.iter().any(|c| attached(c.state)))
        .count()
}

fn is(s: &mut Scene, at: [i32; 3], block: &str) -> bool {
    let m = s.get(at);
    m != M.air && dwell_blocks::block_of(m).name == block
}

fn loose_count(s: &Scene, item: &str) -> usize {
    s.fx.server
        .loose_items()
        .iter()
        .filter(|l| l.stack.item == item)
        .map(|l| usize::from(l.stack.count))
        .sum()
}

dwell_world::both_faces!(
    fn a_blast_over_tall_grass_flings_no_grass_and_removes_it(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 1);
        s.put(1, 0.5, -13.5);
        let (dirt, grass) = (require("dwell:dirt"), require("dwell:tall_grass"));
        for x in -8..=8 {
            for z in -8..=8 {
                s.set([x, -1, z], dirt);
                s.set([x, 0, z], grass);
            }
        }
        let out = blow(&mut s, [0, 0, 0]);
        let pieces = spawns(&out);
        assert!(
            pieces.len() > 50,
            "the dirt and stone fly: {}",
            pieces.len()
        );
        assert_eq!(attached_pieces(&out), 0, "no grass flies");
        for x in -8..=8 {
            for z in -8..=8 {
                let d = dist([x, 0, z]);
                let here = is(&mut s, [x, 0, z], "dwell:tall_grass");
                if d <= e.radius_m {
                    assert!(!here, "the grass {d:.2} m out is removed");
                }
                if here {
                    assert!(is(&mut s, [x, -1, z], "dwell:dirt"), "no grass floats");
                }
            }
        }
        // Removed as if broken: adventure drops its fibre (a full-grown plant's is certain).
        assert!(loose_count(&s, "dwell:grass_fiber") > 20, "the grass drops");
    }
);

dwell_world::both_faces!(
    fn attached_blocks_on_flung_blocks_go_and_are_not_carried(f: FaceFrame) {
        let e = dynamite();
        let mut s = Scene::new(f, 1);
        s.put(1, 0.5, -13.5);
        // A dirt deck 4.5 m over the charge: its cells fly (within the radius), what stands on
        // them lies just past it. A torch (two cells) and a sapling as well as tall grass: the
        // rule is their `needsSupport` data alone.
        let dirt = require("dwell:dirt");
        for x in -2..=2 {
            for z in -2..=2 {
                s.set([x, 9, z], dirt);
                s.set([x, 10, z], require("dwell:tall_grass"));
            }
        }
        s.set([-1, 10, -1], require("dwell:torch"));
        s.set([-1, 11, -1], require("dwell:torch_upper"));
        s.set([1, 10, -1], require("dwell:oak_sapling"));
        assert!(dist([1, 10, 0]) > e.radius_m && dist([1, 9, 0]) <= e.radius_m);
        let out = blow(&mut s, [0, 0, 0]);
        assert_eq!(attached_pieces(&out), 0, "nothing attached is carried");
        let mut beyond = 0;
        for x in -2..=2 {
            for z in -2..=2 {
                let flown = s.get([x, 9, z]) == M.air;
                if flown && dist([x, 10, z]) > e.radius_m {
                    beyond += 1;
                }
                if flown {
                    assert_eq!(s.get([x, 10, z]), M.air, "{x},{z}: nothing left floating");
                    assert_eq!(s.get([x, 11, z]), M.air, "{x},{z}: nor the torch's top");
                }
            }
        }
        assert!(beyond >= 4, "decks past the radius flew: {beyond}");
        assert!(
            s.get([-1, 9, -1]) == M.air && s.get([1, 9, -1]) == M.air,
            "the torch's and sapling's blocks flew"
        );
    }
);

dwell_world::both_faces!(
    fn tall_grass_on_a_block_broken_by_a_tool_goes_with_it(f: FaceFrame) {
        let mut s = Scene::in_config(f, 1, "adventure", |c| c.one_hit_breaks = true);
        s.set([0, -1, 0], require("dwell:dirt"));
        s.set([0, 0, 0], require("dwell:tall_grass"));
        s.put(1, 0.5, -1.5);
        s.run(2);
        let cell = s.cell([0, -1, 0]);
        s.fx.send(
            1,
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Break,
                cell,
                face: f.cell_face(2) as u8,
                material: M.air,
                rows: 1,
                down: false,
                point: None,
            }),
        );
        s.run(3);
        assert_eq!(s.get([0, -1, 0]), M.air, "the dirt broke");
        assert_eq!(s.get([0, 0, 0]), M.air, "the grass went with it");
        assert!(loose_count(&s, "dwell:grass_fiber") >= 1, "and dropped");
    }
);

dwell_world::both_faces!(
    fn a_torch_on_sand_that_falls_goes_and_is_not_left_floating(f: FaceFrame) {
        // A torch on sand over the stone ground; the stone under the sand broken from the side
        // (a pit dug beside it): the sand falls as a piece and the torch goes, not left floating.
        let mut s = Scene::in_config(f, 1, "adventure", |c| c.one_hit_breaks = true);
        s.set([0, 0, 0], require("dwell:sand"));
        s.set([0, 1, 0], require("dwell:torch"));
        s.set([0, 2, 0], require("dwell:torch_upper"));
        s.set([0, -1, -1], M.air);
        s.put(1, 0.5, -2.5);
        s.run(2);
        let cell = s.cell([0, -1, 0]);
        s.fx.send(
            1,
            Message::BlockEditRequest(BlockEditRequest {
                action: BlockEditAction::Break,
                cell,
                face: f.cell_face(5) as u8,
                material: M.air,
                rows: 1,
                down: false,
                point: None,
            }),
        );
        let out = s.run(3);
        assert_eq!(s.get([0, -1, 0]), M.air, "the stone broke");
        assert_eq!(s.get([0, 0, 0]), M.air, "the sand fell");
        assert!(!spawns(&out).is_empty(), "as a piece");
        assert_eq!(attached_pieces(&out), 0, "carrying nothing attached");
        assert_eq!(s.get([0, 1, 0]), M.air, "the torch went");
        assert_eq!(s.get([0, 2, 0]), M.air, "and its top");
        assert!(loose_count(&s, "dwell:torch") >= 1, "dropping it");
    }
);
