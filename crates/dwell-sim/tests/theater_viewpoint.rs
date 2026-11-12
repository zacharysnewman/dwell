//! Theater mode's camera as a simulation viewpoint (docs/THEATER_MODE.md §7, T-4): in creative a
//! client's `TheaterViewpoint` brings the world near it — chunks streamed around it, creatures
//! spawned there — as near the body; in adventure the server ignores it (it never trusts the
//! client); a client that never said the `theater.viewpoint` capability is ignored too; clearing
//! it ends it (its chunks unload, its creatures despawn). Both faces.

mod common;

use common::*;
use dwell_blocks::require;
use dwell_protocol::*;
use dwell_sim::creatures::Table;
use dwell_sim::server::{ServerConfig, VIEWPOINT_MAX_M};
use dwell_world::{CHUNK_SIZE, ChunkCoord, FLAT_TOP_Y, FaceFrame, empty_generator, m_of_row};

const BASE: i32 = FLAT_TOP_Y + 40;
/// The viewpoint's distance along +x (face-local): past the render radius (12 chunks, 384 m) and
/// the creatures' despawn distance, so nothing there is the body's doing.
const FAR_X: i32 = 640;

/// A stone floor two rows deep over face-local x, z in `[x0, x0 + n)`, `[z0, z0 + n)`.
fn floor(fx: &mut Fixture, f: FaceFrame, x0: i32, z0: i32, n: i32) {
    let stone = f.state(require("dwell:stone"));
    for x in x0..x0 + n {
        for z in z0..z0 + n {
            for r in -2..0 {
                let [x, y, z] = f.cell([x, BASE + r, z]);
                fx.server.world().set_voxel(x, y, z, stone);
            }
        }
    }
}

/// A server in `mode` on an empty world: player 1 on a small floor at the origin, a floor for the
/// fixture creatures far along +x. Its client said this build's capabilities when `caps`.
fn scene(f: FaceFrame, mode: &str, caps: bool) -> Fixture {
    let mut fx = Fixture::new(ServerConfig {
        generator_override: Some(empty_generator()),
        spawn: Some(f.point([0.5, m_of_row(BASE), 0.5])),
        mode: mode.into(),
        ..flat()
    });
    fx.server.set_creature_table(Table::FIXTURE);
    floor(&mut fx, f, -1, -1, 3);
    floor(&mut fx, f, FAR_X - 2, -2, 36);
    fx.join(1, &Client::new(1));
    // Full chunks (no generator check): every chunk is sent as it is.
    fx.send(1, Message::WorldgenCheck(0));
    if caps {
        let c = dwell_protocol::capabilities::Capabilities::this_client();
        assert!(c.has(capability_names::THEATER_VIEWPOINT));
        fx.send(1, Message::Capabilities(c));
    }
    for _ in 0..5 {
        fx.server.step();
    }
    fx.take_all();
    fx
}

/// The theater camera's eye, 3 m over the far floor's middle (world).
fn far_eye(f: FaceFrame) -> [f64; 3] {
    f.point([f64::from(FAR_X) + 16.5, m_of_row(BASE) + 3.0, 16.5])
}

fn chunk_of_point(p: [f64; 3]) -> ChunkCoord {
    let s = f64::from(CHUNK_SIZE);
    ChunkCoord::new(
        (p[0] / s).floor() as i32,
        (p[1] / s).floor() as i32,
        (p[2] / s).floor() as i32,
    )
}

/// Chunks within one chunk of `c`.
fn near(a: ChunkCoord, c: ChunkCoord) -> bool {
    (a.x - c.x).abs() <= 1 && (a.y - c.y).abs() <= 1 && (a.z - c.z).abs() <= 1
}

/// Creatures within 100 m of `p`.
fn creatures_near(fx: &Fixture, p: [f64; 3]) -> usize {
    fx.server
        .creatures()
        .iter()
        .filter(|c| (0..3).map(|k| (c.position[k] - p[k]).powi(2)).sum::<f64>() < 100.0 * 100.0)
        .count()
}

/// Steps `ticks`, returning the chunks sent to player 1 and its unloads.
fn stream_for(fx: &mut Fixture, ticks: i32) -> (Vec<ChunkCoord>, Vec<ChunkCoord>) {
    let mut unloads = Vec::new();
    let sent = fx.chunks(1, ticks, Some(&mut unloads));
    (
        sent.iter().map(coord_of).collect(),
        unloads
            .iter()
            .flat_map(|u| u.coords.iter().map(|&c| ChunkCoord::from_array(c)))
            .collect(),
    )
}

