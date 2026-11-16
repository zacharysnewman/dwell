//! The horizon cull (the owner's 60 fps pass, 2026-11-15: "Do we skip occluded chunks?"; measured
//! by `dwell-mesh` `tests/occlusion_probe.rs`: the terrain alone hides 25–43 % of the near chunks'
//! triangles from a standing eye). On the CPU, each frame, face-locally: every chunk column's
//! ground is an occluder up to the height it is solid through (a chunk's `floor` over ground below
//! it that is buried or solid through), and a chunk is hidden when, in every direction it spans,
//! nearer occluders stand above the line from the eye to its highest point.
//!
//! Conservative: an occluder's height is seen from its farthest corner (the lowest angle it can
//! hold up), the cull only spans the azimuth bins wholly inside an occluder and every bin a chunk
//! touches, and a chunk the eye is within (or almost) is never culled.

use std::f64::consts::TAU;

/// Azimuth bins round the eye.
pub const BINS: usize = 512;
/// A chunk nearer than this (m, across the ground) is never culled.
const NEAR_M: f64 = 4.0;

/// A footprint's (x0, z0, x1, z1: face-local, across the ground) nearest and farthest distance
/// from the eye at `eye` (x, z), or None when the eye is within it.
fn distances(eye: [f64; 2], fp: [f64; 4]) -> Option<(f64, f64)> {
    let (dx, dz) = (
        (fp[0] - eye[0]).max(eye[0] - fp[2]).max(0.0),
        (fp[1] - eye[1]).max(eye[1] - fp[3]).max(0.0),
    );
    let near = (dx * dx + dz * dz).sqrt();
    if near <= 0.0 {
        return None;
    }
    let far = [
        [fp[0], fp[1]],
        [fp[2], fp[1]],
        [fp[2], fp[3]],
        [fp[0], fp[3]],
    ]
    .iter()
    .map(|c| ((c[0] - eye[0]).powi(2) + (c[1] - eye[1]).powi(2)).sqrt())
    .fold(0.0, f64::max);
    Some((near, far))
}

/// An occluder: a column of ground solid through up to `height` (face-local m) over footprint
/// `fp`.
#[derive(Clone, Copy, Debug)]
pub struct Occluder {
    pub fp: [f64; 4],
    pub height: f64,
}

/// A chunk to test: its footprint and its highest point (face-local m).
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub fp: [f64; 4],
    pub top: f64,
}

fn bin(a: f64) -> f64 {
    a / TAU * BINS as f64
}

/// Which of `candidates` the `occluders` hide from an eye at `eye` (face-local).
pub fn hidden(eye: [f64; 3], occluders: &[Occluder], candidates: &[Candidate]) -> Vec<bool> {
    let flat = [eye[0], eye[2]];
    // The occluders by their farthest distance; the candidates by their nearest.
    let mut occ: Vec<(f64, f64, f64, f64)> = occluders
        .iter()
        .filter_map(|o| {
            let (_, far, lo, hi) = span_of(flat, o.fp)?;
            Some((far, (o.height - eye[1]) / far, lo, hi))
        })
        .collect();
    occ.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    let near_of = |c: &Candidate| span_of(flat, c.fp).map_or(0.0, |s| s.0);
    order.sort_by(|&a, &b| near_of(&candidates[a]).total_cmp(&near_of(&candidates[b])));
    let mut horizon = vec![f64::NEG_INFINITY; BINS];
    let mut out = vec![false; candidates.len()];
    let mut next = 0;
    for i in order {
        let c = &candidates[i];
        let Some((near, _, lo, hi)) = span_of(flat, c.fp) else {
            continue;
        };
        // Every occluder wholly nearer than it rises into the horizon first.
        while next < occ.len() && occ[next].0 <= near {
            let (_, slope, olo, ohi) = occ[next];
            // The bins wholly inside its span.
            let (a, b) = (bin(olo).ceil() as i64, bin(ohi).floor() as i64);
            for k in a..b {
                let h = &mut horizon[k.rem_euclid(BINS as i64) as usize];
                *h = h.max(slope);
            }
            next += 1;
        }
        if near < NEAR_M {
            continue;
        }
        let slope = (c.top - eye[1]) / near;
        // Every bin it touches.
        let (a, b) = (bin(lo).floor() as i64, bin(hi).ceil() as i64);
        out[i] = (a..b.max(a + 1)).all(|k| horizon[k.rem_euclid(BINS as i64) as usize] > slope);
    }
    out
}

