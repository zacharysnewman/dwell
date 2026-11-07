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
    fn new(f: FaceFrame, players: usize) -> Self {
        let fx = Fixture::new(ServerConfig {
            generator_override: Some(empty_generator()),
            spawn: Some(f.point([0.5, m_of_row(BASE), -10.5])),
            ..flat()
        });
        let mut s = Self { f, fx, base: BASE };
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
        // Each flung out of the crater (face-locally up), the near ones faster.
        let up = DVec3::from(f.vector([0.0, 1.0, 0.0]));
        let mut by_d: Vec<(f64, f64)> = Vec::new();
        for p in &pieces {
            let h = s.fx.server.body_handle(p.id).expect("a body");
            let v = s.fx.server.physics().velocity(h);
            let pos = DVec3::from(p.origin);
            let d = (pos - centre).length();
            // A slow one at the edge is jostled by its packed neighbours in the step it spawned in.
            if v.length() > 1.0 {
                assert!(
                    v.dot(up) > 0.0,
                    "{:?} flung up and out: {v:?}",
                    p.cells[0].state
                );
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
            near > 3.0 * far,
            "faster near it: {near:.2} vs {far:.2} m/s"
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
    s.set([0, 0, 0], require("dwell:dynamite"));
    s.light([0, 0, 0]);
    s.run(1);
    s.put(1, 0.5, -13.0);
    let fuse = s.fx.server.fuses()[0].1 as usize;
    s.run(fuse - 1);
    let n = spawns(&s.run(1)).len();
    let (mut body_ticks, mut ticks, mut worst) = (0, 0u32, 0.0f64);
    while s.fx.server.body_count() > 0 && ticks < 20 * 60 {
        let t = std::time::Instant::now();
        s.run(1);
        worst = worst.max(t.elapsed().as_secs_f64() * 1000.0);
        body_ticks += s.fx.server.body_count();
        ticks += 1;
    }
    assert_eq!(
        s.fx.server.body_count(),
        0,
        "every piece settled within 20 s"
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
        ..dynamite()
    });
    println!("now {now:?}, before {before:?} (pieces, body-ticks, ticks, worst ms)");
    // Natively on the flat ground the pieces once driven into it came to rest at once (1.4 s,
    // ~13,400 body-ticks); flung up out of the crater they fly and land (~5.7 s, ~14,100 from
    // about half as many pieces): fewer bodies, not fewer body-ticks (docs/EXPLOSIVES.md §3.2).
    assert!(now.0 < before.0 * 3 / 5, "pieces {} vs {}", now.0, before.0);
    assert!(
        now.1 <= 20_000,
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

dwell_world::both_faces!(
    fn the_pieces_leave_through_the_open_side(f: FaceFrame) {
        // Owner (2026-11-06): "putting dynamite on the side of a cliff or the underside of a cave
        // roof would blast out or down".
        let cases: [(&str, fn([i32; 3]) -> bool, [f64; 3]); 3] = [
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
