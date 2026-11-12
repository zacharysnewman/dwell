//! The moon on the server (docs/proposals/MOON_WORLD.md §7.2, M-7b): a flier captured into the moon's
//! space and released back home with nothing jumping (≤ 1 cm), landing and walking at the moon's
//! gravity on each of its faces, the moon's chunks streamed to a `grid.frames` client only (an older
//! client never gets a grid id), and the moon's edits saved and found again on reopening.

mod common;
use common::*;
use dwell_protocol::capabilities::Capabilities;
use dwell_protocol::*;
use dwell_sim::server::{OutKind, ServerConfig, SessionId};
use dwell_sim::travel::GRID_CENTRE;
use dwell_storage::{WorldDb, WorldStore};
use dwell_world::{CELL_H, FaceFrame, GridFrame};
use dwell_worldgen::grids::{CrateredTerrain, grid};

type V3 = [f64; 3];

const NEW: SessionId = 1;
const OLD: SessionId = 2;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

struct Scene {
    fx: Fixture,
    seq: u32,
    /// Every message each session got, decoded (reliable and datagrams).
    got: Vec<(SessionId, Message)>,
}

impl Scene {
    fn new(config: ServerConfig) -> Self {
        let mut fx = Fixture::new(config);
        fx.join(NEW, &Client::new(1));
        fx.join(OLD, &Client::new(2));
        // This build's client says what it has; the other is an older build (says nothing).
        fx.send(NEW, Message::Capabilities(Capabilities::this_build()));
        fx.send(NEW, Message::SwingHello);
        for id in [NEW, OLD] {
            fx.send(id, Message::WorldgenCheck(0)); // full mode: every chunk's voxels
        }
        Self {
            fx,
            seq: 0,
            got: Vec::new(),
        }
    }

    fn server(&mut self) -> &mut dwell_sim::server::Server {
        &mut self.fx.server
    }

    /// One tick with both players' input.
    fn tick(&mut self, buttons: u16, move_y: i8) {
        self.seq += 1;
        for id in [NEW, OLD] {
            let m = Message::PlayerInput(PlayerInput {
                last_snapshot_tick: 0,
                inputs: vec![InputFrame {
                    seq: self.seq,
                    move_x: 0,
                    move_y,
                    buttons,
                    yaw: 0,
                    pitch: 0,
                }],
            });
            self.fx.server.on_datagram(id, &m.encode());
        }
        self.fx.server.step();
        for o in self.fx.server.take_outbox() {
            let _ = matches!(o.kind, OutKind::Datagram);
            if let Some(m) = Message::decode(&o.bytes) {
                self.got.push((o.session, m));
            }
        }
    }

    fn world_s(&mut self) -> f64 {
        f64::from(self.fx.server.tick()) / 60.0
    }

    fn moon_frame(&mut self) -> GridFrame {
        let t = self.world_s();
        self.fx.server.grids().frame(0, t)
    }

    fn last_snapshot(&self, id: SessionId) -> PhysicsSnapshot {
        self.got
            .iter()
            .rev()
            .find_map(|(s, m)| match m {
                Message::PhysicsSnapshot(p) if *s == id => Some(p.clone()),
                _ => None,
            })
            .expect("a snapshot")
    }

    fn grid_chunks(&self, id: SessionId) -> usize {
        self.got
            .iter()
            .filter(|(s, m)| *s == id && matches!(m, Message::ChunkData(c) if c.grid != 0))
            .count()
    }
}

fn moon_spawn(face: FaceFrame, up: f64) -> V3 {
    let moon = CrateredTerrain::for_grid("dwell:moon", ServerConfig::default().world_seed).unwrap();
    let s = moon.spawn_point(face.face);
    face.point([s[0], s[1] + up, s[2]])
}

