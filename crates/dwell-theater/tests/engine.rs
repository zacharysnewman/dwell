//! The camera path engine (THEATER_MODE.md §3–§4) on both faces: a path starts and ends at its
//! keyframes, moves without jumps, orbits at a constant radius, keeps a tracked target centred,
//! plays the same at any frame rate; presets expand by data alone; bad paths fail with messages.

use dwell_theater::eval::{pose_at, segment_at};
use dwell_theater::{Context, NoTargets, Path, Player, Pose, TargetRef, Targets, parse_presets};
use dwell_world::FaceFrame;
use serde_json::{Value, json};

/// The player standing at `at` (world metres).
struct Standing {
    at: [f64; 3],
}
impl Targets for Standing {
    fn position(&self, t: &TargetRef) -> Option<[f64; 3]> {
        match t {
            TargetRef::Point(p) => Some(*p),
            TargetRef::Player => Some(self.at),
            _ => None,
        }
    }
}

fn path(v: Value) -> Path {
    Path::parse(&v).unwrap_or_else(|e| panic!("{e}"))
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn angle_deg(a: [f64; 3], b: [f64; 3]) -> f64 {
    let n = (a.iter().map(|c| c * c).sum::<f64>() * b.iter().map(|c| c * c).sum::<f64>()).sqrt();
    let c = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / n;
    c.clamp(-1.0, 1.0).acos().to_degrees()
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A face-local point (100 m up the face) in the world through `f`.
fn at(f: FaceFrame, x: f64, y: f64, z: f64) -> [f64; 3] {
    f.point([x, 100.0 + y, z])
}

/// A two-keyframe path of `segment` from `a` to `b` on face `f`, looking at the origin's point.
fn two(f: FaceFrame, a: [f64; 3], b: [f64; 3], segment: Value) -> Path {
    path(json!({
        "formatVersion": 1,
        "keyframes": [
            { "t": 0, "position": a, "target": at(f, 0.0, 0.0, 0.0), "fov": 60 },
            { "t": 8, "position": b, "fov": 40 }
        ],
        "segments": [segment]
    }))
}

dwell_world::both_faces!(
    /// Every move but a pan (in place) and a track (the target's) starts at its first keyframe
    /// and ends at its last.
    fn every_move_starts_and_ends_at_its_keyframes(f: FaceFrame) {
        let (a, b) = (at(f, 10.0, 3.0, 0.0), at(f, 0.0, 6.0, 14.0));
        let c = at(f, 0.0, 0.0, 0.0);
        for seg in [
            json!({ "move": "cut" }),
            json!({ "move": "linear" }),
            json!({ "move": "ease", "easing": "inOutQuad" }),
            json!({ "move": "orbit", "around": c }),
            json!({ "move": "orbit", "around": c, "degrees": 450 }),
            json!({ "move": "dolly", "at": c }),
            json!({ "move": "crane", "upEasing": "outQuad" }),
        ] {
            let p = two(f, a, b, seg.clone());
            let start = pose_at(&p, 0.0, &NoTargets);
            let end = pose_at(&p, 8.0, &NoTargets);
            assert!(
                dist(start.position, a) < 1e-9,
                "{seg}: starts at {:?}",
                start.position
            );
            assert!(
                dist(end.position, b) < 1e-6,
                "{seg}: ends at {:?}",
                end.position
            );
            assert_eq!(start.fov_deg, Some(60.0), "{seg}");
            assert!((end.fov_deg.unwrap_or(0.0) - 40.0).abs() < 1e-9, "{seg}");
        }
        // A pan holds the position and ends on the second keyframe's look.
        let p = path(json!({ "formatVersion": 1,
            "keyframes": [{ "t": 0, "position": a, "look": [0, 10] }, { "t": 5, "look": [120, -20] }],
            "segments": [{ "move": "pan" }] }));
        let end = pose_at(&p, 5.0, &NoTargets);
        assert!(dist(end.position, a) < 1e-12);
        assert!(
            (end.yaw_deg - 120.0).abs() < 1e-6 && (end.pitch_deg + 20.0).abs() < 1e-6,
            "{end:?}"
        );
    }
);

dwell_world::both_faces!(
    /// A path of several moves played at 60 fps: no frame moves the camera or turns the look
    /// much further than the frames around it, at the joins too.
    fn a_path_moves_without_jumps(f: FaceFrame) {
        let c = at(f, 0.0, 0.0, 0.0);
        let p = path(json!({ "formatVersion": 1,
            "keyframes": [
                { "t": 0, "position": at(f, 30.0, 2.0, 0.0), "target": c },
                { "t": 4, "position": at(f, 20.0, 4.0, 0.0) },
                { "t": 10, "position": at(f, 0.0, 4.0, 20.0) },
                { "t": 14, "position": at(f, 0.0, 30.0, 25.0) },
                { "t": 18, "position": at(f, 10.0, 30.0, 25.0), "look": [90, -30] }
            ],
            "segments": [
                { "move": "ease" },
                { "move": "orbit", "around": c, "easing": "inOutSine" },
                { "move": "crane" },
                { "move": "linear", "easing": "smooth" }
            ] }));
        let dt = 1.0 / 60.0;
        let mut player = Player::new(p);
        let mut last: Option<Pose> = None;
        let (mut worst_move, mut worst_turn) = (0.0f64, 0.0f64);
        while !player.finished() {
            let pose = player.step(dt, &NoTargets);
            if let Some(l) = last {
                worst_move = worst_move.max(dist(l.position, pose.position));
                worst_turn = worst_turn.max(angle_deg(l.direction, pose.direction));
            }
            last = Some(pose);
        }
        // The fastest stretch (the crane's 26 m rise in 4 s, eased) is well under 0.5 m a frame,
        // and the look turns well under 3 degrees a frame.
        assert!(worst_move < 0.5, "a {worst_move} m step");
        assert!(worst_turn < 3.0, "a {worst_turn}° turn");
    }
);

dwell_world::both_faces!(
    /// An orbit keeps its radius and height around the point and looks straight at it.
    fn an_orbit_keeps_its_radius(f: FaceFrame) {
        let c = at(f, 5.0, 0.0, -3.0);
        let p = two(
            f,
            at(f, 17.0, 4.0, -3.0),
            at(f, 17.0, 4.0, -3.0),
            json!({ "move": "orbit", "around": c, "degrees": 360 }),
        );
        for k in 0..=80 {
            let pose = pose_at(&p, k as f64 / 10.0, &NoTargets);
            let l = f.vector(sub(pose.position, c));
            assert!(
                (l[0].hypot(l[2]) - 12.0).abs() < 1e-9,
                "radius at {k}: {l:?}"
            );
            assert!((l[1] - 4.0).abs() < 1e-9, "height at {k}");
            assert!(
                angle_deg(pose.direction, sub(c, pose.position)) < 1e-6,
                "looks in at {k}"
            );
        }
        // Half way round a full turn it is on the far side.
        let half = pose_at(&p, 4.0, &NoTargets);
        assert!(
            dist(half.position, at(f, -7.0, 4.0, -3.0)) < 1e-6,
            "{:?}",
            half.position
        );
    }
);

/// A target walking at 4 m/s across the face.
struct Walking {
    f: FaceFrame,
    t: f64,
}
impl Targets for Walking {
    fn position(&self, t: &TargetRef) -> Option<[f64; 3]> {
        match t {
            TargetRef::Player => Some(at(self.f, 4.0 * self.t, 1.6, 0.5 * self.t)),
            TargetRef::Point(p) => Some(*p),
            _ => None,
        }
    }
}

/// Plays a track of the walking player at `fps` for 10 s; the poses and the target each frame.
fn play_track(f: FaceFrame, fps: f64) -> Vec<(Pose, [f64; 3])> {
    let p = path(json!({ "formatVersion": 1,
        "keyframes": [{ "t": 0, "position": at(f, -6.0, 3.6, 0.0) }, { "t": 10 }],
        "segments": [{ "move": "track", "target": "player", "offset": [-6, 2, 0], "smoothingS": 0.5 }] }));
    let mut player = Player::new(p);
    let mut w = Walking { f, t: 0.0 };
    let mut out = Vec::new();
    let dt = 1.0 / fps;
    while !player.finished() {
        w.t += dt;
        let pose = player.step(dt, &w);
        out.push((pose, w.position(&TargetRef::Player).unwrap_or_default()));
    }
    out
}

dwell_world::both_faces!(
    /// Tracking a walking target keeps it centred and, once caught up, at the offset's distance
    /// (behind by the smoothing's lag: 4 m/s × 0.5 s).
    fn a_track_keeps_the_target_centred(f: FaceFrame) {
        let frames = play_track(f, 60.0);
        for (k, (pose, target)) in frames.iter().enumerate() {
            let off = angle_deg(pose.direction, sub(*target, pose.position));
            assert!(off < 0.5, "frame {k}: the target {off}° off centre");
        }
        let (pose, target) = frames[frames.len() - 1];
        let d = dist(pose.position, target);
        let nominal = (36.0f64 + 4.0).sqrt();
        assert!(
            (d - nominal).abs() < 2.5,
            "settled {d} m away (nominal {nominal})"
        );
    }
);

dwell_world::both_faces!(
    /// The same track at 30 and 144 frames a second sits at the same place behind the target
    /// (within a centimetre) at every whole second.
    fn tracking_is_frame_rate_independent(f: FaceFrame) {
        let slow = play_track(f, 30.0);
        let fast = play_track(f, 144.0);
        for s in 1..10 {
            let (a, ta) = slow[s * 30 - 1];
            let (b, tb) = fast[s * 144 - 1];
            let (oa, ob) = (sub(a.position, ta), sub(b.position, tb));
            assert!(dist(oa, ob) < 0.01, "{s} s: {oa:?} vs {ob:?}");
        }
    }
);

dwell_world::both_faces!(
    /// A speed ramp (1× to 4×) plays a 20 s path in 8 s at any frame rate; the time of day runs
    /// forward through midnight.
    fn speed_and_time_of_day(f: FaceFrame) {
        let p = path(json!({ "formatVersion": 1,
            "keyframes": [
                { "t": 0, "position": at(f, 0.0, 0.0, 0.0), "speed": 1, "timeOfDay": 0.8 },
                { "t": 20, "position": at(f, 10.0, 0.0, 0.0), "speed": 4, "timeOfDay": 0.2 }
            ] }));
        // dτ/dt = 1 + 3τ/20 → τ(t) = (20/3)(e^{3t/20} − 1); τ = 20 at t = (20/3) ln 4 ≈ 9.24 s.
        for fps in [30.0, 144.0] {
            let mut player = Player::new(p.clone());
            let mut real = 0.0;
            while !player.finished() {
                player.step(1.0 / fps, &NoTargets);
                real += 1.0 / fps;
            }
            assert!((real - 9.242).abs() < 0.05, "{fps} fps: {real} s");
        }
        let mid = pose_at(&p, 10.0, &NoTargets).time_of_day.unwrap_or(-1.0);
        assert!(
            (mid - 0.0).abs() < 1e-9 || (mid - 1.0).abs() < 1e-9,
            "midnight half way: {mid}"
        );
    }
);

dwell_world::both_faces!(
    /// A vertigo dolly holds the subject's size on screen: tan(fov / 2) × distance is constant.
    fn a_vertigo_dolly_holds_the_subject(f: FaceFrame) {
        let c = at(f, 0.0, 0.0, 0.0);
        let p = two(
            f,
            at(f, 0.0, 0.0, 30.0),
            at(f, 0.0, 0.0, 8.0),
            json!({ "move": "dolly", "at": c, "vertigo": true }),
        );
        let size = |pose: Pose| {
            (pose.fov_deg.unwrap_or(0.0).to_radians() / 2.0).tan() * dist(pose.position, c)
        };
        let s0 = size(pose_at(&p, 0.0, &NoTargets));
        for k in 1..=8 {
            let s = size(pose_at(&p, f64::from(k), &NoTargets));
            assert!((s - s0).abs() < 1e-6, "{k}: {s} vs {s0}");
        }
    }
);

dwell_world::both_faces!(
    /// Face B's pose of a face-local path is face A's mirrored (the same yaw and face-local pitch).
    fn face_b_mirrors_face_a(f: FaceFrame) {
        let make = |g: FaceFrame| {
            two(
                g,
                at(g, 10.0, 3.0, 0.0),
                at(g, 0.0, 6.0, 14.0),
                json!({ "move": "orbit", "around": at(g, 0.0, 0.0, 0.0) }),
            )
        };
        let (pa, pf) = (make(FaceFrame::A), make(f));
        for k in 0..=8 {
            let (x, y) = (
                pose_at(&pa, f64::from(k), &NoTargets),
                pose_at(&pf, f64::from(k), &NoTargets),
            );
            assert!(dist(f.point(x.position), y.position) < 1e-9);
            assert!(
                (x.yaw_deg - y.yaw_deg).abs() < 1e-9 && (x.pitch_deg - y.pitch_deg).abs() < 1e-9
            );
        }
    }
);

/// A straight path from face A over the midplane onto face B keeps its world direction: the look
/// doesn't flip as the eye crosses.
#[test]
fn a_path_across_the_midplane_keeps_its_world_look() {
    // both-faces: the path crosses from face A to face B itself.
    let mid = dwell_world::MIDPLANE_M;
    let p = path(json!({ "formatVersion": 1,
        "keyframes": [
            { "t": 0, "position": [0, mid + 20.0, 0], "look": [90, -45] },
            { "t": 4, "position": [20, mid - 20.0, 0] }
        ],
        "segments": [{ "move": "linear" }] }));
    let first = pose_at(&p, 0.0, &NoTargets).direction;
    for k in 0..=40 {
        let pose = pose_at(&p, f64::from(k) / 10.0, &NoTargets);
        assert!(
            angle_deg(pose.direction, first) < 1e-6,
            "{k}: {:?}",
            pose.direction
        );
    }
    let end = pose_at(&p, 4.0, &NoTargets);
    assert!(FaceFrame::at_height(end.position[1]).is_b());
}

/// The game's presets read, and each expands around a subject into a valid path on both faces.
#[test]
fn the_games_presets_expand_on_both_faces() {
    let presets = dwell_theater::presets();
    let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["orbit", "dolly", "track", "rise", "pan"]);
    for f in FaceFrame::under_test() {
        let s = at(f, 0.0, 0.0, 0.0);
        let ctx = Context {
            subject: Some((TargetRef::Player, s)),
            camera: Pose::new(at(f, 0.0, 2.0, -10.0), f.vector([0.0, 0.0, 1.0])),
        };
        for p in &presets {
            let path = p.expand(&ctx, &[]).unwrap_or_else(|e| panic!("{e}"));
            let start = pose_at(&path, path.keyframes[0].t, &Standing { at: s });
            assert!(start.position.iter().all(|c| c.is_finite()), "{}", p.name);
        }
    }
}

dwell_world::both_faces!(
    /// A preset added by data alone (a fixture no code names) expands with its defaults and its
    /// overrides clamped to their ranges.
    fn a_fixture_preset_by_data_alone(f: FaceFrame) {
        let presets =
            parse_presets(include_str!("fixtures/presets.json")).unwrap_or_else(|e| panic!("{e}"));
        let ring = &presets[0];
        assert_eq!(ring.name, "slow_half_ring");
        let s = at(f, 0.0, 0.0, 0.0);
        let ctx = Context {
            subject: Some((TargetRef::Point(s), s)),
            camera: Pose::new(at(f, 0.0, 0.0, -50.0), [0.0, 0.0, 1.0]),
        };
        let path = ring.expand(&ctx, &[]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(path.duration(), 40.0);
        for k in 0..=40 {
            let pose = pose_at(&path, f64::from(k), &NoTargets);
            let l = f.vector(sub(pose.position, s));
            assert!((l[0].hypot(l[2]) - 30.0).abs() < 1e-9 && (l[1] - 9.0).abs() < 1e-9);
        }
        // Half way round from the camera's side (−z): on the far side, +z.
        let end = pose_at(&path, 40.0, &NoTargets);
        assert!(
            dist(end.position, at(f, 0.0, 9.0, 30.0)) < 1e-6,
            "{:?}",
            end.position
        );
        // An override past the range is clamped; one the move doesn't know fails.
        let wide = ring
            .expand(&ctx, &[("radiusM".into(), 500.0)])
            .unwrap_or_else(|e| panic!("{e}"));
        let l = f.vector(sub(wide.keyframes[0].position, s));
        assert!((l[0].hypot(l[2]) - 60.0).abs() < 1e-9);
        let err = ring
            .expand(&ctx, &[("pitch".into(), 1.0)])
            .err()
            .unwrap_or_default();
        assert!(err.contains("no parameter pitch"), "{err}");
    }
);

/// Bad paths and presets fail with a message naming the entry.
#[test]
fn bad_data_fails_with_a_message() {
    let base =
        |kfs: Value, segs: Value| json!({ "formatVersion": 1, "keyframes": kfs, "segments": segs });
    let kf2 = json!([{ "t": 0, "position": [0, 100, 0] }, { "t": 2, "position": [5, 100, 0] }]);
    for (v, want) in [
        (
            base(kf2.clone(), json!([{ "move": "spin" }])),
            "segments[0].move: unknown move spin",
        ),
        (
            base(kf2.clone(), json!([{ "move": "pan" }])),
            "segments[0]: a pan turns in place",
        ),
        (
            base(kf2.clone(), json!([{ "move": "ease", "easing": "bouncy" }])),
            "segments[0].easing must be one of",
        ),
        (
            base(
                kf2.clone(),
                json!([{ "move": "linear", "around": [0, 0, 0] }]),
            ),
            "a linear move has no field around",
        ),
        (
            base(kf2.clone(), json!([{ "move": "orbit" }])),
            "segments[0].around is required",
        ),
        (
            base(
                kf2.clone(),
                json!([{ "move": "dolly", "vertigo": true, "at": [0, 0, 0] }]),
            ),
            "needs keyframes[0].fov",
        ),
        (
            base(
                json!([{ "t": 0, "position": [0, 0, 0] }, { "t": 0 }]),
                json!([]),
            ),
            "keyframes[1].t must be after",
        ),
        (
            base(json!([{ "t": 0 }, { "t": 1 }]), json!([])),
            "keyframes[0].position is required",
        ),
        (
            base(
                json!([{ "t": 0, "position": [0, 0, 0], "zoom": 2 }, { "t": 1 }]),
                json!([]),
            ),
            "keyframes[0]: unknown field zoom",
        ),
        (
            base(
                json!([{ "t": 0, "position": [0, 0, 0], "timeOfDay": 2 }, { "t": 1 }]),
                json!([]),
            ),
            "keyframes[0].timeOfDay",
        ),
    ] {
        let err = Path::parse(&v).err().unwrap_or_default();
        assert!(err.contains(want), "{v}: {err:?} lacks {want:?}");
    }
    let err = parse_presets(r#"{"formatVersion":1,"presets":[{"name":"x","move":"orbit","params":{"seconds":{"default":5,"min":1,"max":9},"zoomM":{"min":0,"max":1}}}]}"#)
        .err()
        .unwrap_or_default();
    assert!(
        err.contains("preset x.params: a orbit preset has no parameter zoomM"),
        "{err}"
    );
}

/// A path written out reads back the same (export / import).
#[test]
fn a_path_round_trips_through_json() {
    let p = path(json!({ "formatVersion": 1, "name": "tour", "loop": true,
        "keyframes": [
            { "t": 0, "position": [1, 100, 2], "look": [10, -5], "fov": 50, "timeOfDay": 0.25 },
            { "t": 3, "position": [4, 105, 6], "target": { "creature": 7 }, "speed": 2 },
            { "t": 6, "position": [9, 120, 6], "target": "player" },
            { "t": 9 }
        ],
        "segments": [
            { "move": "dolly", "at": [0, 100, 0], "vertigo": true, "days": 1 },
            { "move": "track", "target": { "npc": 3 }, "offset": [1, 2, 3], "smoothingS": 0.2 },
            { "move": "crane", "upEasing": "inQuad", "easing": "outSine" }
        ] }));
    let again = Path::parse(&p.to_json()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p, again);
    assert_eq!(segment_at(&p, 4.0), (1, 1.0 / 3.0));
}
