//! The swap state machine as a CPU model of the renderer's dither: at every frame, for a fine
//! region and every threshold a pixel can have, exactly one drawn item covering the region keeps
//! the pixel.

use super::*;
use crate::grid::section_at;
use dwell_world::lod::child;

const FADE_MS: f64 = 400.0;
const FRAME_MS: f64 = 16.0;

fn kids(c: LodCoord) -> Vec<LodCoord> {
    (0..8).map(|o| child(c, o)).collect()
}

fn sections(cs: &[LodCoord]) -> Vec<Item> {
    cs.iter().map(|&c| Item::section(c, 0)).collect()
}

/// Whether `a`'s box holds `probe`'s.
fn holds(a: LodCoord, probe: LodCoord) -> bool {
    a.level >= probe.level && {
        let d = a.level - probe.level;
        (probe.i >> d, probe.j >> d, probe.k >> d) == (a.i, a.j, a.k)
    }
}

/// The thresholds a pixel can have: the 4 × 4 Bayer steps and some in between.
fn thresholds() -> Vec<f32> {
    (0..64).map(|k| (k as f32 + 0.5) / 64.0).collect()
}

/// How many shown items covering `probe` keep a pixel of threshold `n`.
fn keeping(f: &SwapFade, now: f64, probe: LodCoord, n: f32) -> usize {
    f.shown(now)
        .filter(|(it, fade)| holds(it.area, probe) && fade.keeps(n))
        .count()
}

fn assert_one_level(f: &SwapFade, now: f64, probes: &[LodCoord], what: &str) {
    for &p in probes {
        for n in thresholds() {
            let k = keeping(f, now, p, n);
            assert_eq!(
                k, 1,
                "{what} at {now} ms: {k} levels draw {p:?} at threshold {n}"
            );
        }
    }
}

/// Fine regions under `c` (one per grandchild).
fn probes(c: LodCoord) -> Vec<LodCoord> {
    kids(c).into_iter().flat_map(kids).collect()
}

/// A level-4 section by the origin (in the world, so its id is its own).
fn p() -> LodCoord {
    section_at(4, [10.0, 1.0, 10.0])
}

#[test]
fn a_refine_cross_fades_and_frees_the_parent_when_it_ends() {
    let mut f = SwapFade::new(FADE_MS);
    let mut now = 0.0;
    f.update(&sections(&[p()]), now);
    assert_eq!(f.shown(now).count(), 1);
    let k = sections(&kids(p()));
    let mut frames_fading = 0;
    loop {
        now += FRAME_MS;
        f.update(&k, now);
        assert_one_level(&f, now, &probes(p()), "refine");
        if f.retiring().next().is_none() {
            break;
        }
        frames_fading += 1;
        assert!(now < 2.0 * FADE_MS, "the fade never ended");
    }
    // About fade_ms of frames with both levels, then only the children: the parent is retired
    // and its mesh may go (nothing holds it).
    assert!((24..=26).contains(&frames_fading), "{frames_fading}");
    assert_eq!(f.shown(now).count(), 8);
    assert!(
        f.shown(now)
            .all(|(it, fade)| it.area.level == 3 && fade == Fade::Full)
    );
    assert_eq!(f.stats().swaps, 0);
}

#[test]
fn a_coarsen_cross_fades_too() {
    let mut f = SwapFade::new(FADE_MS);
    let mut now = 0.0;
    f.update(&sections(&kids(p())), now);
    let mut seen_both = false;
    for _ in 0..40 {
        now += FRAME_MS;
        f.update(&sections(&[p()]), now);
        assert_one_level(&f, now, &probes(p()), "coarsen");
        seen_both |= f.shown(now).count() == 9;
    }
    assert!(seen_both);
    assert_eq!(f.shown(now).count(), 1);
    assert!(f.retiring().next().is_none());
}

#[test]
fn chunks_and_stand_ins_swap_like_sections() {
    // A level-1 section to its chunks, and a stand-in to the section it stood in for.
    let s1 = section_at(1, [10.0, 1.0, 10.0]);
    let hole = section_at(2, [300.0, 1.0, 300.0]);
    let mut f = SwapFade::new(FADE_MS);
    let mut now = 0.0;
    f.update(&[Item::section(s1, 0), Item::stand_in(hole, 77)], now);
    for _ in 0..30 {
        now += FRAME_MS;
        f.update(&[Item::chunks(s1), Item::section(hole, 0)], now);
        assert_one_level(&f, now, &[s1], "chunks");
        assert_one_level(&f, now, &probes(hole), "stand-in");
    }
    let kinds: Vec<ItemKind> = f.shown(now).map(|(it, _)| it.kind).collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.contains(&ItemKind::Chunks) && kinds.contains(&ItemKind::Section));
}

#[test]
fn a_fade_time_of_zero_swaps_in_one_frame_as_before() {
    let mut f = SwapFade::new(0.0);
    f.update(&sections(&[p()]), 0.0);
    f.update(&sections(&kids(p())), 16.0);
    assert_eq!(f.shown(16.0).count(), 8);
    assert!(f.shown(16.0).all(|(_, fade)| fade == Fade::Full));
    assert!(f.retiring().next().is_none());
    assert_one_level(&f, 16.0, &probes(p()), "no fade");
}