/// Flies both players in from just outside the moon's capture sphere, captured on the way.
fn capture(sc: &mut Scene, face: FaceFrame) {
    let moon = grid("dwell:moon").unwrap();
    let target = moon_spawn(face, 4.0);
    // A point 1 m outside the capture sphere, toward the target, carried with the moon.
    let dir = sub(target, GRID_CENTRE);
    let out =
        [0, 1, 2].map(|k| GRID_CENTRE[k] + dir[k] / len(dir) * (moon.capture_m.unwrap() + 1.0));
    for _ in 0..3 {
        sc.tick(input_buttons::FLY, 0);
    }
    let f = sc.moon_frame();
    let (wp, wv) = f.release(out, [0.0; 3]);
    for pid in [1, 2] {
        assert!(sc.server().place_on_grid(pid, wp, wv));
    }
    // Inward at the moon's own pace: a few ticks of its far flight pull it over the sphere.
    let mut captured = Vec::new();
    for _ in 0..600 {
        let f = sc.moon_frame();
        for pid in [1u16, 2] {
            if let Some((0, p, _)) = sc.server().grid_state_of(pid) {
                // Steer straight in: carried with the moon, 10 m/s inward.
                let local = f.to_grid(p);
                let d = sub(GRID_CENTRE, local);
                let vin = [0, 1, 2].map(|k| d[k] / len(d) * 10.0);
                let next = [0, 1, 2].map(|k| local[k] + vin[k] / 60.0);
                let (wp, wv) = f.release(next, vin);
                sc.server().place_on_grid(pid, wp, wv);
            }
        }
        sc.tick(input_buttons::FLY, 0);
        captured.extend(sc.server().take_transfers());
        if captured.len() == 2 {
            break;
        }
    }
    assert_eq!(captured.len(), 2, "both captured: {captured:?}");
    for (pid, x) in &captured {
        assert_eq!((x.from, x.to), (0, 1), "player {pid} into the moon's space");
        let jump = len(sub(x.world_after, x.world_before));
        assert!(jump < 0.01, "player {pid} jumped {jump} m at capture");
        let dv = len(sub(x.velocity_after, x.velocity_before));
        assert!(dv < 0.05, "player {pid}'s velocity changed {dv} m/s");
    }
    // Then to just over the spawn, at rest on the moon, a few metres apart.
    for pid in [1u16, 2] {
        let side = f64::from(pid - 1) * 3.0;
        let at = sub(target, face.vector([-side, 0.0, 0.0]));
        assert!(sc.server().place_on_grid(pid, at, [0.0; 3]));
    }
}

dwell_world::both_faces!(
    /// MOON_WORLD.md §7.2: captured, landed and walking under ⅙ g on the moon's face `f`; the
    /// moon's chunks only to the `grid.frames` client and its grid in its snapshots only; then
    /// released home with no jump.
    fn a_flight_to_the_moon_lands_walks_and_comes_home(f: FaceFrame) {
        let mut sc = Scene::new(flat());
        capture(&mut sc, f);
        // Land: flight off, ⅙ g.
        for _ in 0..240 {
            sc.tick(0, 0);
        }
        let ground = moon_spawn(f, 0.0);
        for pid in [1u16, 2] {
            let (g, p, v) = sc.server().grid_state_of(pid).unwrap();
            assert_eq!(g, 1, "player {pid} on the moon");
            let above = f.vector(sub(p, ground))[1];

            assert!(
                (-0.5..2.5).contains(&above) && len(v) < 0.5,
                "player {pid} landed: {above} m over the ground at {v:?}"
            );
        }
        // Walk a second.
        let before = sc.server().grid_state_of(1).unwrap().1;
        for _ in 0..60 {
            sc.tick(0, 127);
        }
        let after = sc.server().grid_state_of(1).unwrap().1;
        let walked = f.vector(sub(after, before));
        assert!(walked[0].hypot(walked[2]) > 1.5, "walked {walked:?}");

        // The new client: the moon's chunks, its grid and frame in the snapshot, its position
        // grid-local. The older one: no grid chunk, no grid id, its world position.
        assert!(sc.grid_chunks(NEW) > 10, "the moon's chunks arrived");
        assert_eq!(sc.grid_chunks(OLD), 0, "an older client never gets them");
        let snap = sc.last_snapshot(NEW);
        let view = snap.grid.expect("the grid view");
        assert_eq!(view.grid, 1);
        let fr = sc.moon_frame();
        assert!(len(sub(view.origin, fr.origin)) < 1e3, "the moon's frame");
        assert!(len(sub(snap.local.position, after)) < 5.0, "grid-local");
        let old = sc.last_snapshot(OLD);
        assert!(old.grid.is_none(), "an older client never gets a grid id");
        let world_old = sc.server().world_state_of(2).unwrap().0;
        assert!(
            len(sub(old.local.position, world_old)) < 100.0,
            "the world position"
        );
        assert!(len(old.local.position) > 500_000.0);
        // A snapshot with a grid view round-trips; one without has none.
        let back = Message::decode(&Message::PhysicsSnapshot(snap.clone()).encode());
        assert_eq!(back, Some(Message::PhysicsSnapshot(snap)));

        // Home again: out past captureM × 1.1, flying.
        let moon = grid("dwell:moon").unwrap();
        let r = moon.capture_m.unwrap() * dwell_worldgen::grids::travel().release_factor + 5.0;
        let up = f.vector([0.0, 1.0, 0.0]);
        let out = [0, 1, 2].map(|k| GRID_CENTRE[k] + up[k] * r);
        sc.tick(input_buttons::FLY, 0);
        for pid in [1, 2] {
            assert!(sc.server().place_on_grid(pid, out, [0.0; 3]));
        }
        sc.tick(input_buttons::FLY, 0);
        let moved = sc.server().take_transfers();
        assert_eq!(moved.len(), 2, "both released: {moved:?}");
        for (pid, x) in &moved {
            assert_eq!((x.from, x.to), (1, 0));
            let jump = len(sub(x.world_after, x.world_before));
            assert!(jump < 0.01, "player {pid} jumped {jump} m at release");
        }
        assert_eq!(sc.server().grid_state_of(1).unwrap().0, 0);
        sc.tick(input_buttons::FLY, 0);
        assert_eq!(sc.last_snapshot(NEW).grid.map(|g| g.grid), Some(0));
        let unloaded = sc
            .got
            .iter()
            .any(|(s, m)| *s == NEW && matches!(m, Message::ChunkUnload(u) if u.grid == 1));
        assert!(unloaded, "the moon's chunks unloaded on the way home");
    }
);

