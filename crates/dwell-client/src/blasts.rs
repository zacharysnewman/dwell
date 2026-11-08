//! Blasts on the client (docs/EXPLOSIVES.md §4): the effects of an explosive going off, started
//! when the server's `Explosion` modification turns a lit explosive's cell to air (nothing else on
//! the wire). The flash (a short point light and a fireball), smoke rising and spreading, dust
//! thrown out and falling, and the camera's shake, each as the explosive's data says
//! (`dwell_world::explosives`), each switchable (Settings → Explosions); a CPU adapter draws its
//! `cpuCount` puffs. Draw-only: nothing here feeds the simulation or the aim.
//!
//! Face-local: smoke rises and dust falls along the face's up at the blast (`FaceFrame`).

use dwell_look::lights::PointLight;
use dwell_world::FaceFrame;
use dwell_world::explosives::{EXPLOSIVES, ExplosiveData, LIMITS, PuffLook, shake_level};

/// A blast going off: where (world m), its explosive (index in `EXPLOSIVES`), when (s), its seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub at: [f64; 3],
    pub explosive: usize,
    pub start_s: f64,
    pub seed: u32,
    /// Its way out (world, `dwell_world::explosives::open_dir`, at most 1 long; about 0 buried),
    /// read from the client's cells round it before they cleared: the puffs follow it.
    pub open: [f64; 3],
    /// The share (0–1) of its smoke and dust it draws: what the puffs budget
    /// (`limits.maxPuffs`) had left when it went off, at least `limits.minPuffShare`. Fixed at
    /// its start, so no puff pops when another blast goes off.
    pub share: f64,
}

/// What a puff is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PuffKind {
    Smoke,
    Dust,
    /// The flash's fireball (self-lit).
    Fire,
    /// A wood chip off an axe's swing (crate::chips; `explosive` is its wood's index in `WOODS`).
    Chip,
    /// A gouge on a cut wood cell's side (crate::chips): not a billboard but a quad spanning `u`
    /// and `v` (half extents, world) about `at`; `size` is its depth (0–1 across the cell).
    Gouge {
        u: [f64; 3],
        v: [f64; 3],
        kerf: bool,
    },
}

/// One billboard to draw: its centre (world m), half size (m), opacity, kind and its explosive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puff {
    pub at: [f64; 3],
    pub size: f64,
    pub alpha: f64,
    pub kind: PuffKind,
    pub explosive: usize,
    pub seed: u32,
}

/// Which effects show (Settings → Explosions), and whether the adapter is a CPU's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlastSettings {
    pub flash: bool,
    pub smoke: bool,
    pub shake: bool,
    pub cpu: bool,
}