/// A footprint's [`distances`] and its azimuth span about the eye: start and end angles (radians,
/// the end may pass τ; the eye is outside, so it spans under π).
fn span_of(eye: [f64; 2], fp: [f64; 4]) -> Option<(f64, f64, f64, f64)> {
    let (near, far) = distances(eye, fp)?;
    let corners = [
        [fp[0], fp[1]],
        [fp[2], fp[1]],
        [fp[2], fp[3]],
        [fp[0], fp[3]],
    ];
    let centre = [
        (fp[0] + fp[2]) / 2.0 - eye[0],
        (fp[1] + fp[3]) / 2.0 - eye[1],
    ];
    let mid = centre[1].atan2(centre[0]);
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    for c in corners {
        let mut d = (c[1] - eye[1]).atan2(c[0] - eye[0]) - mid;
        while d > std::f64::consts::PI {
            d -= TAU;
        }
        while d < -std::f64::consts::PI {
            d += TAU;
        }
        lo = lo.min(d);
        hi = hi.max(d);
    }
    let start = (mid + lo).rem_euclid(TAU);
    Some((near, far, start, start + (hi - lo)))
}

#[cfg(test)]
mod tests {
    // face-a-only: face-local arithmetic; the renderer crosses each face into it.
    use super::*;

    fn fp(x: f64, z: f64) -> [f64; 4] {
        [x, z, x + 32.0, z + 32.0]
    }

    #[test]
    fn a_chunk_behind_a_hill_is_hidden_and_one_above_it_is_not() {
        let eye = [0.0, 10.0, 16.0];
        // A ridge 40 m high at x 32..64, the valley floor behind it (x 96..128) at 5 m.
        let occ = [Occluder {
            fp: fp(32.0, 0.0),
            height: 40.0,
        }];
        let cands = [
            Candidate {
                fp: fp(96.0, 0.0),
                top: 12.0,
            },
            // A tall tree there reaching over the ridge's line.
            Candidate {
                fp: fp(96.0, 0.0),
                top: 150.0,
            },
            // Before the ridge: never hidden by it.
            Candidate {
                fp: fp(32.0, 0.0),
                top: 12.0,
            },
        ];
        assert_eq!(hidden(eye, &occ, &cands), vec![true, false, false]);
    }

    #[test]
    fn nothing_is_hidden_from_above_or_beside_the_occluder() {
        let occ = [Occluder {
            fp: fp(32.0, 0.0),
            height: 40.0,
        }];
        let behind = [Candidate {
            fp: fp(96.0, 0.0),
            top: 12.0,
        }];
        // Flying well over the ridge (from 80 m its line still passes over the valley's 12 m).
        assert_eq!(hidden([0.0, 80.0, 16.0], &occ, &behind), vec![true]);
        assert_eq!(hidden([0.0, 200.0, 16.0], &occ, &behind), vec![false]);
        // Off to the side: the ridge doesn't stand between.
        let beside = [Candidate {
            fp: fp(96.0, 96.0),
            top: 12.0,
        }];
        assert_eq!(hidden([0.0, 10.0, 16.0], &occ, &beside), vec![false]);
    }

    #[test]
    fn a_chunk_straddling_the_ridge_edge_is_kept() {
        // Partly behind the ridge, partly past its end: some bin it touches is open.
        let occ = [Occluder {
            fp: fp(32.0, 0.0),
            height: 40.0,
        }];
        // The ridge spans ±26.6° from the eye; this chunk 10.6°–30.3°.
        let cands = [Candidate {
            fp: [96.0, 40.0, 128.0, 72.0],
            top: 12.0,
        }];
        assert_eq!(hidden([0.0, 10.0, 16.0], &occ, &cands), vec![false]);
    }

    #[test]
    fn spans_wrap_round_behind_the_eye() {
        // Due −x of the eye: the span crosses ±π.
        let eye = [0.0, 10.0, 0.0];
        let occ = [Occluder {
            fp: [-64.0, -16.0, -32.0, 16.0],
            height: 40.0,
        }];
        let cands = [Candidate {
            fp: [-160.0, -8.0, -128.0, 8.0],
            top: 12.0,
        }];
        assert_eq!(hidden(eye, &occ, &cands), vec![true]);
    }
}