dwell_world::both_faces!(
    /// MOON_WORLD.md §7.2: a block broken on the moon is the moon's (not the home cell of the same
    /// coordinates), saved in the world's `grid_chunks` and there again when the world reopens.
    fn the_moons_edits_are_saved_with_the_world(f: FaceFrame) {
        let db = WorldDb::in_memory().expect("db");
        let config = || ServerConfig {
            store: Some(WorldStore::new(db.clone(), false)),
            ..flat()
        };
        // The top cell of the ground under the spawn, in the moon's space.
        let moon =
            CrateredTerrain::for_grid("dwell:moon", ServerConfig::default().world_seed).unwrap();
        let s = moon.spawn_point(f.face);
        let cell = GridFrame::HOME.cell_of(f.point([s[0], s[1] - 0.5 * CELL_H, s[2]]));
        let home_before;
        {
            let mut sc = Scene::new(config());
            capture(&mut sc, f);
            let solid = sc.server().grid_voxel(1, cell).unwrap();
            assert_ne!(solid, dwell_blocks::AIR, "moon dust under the spawn");
            home_before = sc.server().world().get_voxel(cell[0], cell[1], cell[2]);
            sc.fx.send(
                NEW,
                Message::BlockEditRequest(BlockEditRequest {
                    action: BlockEditAction::Break,
                    cell,
                    face: 2,
                    material: 0,
                    rows: 1,
                    down: false,
                    point: None,
                }),
            );
            sc.tick(0, 0);
            assert_eq!(sc.server().grid_voxel(1, cell), Some(dwell_blocks::AIR));
            assert_eq!(
                sc.server().world().get_voxel(cell[0], cell[1], cell[2]),
                home_before,
                "the home cell untouched"
            );
            sc.server().save_now();
            sc.server().flush_store();
        }
        assert_eq!(
            db.grid_chunks("dwell:moon").len(),
            1,
            "one moon chunk saved"
        );
        let mut fx = Fixture::new(config());
        fx.server.step();
        assert_eq!(
            fx.server.grid_voxel(1, cell),
            Some(dwell_blocks::AIR),
            "the moon's edit kept"
        );
    }
);