#[test]
fn a_swap_inside_one_still_fading_joins_it_and_never_leaves_a_hole() {
    // Coarse to fine a level a frame (the view's first seconds), and a grandchild refining
    // halfway through its parent's fade.
    let mut f = SwapFade::new(FADE_MS);
    let mut now = 0.0;
    f.update(&sections(&[p()]), now);
    let k = kids(p());
    now += FRAME_MS;
    f.update(&sections(&k), now);
    for _ in 0..10 {
        now += FRAME_MS;
        f.update(&sections(&k), now);
        assert_one_level(&f, now, &probes(p()), "first swap");
    }
    let mut next: Vec<LodCoord> = k[1..].to_vec();
    next.extend(kids(k[0]));
    for _ in 0..40 {
        now += FRAME_MS;
        f.update(&sections(&next), now);
        assert_one_level(&f, now, &probes(p()), "nested swap");
    }
    assert!(f.stats().merged >= 1);
    assert_eq!(f.shown(now).count(), 15);
}

#[test]
fn a_swap_undone_halfway_runs_back_from_where_it_was() {
    let mut f = SwapFade::new(FADE_MS);
    let mut now = 0.0;
    f.update(&sections(&[p()]), now);
    for _ in 0..10 {
        now += FRAME_MS;
        f.update(&sections(&kids(p())), now);
    }
    // The parent's share of pixels just before and just after it comes back.
    let share = |f: &SwapFade, now: f64| {
        let ts = thresholds();
        ts.iter()
            .filter(|&&n| {
                f.shown(now)
                    .any(|(it, fade)| it.area == p() && fade.keeps(n))
            })
            .count()
    };
    let before = share(&f, now);
    f.update(&sections(&[p()]), now);
    let after = share(&f, now);
    assert!(before.abs_diff(after) <= 1, "{before} → {after}");
    assert_eq!(f.stats().reversed, 1);
    for _ in 0..40 {
        now += FRAME_MS;
        f.update(&sections(&[p()]), now);
        assert_one_level(&f, now, &probes(p()), "reversed");
    }
    assert_eq!(f.shown(now).count(), 1);
}

#[test]
fn a_finer_level_with_nothing_there_fades_the_coarse_one_out_and_a_first_view_shows_at_once() {
    let mut f = SwapFade::new(FADE_MS);
    // Nothing drew before: the first selection shows in full.
    f.update(&sections(&[p()]), 0.0);
    assert!(f.shown(0.0).all(|(_, fade)| fade == Fade::Full));
    // The finer level is empty here: the parent fades out to nothing, then goes.
    f.update(&[], 16.0);
    assert!(matches!(f.shown(16.0).next(), Some((_, Fade::Out(_)))));
    f.update(&[], 16.0 + FADE_MS);
    assert_eq!(f.shown(16.0 + FADE_MS).count(), 0);
}

#[test]
fn the_shader_value_encodes_full_in_and_out_apart() {
    assert_eq!(Fade::Full.shader_value(), 0.0);
    assert_eq!(Fade::In(0.0).shader_value(), 1.0);
    assert_eq!(Fade::In(0.25).shader_value(), 1.25);
    assert_eq!(Fade::Out(0.0).shader_value(), -1.0);
    assert_eq!(Fade::Out(1.0).shader_value(), -2.0);
    for t in [0.0, 0.3, 1.0] {
        for n in thresholds() {
            assert_ne!(Fade::In(t).keeps(n), Fade::Out(t).keeps(n));
        }
    }
}

#[test]
fn many_concurrent_swaps_cost_little_per_frame() {
    // 4,096 level-3 sections refining at once into 32,768 children (more than any real frame
    // starts: a whole ring of the view swapping), then the frames while they fade.
    let o = section_at(3, [0.0, 1.0, 0.0]);
    let parents: Vec<LodCoord> = (0..64)
        .flat_map(|i| (0..64).map(move |k| LodCoord::new(3, o.i + i, o.j, o.k + k)))
        .collect();
    let children: Vec<LodCoord> = parents.iter().flat_map(|&p| kids(p)).collect();
    let mut f = SwapFade::new(FADE_MS);
    f.update(&sections(&parents), 0.0);
    let k = sections(&children);
    let t0 = std::time::Instant::now();
    f.update(&k, 16.0);
    let start = t0.elapsed();
    assert_eq!(f.stats().swaps, parents.len());
    let t1 = std::time::Instant::now();
    for frame in 2..10 {
        f.update(&k, f64::from(frame) * 16.0);
    }
    let fading = t1.elapsed() / 8;
    assert_eq!(f.stats().retiring, parents.len());
    eprintln!(
        "{} swaps: start {start:?}, a frame while fading {fading:?}",
        parents.len()
    );
    // Generous bounds (debug build, shared machine): the point is no blow-up with the swap count.
    assert!(start.as_millis() < 500, "{start:?}");
    assert!(fading.as_millis() < 100, "{fading:?}");
}
