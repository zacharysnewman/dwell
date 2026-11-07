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
use dwell_world::explosives::{EXPLOSIVES, ExplosiveData, PuffLook, shake_level};

/// A blast going off: where (world m), its explosive (index in `EXPLOSIVES`), when (s), its seed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub at: [f64; 3],
    pub explosive: usize,
    pub start_s: f64,
    pub seed: u32,
}

/// What a puff is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PuffKind {
    Smoke,
    Dust,
    /// The flash's fireball (self-lit).
    Fire,
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
    /// A blast of explosive `explosive` at `at` (its cell's centre), now (`now_s`).
    pub fn start(&mut self, at: [f64; 3], explosive: usize, now_s: f64) {
        let seed = (at[0] as i32 as u32).wrapping_mul(73_856_093)
            ^ (at[1] as i32 as u32).wrapping_mul(19_349_663)
            ^ (at[2] as i32 as u32).wrapping_mul(83_492_791)
            ^ (now_s * 1000.0) as u32;
        self.list.push(Blast {
            at,
            explosive,
            start_s: now_s,
            seed,
        });
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
    let n = if cpu { look.cpu_count } else { look.count };
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
        let dir = [a.cos() * side, up, a.sin() * side];
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
        let alpha = look.opacity * fade_in * (1.0 - u).powf(1.3);
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
            b.start(at, dynamite(), 0.0);
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
                early.iter().any(|p| p.kind == PuffKind::Fire),
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

    #[test]
    fn each_effect_follows_its_setting_and_a_cpu_draws_fewer() {
        let mut b = Blasts::default();
        b.start([0.5, 40.25, 0.5], dynamite(), 0.0);
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
        b.start([0.5, 40.25, 0.5], dynamite(), 0.0);
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