impl Default for BlastSettings {
    fn default() -> Self {
        Self {
            flash: true,
            smoke: true,
            shake: true,
            cpu: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Blasts {
    pub list: Vec<Blast>,
    pub settings: BlastSettings,
}

fn hash(seed: u32, k: u32) -> f64 {
    let mut h = seed ^ k.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / f64::from(u32::MAX)
}

/// The way out of a blast of `e` at world cell `cell` (`dwell_world::explosives::open_dir`, as the
/// server's), from the client's cells before the blast clears them (`solid` at a world cell).
pub fn open_way(
    e: &ExplosiveData,
    cell: [i32; 3],
    mut solid: impl FnMut([i32; 3]) -> bool,
) -> [f64; 3] {
    dwell_world::explosives::open_dir(e.open_sense_m, |o| {
        solid([cell[0] + o[0], cell[1] + o[1], cell[2] + o[2]])
    })
}

/// A puff's way out `dir` (face-local unit) turned toward the blast's way out `open` (face-local,
/// at most 1 long) by `bias` × its length: unchanged for a buried blast (`open` about 0).
pub fn toward_open(dir: [f64; 3], open: [f64; 3], bias: f64) -> [f64; 3] {
    let len = (open[0] * open[0] + open[1] * open[1] + open[2] * open[2]).sqrt();
    if len < 1e-6 || bias <= 0.0 {
        return dir;
    }
    let w = (bias * len.min(1.0)).clamp(0.0, 1.0);
    let v: [f64; 3] = std::array::from_fn(|k| (1.0 - w) * dir[k] + w * open[k] / len);
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n < 1e-6 { dir } else { v.map(|c| c / n) }
}

fn lerp([a, b]: [f64; 2], t: f64) -> f64 {
    a + (b - a) * t
}

/// How long blast `e`'s effects last (s): its longest.
fn lasts(e: &ExplosiveData) -> f64 {
    let fx = e.effects;
    fx.smoke.seconds[1]
        .max(fx.dust.seconds[1])
        .max(fx.flash.seconds)
        .max(fx.shake.seconds)
}

impl Blasts {
    /// A blast of explosive `explosive` at `at` (its cell's centre), now (`now_s`), its way out
    /// `open` (world; [`open_way`]).
    pub fn start(&mut self, at: [f64; 3], explosive: usize, now_s: f64, open: [f64; 3]) {
        let seed = (at[0] as i32 as u32).wrapping_mul(73_856_093)
            ^ (at[1] as i32 as u32).wrapping_mul(19_349_663)
            ^ (at[2] as i32 as u32).wrapping_mul(83_492_791)
            ^ (now_s * 1000.0) as u32;
        let share = EXPLOSIVES.get(explosive).map_or(1.0, |e| {
            let n = self.puff_count(e, 1.0);
            let left = f64::from(LIMITS.max_puffs) - self.puffs_in_flight(now_s) as f64;
            if n == 0 {
                1.0
            } else {
                (left / n as f64).clamp(LIMITS.min_puff_share, 1.0)
            }
        });
        self.list.push(Blast {
            at,
            explosive,
            start_s: now_s,
            seed,
            open,
            share,
        });
    }

    /// Blast `e`'s smoke and dust puffs at `share` (the CPU's counts on a CPU adapter).
    fn puff_count(&self, e: &ExplosiveData, share: f64) -> usize {
        let fx = e.effects;
        let (s, d) = if self.settings.cpu {
            (fx.smoke.cpu_count, fx.dust.cpu_count)
        } else {
            (fx.smoke.count, fx.dust.count)
        };
        shared(s as usize, share) + shared(d as usize, share)
    }

    /// The smoke and dust puffs the blasts in flight draw at most, at `now_s` (each blast's whole
    /// share until its longest puff is gone).
    fn puffs_in_flight(&self, now_s: f64) -> usize {
        self.list
            .iter()
            .filter_map(|b| {
                let e = EXPLOSIVES.get(b.explosive)?;
                let fx = e.effects;
                let t = now_s - b.start_s;
                (t < fx.smoke.seconds[1].max(fx.dust.seconds[1]))
                    .then(|| self.puff_count(e, b.share))
            })
            .sum()
    }

    /// Forgets the blasts whose effects are over.
    pub fn prune(&mut self, now_s: f64) {
        self.list.retain(|b| {
            EXPLOSIVES
                .get(b.explosive)
                .is_some_and(|e| now_s - b.start_s <= lasts(e))
        });
    }

    /// The camera's shake now (yaw, pitch degrees) at `eye`: each blast's, strongest at it and
    /// dying out over its seconds. None with the setting off.
    pub fn shake(&self, eye: [f64; 3], now_s: f64) -> (f64, f64) {
        if !self.settings.shake {
            return (0.0, 0.0);
        }
        let (mut yaw, mut pitch) = (0.0, 0.0);
        for b in &self.list {
            let Some(e) = EXPLOSIVES.get(b.explosive) else {
                continue;
            };
            let s = e.effects.shake;
            let t = now_s - b.start_s;
            if !(0.0..s.seconds).contains(&t) {
                continue;
            }
            let d = dist(eye, b.at);
            let amp = s.degrees * shake_level(e, d) * (1.0 - t / s.seconds).powi(2);
            let w = std::f64::consts::TAU * s.hz * t;
            let p = hash(b.seed, 1) * std::f64::consts::TAU;
            yaw += amp * (w + p).sin();
            pitch += amp * 0.7 * (1.3 * w + 2.0 * p).sin();
        }
        (yaw, pitch)
    }

    /// The flashes' point lights now (with the Flash setting).
    pub fn lights(&self, now_s: f64) -> Vec<PointLight> {
        if !self.settings.flash {
            return Vec::new();
        }
        self.list
            .iter()
            .filter_map(|b| {
                let e = EXPLOSIVES.get(b.explosive)?;
                let f = e.effects.flash;
                let t = now_s - b.start_s;
                if !(0.0..f.seconds).contains(&t) {
                    return None;
                }
                let k = (1.0 - t / f.seconds).powi(2);
                Some(PointLight {
                    pos: b.at,
                    color: f.color.map(|c| dwell_look::srgb_to_linear(f64::from(c))),
                    strength: f.strength * k,
                    radius_m: f.light_radius_m,
                    flicker: 0.0,
                    flicker_speed: 0.0,
                    seed: b.seed,
                    shadows: false,
                    placed: false,
                    slot: None,
                })
            })
            .collect()
    }

    /// The puffs to draw now: the fireball (Flash), the smoke and the dust (Smoke and dust).
    pub fn puffs(&self, now_s: f64) -> Vec<Puff> {
        let mut out = Vec::new();
        for b in &self.list {
            let Some(e) = EXPLOSIVES.get(b.explosive) else {
                continue;
            };
            let t = now_s - b.start_s;
            if t < 0.0 {
                continue;
            }
            let face = FaceFrame::at_height(b.at[1]);
            let local = face.point(b.at);
            let fx = e.effects;
            if self.settings.flash && t < fx.flash.seconds {
                let grow = (t / 0.12).min(1.0);
                out.push(Puff {
                    at: b.at,
                    size: 0.5 * fx.flash.size_m * (0.35 + 0.65 * grow),
                    alpha: (1.0 - t / fx.flash.seconds).powf(1.5),
                    kind: PuffKind::Fire,
                    explosive: b.explosive,
                    seed: b.seed,
                });
            }
            if self.settings.smoke {
                puffs_of(
                    &mut out,
                    b,
                    face,
                    local,
                    &fx.smoke,
                    PuffKind::Smoke,
                    t,
                    self.settings.cpu,
                );
                puffs_of(
                    &mut out,
                    b,
                    face,
                    local,
                    &fx.dust,
                    PuffKind::Dust,
                    t,
                    self.settings.cpu,
                );
            }
        }
        out
    }
}

/// `n` puffs at `share`: rounded up, so a blast draws at least one of each kind it has.
fn shared(n: usize, share: f64) -> usize {
    ((n as f64 * share).ceil() as usize).min(n)
}

#[allow(clippy::too_many_arguments)]
fn puffs_of(
    out: &mut Vec<Puff>,
    b: &Blast,
    face: FaceFrame,
    local: [f64; 3],
    look: &PuffLook,
    kind: PuffKind,
    t: f64,
    cpu: bool,
) {
    // The first `share` of them: each puff's way out is its own hash, so any first few are an
    // even sample of the blast's.
    let n = shared(
        (if cpu { look.cpu_count } else { look.count }) as usize,
        b.share,
    ) as u32;
    let salt = if kind == PuffKind::Smoke { 100 } else { 10_000 };
    for i in 0..n {
        let k = salt + 8 * i;
        let life = lerp(look.seconds, hash(b.seed, k));
        if t >= life {
            continue;
        }
        let u = t / life;
        // A way out over the face-local hemisphere, up-leaning.
        let a = hash(b.seed, k + 1) * std::f64::consts::TAU;
        let up = 0.2 + 0.8 * hash(b.seed, k + 2);
        let side = (1.0 - up * up).sqrt();
        let dir = toward_open(
            [a.cos() * side, up, a.sin() * side],
            face.vector(b.open),
            look.open_bias,
        );
        let p = if kind == PuffKind::Smoke {
            // Spreads out fast then slows, and rises.
            let spread =
                look.spread_m * (0.4 + 0.6 * hash(b.seed, k + 3)) * (1.0 - (-3.0 * t).exp());
            [
                local[0] + dir[0] * spread,
                local[1] + dir[1] * spread * 0.5 + look.rise_ms * t,
                local[2] + dir[2] * spread,
            ]
        } else {
            // Thrown out and falling.
            let v = lerp(look.speed_ms, hash(b.seed, k + 3));
            [
                local[0] + dir[0] * v * t,
                local[1] + dir[1] * v * t - 4.9 * t * t,
                local[2] + dir[2] * v * t,
            ]
        };
        let size = 0.5 * lerp(look.size_m, hash(b.seed, k + 4)) * (0.5 + 0.5 * u.sqrt());
        let fade_in = (t / 0.12).min(1.0);
        let alpha = look.opacity * fade_in * (1.0 - u * u);
        out.push(Puff {
            at: face.point(p),
            size,
            alpha,
            kind,
            explosive: b.explosive,
            seed: b.seed.wrapping_add(k),
        });
    }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dynamite() -> usize {
        EXPLOSIVES
            .iter()
            .position(|e| e.name == "dwell:dynamite")
            .expect("dynamite")
    }

    dwell_world::both_faces!(
        fn smoke_rises_and_dust_falls_along_the_faces_up(f: FaceFrame) {
            let mut b = Blasts::default();
            let at = f.point([0.5, 40.25, 0.5]);
            b.start(at, dynamite(), 0.0, [0.0; 3]);
            let rise = |p: &Puff| f.point(p.at)[1] - 40.25;
            let early = b.puffs(0.3);
            let late = b.puffs(1.5);
            let smoke = |v: &[Puff]| {
                let s: Vec<f64> = v
                    .iter()
                    .filter(|p| p.kind == PuffKind::Smoke)
                    .map(rise)
                    .collect();
                s.iter().sum::<f64>() / s.len() as f64
            };
            assert!(
                smoke(&late) > smoke(&early) && smoke(&late) > 0.5,
                "smoke rises"
            );
            assert!(
                b.puffs(0.1).iter().any(|p| p.kind == PuffKind::Fire),
                "the fireball"
            );
            assert!(
                late.iter().all(|p| p.kind != PuffKind::Fire),
                "gone by then"
            );
            let dust = |v: &[Puff]| {
                v.iter()
                    .filter(|p| p.kind == PuffKind::Dust)
                    .map(rise)
                    .fold(f64::INFINITY, f64::min)
            };
            assert!(dust(&b.puffs(1.4)) < dust(&b.puffs(0.2)), "dust falls back");
        }
    );

    /// A named face-local fixture (which cells are rock) and the way its smoke should go.
    type Case = (&'static str, fn([i32; 3]) -> bool, [f64; 3]);

    dwell_world::both_faces!(
        fn the_smoke_follows_the_blasts_open_side_and_a_buried_one_spreads_evenly(f: FaceFrame) {
            // Owner (2026-11-07): "Make the smoke particles follow explosion direction."
            let e = &EXPLOSIVES[dynamite()];
            let centre = [0, 80, 0];
            // A cliff face open from face-local x = 1 on, a cave roof's underside (open below),
            // and buried: the client's cells round the charge, face-local, through the frame.
            let cases: [Case; 2] = [
                ("a cliff face", |o| o[0] < 1, [1.0, 0.0, 0.0]),
                ("a cave roof", |o| o[1] > -1, [0.0, -1.0, 0.0]),
            ];
            let mean = |open: [f64; 3], cpu: bool| {
                let mut b = Blasts::default();
                b.settings.cpu = cpu;
                let at = f.point([0.5, 40.25, 0.5]);
                b.start(at, dynamite(), 0.0, open);
                let base = f.point(at);
                let smoke: Vec<[f64; 3]> = b
                    .puffs(0.8)
                    .iter()
                    .filter(|p| p.kind == PuffKind::Smoke)
                    .map(|p| {
                        let q = f.point(p.at);
                        std::array::from_fn(|k| q[k] - base[k])
                    })
                    .collect();
                let n = smoke.len() as f64;
                let m: [f64; 3] =
                    std::array::from_fn(|k| smoke.iter().map(|v| v[k]).sum::<f64>() / n);
                (m, smoke.len())
            };
            for (name, rock, want) in cases {
                let c = f.cell(centre);
                let open = open_way(e, c, |w| {
                    let l = f.cell(w);
                    rock([l[0] - centre[0], l[1] - centre[1], l[2] - centre[2]])
                });
                let (m, _) = mean(open, false);
                // Along the open side, past the rise (smoke rises 1.1 m/s × 0.8 s).
                let rise = e.effects.smoke.rise_ms * 0.8;
                let out = [m[0], m[1] - rise, m[2]];
                let len = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
                let cos = (out[0] * want[0] + out[1] * want[1] + out[2] * want[2]) / len;
                assert!(cos > 0.8, "{name}: mean {out:.2?} (cos {cos:.2})");
                assert!(len > 1.0, "{name}: {len:.2} m out");
                // The CPU tier draws its few puffs, the same way.
                let (_, n) = mean(open, true);
                assert_eq!(
                    n, e.effects.smoke.cpu_count as usize,
                    "{name}: the CPU's count"
                );
            }
            // Buried: no way out, the even spread round the hemisphere.
            let c = f.cell(centre);
            let open = open_way(e, c, |_| true);
            assert!(open.iter().all(|v| v.abs() < 1e-9), "buried: {open:?}");
            let (m, _) = mean(open, false);
            assert!(m[0].hypot(m[2]) < 0.8, "buried: sideways {m:.2?}");
            let (even, _) = mean([0.0; 3], false);
            assert_eq!(m, even, "buried: as before");
        }
    );

    #[test]
    fn blasts_in_flight_share_the_puffs_budget_and_none_pops() {
        // Owner, 2026-11-07: eight charges by a river dropped the Air to 49 fps on the puffs.
        let e = &EXPLOSIVES[dynamite()];
        let (smoke, dust) = (
            e.effects.smoke.count as usize,
            e.effects.dust.count as usize,
        );
        let max = LIMITS.max_puffs as usize;
        let smoke_dust = |b: &Blasts, t: f64| {
            b.puffs(t)
                .iter()
                .filter(|p| p.kind != PuffKind::Fire)
                .count()
        };
        let mut b = Blasts::default();
        b.start([0.5, 40.25, 0.5], dynamite(), 0.0, [0.0; 3]);
        assert_eq!(b.list[0].share, 1.0, "one blast draws all its puffs");
        assert_eq!(smoke_dust(&b, 0.05), smoke + dust);
        // A chain of eight 0.3 s apart.
        for k in 1..8 {
            b.start(
                [0.5 + 3.0 * f64::from(k), 40.25, 0.5],
                dynamite(),
                0.3 * f64::from(k),
                [0.0; 3],
            );
        }
        let least = shared(smoke, LIMITS.min_puff_share) + shared(dust, LIMITS.min_puff_share);
        assert!(b.list.iter().all(|x| x.share >= LIMITS.min_puff_share));
        let peak = (0..40)
            .map(|k| smoke_dust(&b, 2.1 + 0.01 * f64::from(k)))
            .max()
            .unwrap();
        assert!(
            peak <= max + 8 * least,
            "{peak} puffs at once, the budget {max} (+ the least shares {least} each)"
        );
        assert!(
            peak < 8 * (smoke + dust) / 2,
            "{peak}: the chain draws far fewer than eight blasts' worth"
        );
        // A blast's puffs are fixed at its start: the next one takes none of them.
        let before = b.puffs(2.2);
        let mut c = b.clone();
        c.start([40.5, 40.25, 0.5], dynamite(), 2.2, [0.0; 3]);
        let after = c.puffs(2.2);
        assert!(before.iter().all(|p| after.contains(p)), "no puff pops");
        // Long after, a blast draws all of them again.
        c.prune(60.0);
        c.start([0.5, 40.25, 0.5], dynamite(), 60.0, [0.0; 3]);
        assert_eq!(c.list.last().unwrap().share, 1.0);
    }

    #[test]
    fn each_effect_follows_its_setting_and_a_cpu_draws_fewer() {
        let mut b = Blasts::default();
        b.start([0.5, 40.25, 0.5], dynamite(), 0.0, [0.0; 3]);
        let all = b.puffs(0.2).len();
        assert!(!b.lights(0.1).is_empty());
        assert_ne!(b.shake([3.0, 41.0, 0.0], 0.05), (0.0, 0.0));
        b.settings.cpu = true;
        let cpu = b.puffs(0.2).len();
        assert!(cpu < all, "{cpu} < {all}");
        b.settings = BlastSettings {
            flash: false,
            smoke: false,
            shake: false,
            cpu: false,
        };
        assert!(b.puffs(0.2).is_empty());
        assert!(b.lights(0.1).is_empty());
        assert_eq!(b.shake([3.0, 41.0, 0.0], 0.05), (0.0, 0.0));
    }

    #[test]
    fn the_flash_dies_away_the_shake_is_stronger_near_and_the_blast_is_forgotten() {
        let mut b = Blasts::default();
        b.start([0.5, 40.25, 0.5], dynamite(), 0.0, [0.0; 3]);
        let s0 = b.lights(0.01)[0].strength;
        let s1 = b.lights(0.2)[0].strength;
        assert!(s1 < s0);
        let peak = |eye: [f64; 3]| {
            (0..40)
                .map(|k| {
                    let (y, p) = b.shake(eye, f64::from(k) * 0.01);
                    y.abs().max(p.abs())
                })
                .fold(0.0, f64::max)
        };
        assert!(peak([2.0, 41.0, 0.5]) > peak([20.0, 41.0, 0.5]));
        assert_eq!(peak([200.0, 41.0, 0.5]), 0.0);
        b.prune(30.0);
        assert!(b.list.is_empty());
    }
}