dwell_world::both_faces!(
    fn creative_streams_and_simulates_around_a_distant_viewpoint(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        let eye = far_eye(f);
        let at = chunk_of_point(eye);
        // The body alone brings nothing there.
        let (sent, _) = stream_for(&mut fx, 120);
        assert!(!sent.iter().any(|&c| near(c, at)), "nothing there yet");
        assert_eq!(creatures_near(&fx, eye), 0);

        fx.send(1, Message::TheaterViewpoint(Some(eye)));
        assert_eq!(fx.server.viewpoint_of_player(1), Some(eye));
        let mut sent = Vec::new();
        for _ in 0..30 {
            sent.extend(stream_for(&mut fx, 60).0);
            if fx.server.viewpoint_streamed(1) {
                break;
            }
        }
        assert!(
            sent.contains(&at),
            "the viewpoint's chunk is streamed ({} sent)",
            sent.len()
        );
        assert!(
            fx.server.viewpoint_streamed(1),
            "the view around it, complete"
        );
        assert!(
            fx.server.stream_stats_of(1).unwrap().viewpoint_sent > 0,
            "counted as the viewpoint's"
        );
        assert!(
            creatures_near(&fx, eye) > 0,
            "the fixture creatures spawn near the viewpoint"
        );
        // And they live there: still near it after a while, the body far away.
        stream_for(&mut fx, 600);
        assert!(creatures_near(&fx, eye) > 0, "kept near the viewpoint");
    }
);

dwell_world::both_faces!(
    fn adventure_ignores_the_viewpoint(f: FaceFrame) {
        let mut fx = scene(f, "adventure", true);
        let eye = far_eye(f);
        let at = chunk_of_point(eye);
        fx.send(1, Message::TheaterViewpoint(Some(eye)));
        assert_eq!(fx.server.viewpoint_of_player(1), None, "never used");
        let (sent, _) = stream_for(&mut fx, 360);
        assert!(!sent.iter().any(|&c| near(c, at)), "no scouting");
        assert_eq!(creatures_near(&fx, eye), 0, "nothing simulated there");
        // Its chunks outside the body's render radius aren't answered either.
        fx.send(1, Message::ChunkRequest(vec![at.to_array()]));
        let (sent, _) = stream_for(&mut fx, 30);
        assert!(!sent.contains(&at), "a request there is refused");
    }
);

dwell_world::both_faces!(
    fn a_client_without_the_capability_is_ignored(f: FaceFrame) {
        // An older client never sends one; one that sends it unannounced changes nothing.
        let mut fx = scene(f, "creative", false);
        let eye = far_eye(f);
        fx.send(1, Message::TheaterViewpoint(Some(eye)));
        assert_eq!(fx.server.viewpoint_of_player(1), None);
        let (sent, _) = stream_for(&mut fx, 240);
        assert!(!sent.iter().any(|&c| near(c, chunk_of_point(eye))));
        assert_eq!(creatures_near(&fx, eye), 0);
    }
);

dwell_world::both_faces!(
    fn the_viewpoint_ends_when_cleared(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        let eye = far_eye(f);
        let at = chunk_of_point(eye);
        fx.send(1, Message::TheaterViewpoint(Some(eye)));
        let (sent, _) = stream_for(&mut fx, 240);
        assert!(sent.contains(&at));
        assert!(creatures_near(&fx, eye) > 0);

        fx.send(1, Message::TheaterViewpoint(None));
        assert_eq!(fx.server.viewpoint_of_player(1), None, "ended at once");
        let (_, unloaded) = stream_for(&mut fx, 5);
        assert!(unloaded.contains(&at), "its chunks leave the client");
        assert_eq!(creatures_near(&fx, eye), 0, "its creatures despawn");
    }
);

dwell_world::both_faces!(
    fn a_mode_change_turns_the_viewpoint_off_and_a_far_one_is_capped(f: FaceFrame) {
        let mut fx = scene(f, "creative", true);
        // Farther than the cap: held on the way to it, VIEWPOINT_MAX_M from the body.
        let far = f.point([3.0 * VIEWPOINT_MAX_M, m_of_row(BASE) + 3.0, 0.5]);
        fx.send(1, Message::TheaterViewpoint(Some(far)));
        let used = fx.server.viewpoint_of_player(1).expect("in creative");
        let feet = fx.server.feet_of(1).unwrap();
        let d = (0..3)
            .map(|k| (used[k] - feet[k]).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(
            d <= VIEWPOINT_MAX_M + 2.0 && d > VIEWPOINT_MAX_M - 2.0,
            "{d}"
        );
        // The host switches the world to adventure: the viewpoint stops counting at once.
        assert!(fx.server.set_mode("adventure"));
        assert_eq!(fx.server.viewpoint_of_player(1), None);
    }
);
