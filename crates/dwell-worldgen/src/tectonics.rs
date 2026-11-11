//! Tectonics at realistic scale (systemic terrain T5, ADR 0075; `terrain.json` `tectonics`): one
//! plate boundary crossing the island and one hotspot chain offshore, as landforms added to the base
//! height field the world map samples (so the uplift, erosion, lakes and rivers all follow them).
//!
//! - **The boundary** is a straight line (face-local: `angle` degrees from +x toward +z, `offset` m
//!   from the centre along its normal). The overriding plate lies on its normal's + side. Its `kind`:
//!   **convergent** (subduction): a range `rangeHeight` m high, `width` m half-wide, on the overriding
//!   side, a trench `trenchDepth` m deep on the other; **divergent**: a rift valley `riftDepth` m deep
//!   along the line with raised shoulders (its floor holds lakes); **transform**: ridges across the
//!   line, offset by `shift` m between the two sides.
//! - **The volcanic arc** (convergent only): `arcCount` stratovolcanoes `arcDistance` m behind the
//!   boundary, `arcSpacing` m apart along it: steep concave cones `coneHeight` m high and `coneRadius`
//!   m in radius, each with a crater `craterRadius` m wide and `craterDepth` m deep (dormant).
//! - **The hotspot chain**: `hotspotCount` volcanic islands from (`hotspotX`, `hotspotZ`) in the plate's
//!   motion (`hotspotHeading` degrees), `hotspotSpacing` m apart: the youngest `hotspotHeight` m over
//!   the sea floor, each older one `hotspotDecay` of the last's height and `hotspotWiden` broader
//!   (eroded down the chain, Hawaii-style); shields without a crater.
//!
//! Neutral by default (`kind` 0, no arc, no hotspots): `enabled` is false and the base field skips it,
//! so the terrain a seed generates is unchanged. Face-local, so both faces.

use std::f64::consts::PI;

/// The boundary's kind (`terrain.json` `tectonics.kind`): 0 none.
pub const NONE: u32 = 0;
pub const CONVERGENT: u32 = 1;
pub const DIVERGENT: u32 = 2;
pub const TRANSFORM: u32 = 3;

/// The layer's data (`terrain.json` `tectonics`; units there).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tectonics {
    pub kind: u32,
    pub angle: f64,
    pub offset: f64,
    pub width: f64,
    pub range_height: f64,
    pub trench_depth: f64,
    pub rift_depth: f64,
    pub shift: f64,
    pub arc_count: u32,
    pub arc_distance: f64,
    pub arc_spacing: f64,
    pub cone_height: f64,
    /// Each next arc cone's height as a share of the last's (1: all `cone_height`).
    pub arc_decay: f64,
    pub cone_radius: f64,
    pub crater_radius: f64,
    pub crater_depth: f64,
    /// How much the arc's cones resist the erosion (0..1: their erodibility × 1 − this toward the
    /// summit), so a tall cone keeps its relief through the world map's erosion.
    pub cone_hardness: f64,
    /// The arc cones' radial ridges (0.52.40, Rainier): how many, how deep (m) the glacier-cut
    /// valleys between them, and how irregular their spacing (radians of angular warp).
    pub ridge_count: u32,
    pub ridge_depth: f64,
    pub ridge_warp: f64,
    /// The summit's offset (m) from the cone's centre toward `summit_angle` degrees (the foot
    /// stays put: an asymmetric cone).
    pub summit_offset: f64,
    pub summit_angle: f64,
    /// A shoulder peak (m high, m from the summit, toward degrees, m wide).
    pub shoulder_height: f64,
    pub shoulder_distance: f64,
    pub shoulder_angle: f64,
    pub shoulder_radius: f64,
    /// Parasitic flank cones: how many, m high, m wide, m from the centre.
    pub vent_count: u32,
    pub vent_height: f64,
    pub vent_radius: f64,
    pub vent_distance: f64,
    /// A crater whose rim stands above `ice_above` m holds an ice dome `ice_dome` m over its rim
    /// instead of a hollow (no crater lake up there).
    pub ice_above: f64,
    pub ice_dome: f64,
    /// The snow line: cover below `snow_below_c` °C where no steeper than `snow_max_slope`.
    pub snow_below_c: f64,
    /// The glacier: gentle ground colder than `ice_below_c` °C is ice (`rocks.json`
    /// `tectonic.ice`); −100 is off.
    pub ice_below_c: f64,
    pub snow_max_slope: f64,
    /// The column-level detail (the Rainier detail; added where the chunks are generated, on
    /// top of the 100 m map's cone, so it survives the map's sampling): cleavers, `cleaver_depth` m
    /// deeper valleys between sharp crests, `cleaver_count` of them round the cone (0: the
    /// ridges' count, along the same crests).
    pub cleaver_count: u32,
    pub cleaver_depth: f64,
    /// The summit crater's rim broken by `rim_notches` notches `rim_notch_depth` m deep.
    pub rim_notches: u32,
    pub rim_notch_depth: f64,
    /// `crag_count` crags on and just outside the rim, up to `crag_height` m high and
    /// `crag_radius` m wide: the highest one the summit (off the crater's centre).
    pub crag_count: u32,
    pub crag_height: f64,
    pub crag_radius: f64,
    /// `cirque_count` glacier-carved bowls at the heads of valleys between the ridges,
    /// `cirque_distance` m from the summit, `cirque_depth` m deep and `cirque_radius` m wide,
    /// with steep headwalls on their summit side and a lip on the downhill side.
    pub cirque_count: u32,
    pub cirque_depth: f64,
    pub cirque_radius: f64,
    pub cirque_distance: f64,
    pub hotspot_count: u32,
    pub hotspot_x: f64,
    pub hotspot_z: f64,
    pub hotspot_heading: f64,
    pub hotspot_spacing: f64,
    pub hotspot_height: f64,
    pub hotspot_radius: f64,
    pub hotspot_decay: f64,
    pub hotspot_widen: f64,
    /// How far (m) behind the boundary the overriding plate's old core begins (its granite).
    pub core_distance: f64,
}

/// A volcano: its centre (face-local m), height (m added), radius (m), crater radius and depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volcano {
    pub x: f64,
    pub z: f64,
    pub height: f64,
    pub radius: f64,
    pub crater_radius: f64,
    pub crater_depth: f64,
    /// 0 the arc's, or the hotspot chain's age index + 1.
    pub chain: u32,
}

/// The cone a lava vent sits on (`tectonics.lavaVents[].cone` and `index`): the arc's `index`-th
/// cone, or the hotspot chain's, youngest first (docs/proposals/VOLCANOES.md §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cone {
    Arc(u32),
    Hotspot(u32),
}

/// A lava vent's kind (`kind`): a lake held at its level, or a steady river.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LavaVentKind {
    Lake,
    River,
}

/// A lava vent on a cone (`terrain.json` `tectonics.lavaVents`, opt-in per world: none by default),
/// placed by the server when its world is created or first loaded (docs/proposals/VOLCANOES.md §6.2).
#[derive(Clone, Debug, PartialEq)]
pub struct LavaVent {
    pub cone: Cone,
    pub kind: LavaVentKind,
    /// The hot fluid (`shared/fluids/lava.json` `fluids`), by name.
    pub fluid: String,
    pub rate_m3s: f32,
    /// A lake's held surface over the vent column's bed (m).
    pub depth_m: f32,
    /// Its field reaches this many columns each way from the vent.
    pub radius: u32,
    /// The vent's offset from the cone's summit (m) toward `angle` degrees (0: on the summit).
    pub distance: f64,
    pub angle: f64,
    /// The faces it is placed on (`faces`, default both: each face generates its own cone, so a
    /// river's way to the sea can suit one face only): face A, face B.
    pub faces: [bool; 2],
}

/// An arc cone's column-level detail, placed once per map ([`Tectonics::peak_details`]): its
/// cone, summit, cirques (x, z, outward heading in radians, floor height m) and crags (x, z,
/// height m).
#[derive(Clone, Debug, PartialEq)]
pub struct PeakDetail {
    pub cone: Volcano,
    pub apex: (f64, f64),
    pub cirques: Vec<(f64, f64, f64, f64)>,
    pub crags: Vec<(f64, f64, f64)>,
}

/// The fractional part of `x` (0..1).
fn frac(x: f64) -> f64 {
    x - x.floor()
}

/// The angle from `b` to `a`, wrapped to −π..π.
fn wrap(a: f64) -> f64 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// `ice_above`'s off value (no crater is ever that high).
const fn v_max() -> f64 {
    100_000.0
}

/// A smooth bump: 1 at 0, 0 from |t| ≥ 1.
fn bump(t: f64) -> f64 {
    let t = t.abs();
    if t >= 1.0 {
        0.0
    } else {
        let u = 1.0 - t * t;
        u * u
    }
}

impl Tectonics {
    /// Whether the layer adds anything.
    pub fn enabled(&self) -> bool {
        self.kind != NONE || self.hotspot_count > 0
    }

    /// The boundary's frame at (x, z): (along the line, across it: + the overriding plate).
    pub fn frame(&self, x: f64, z: f64) -> (f64, f64) {
        let a = self.angle * PI / 180.0;
        let (c, s) = (a.cos(), a.sin());
        (x * c + z * s, -x * s + z * c - self.offset)
    }

    fn point(&self, along: f64, across: f64) -> (f64, f64) {
        let a = self.angle * PI / 180.0;
        let (c, s) = (a.cos(), a.sin());
        let across = across + self.offset;
        (along * c - across * s, along * s + across * c)
    }

    /// Every volcano: the arc's (convergent only), then the hotspot chain youngest first.
    pub fn volcanoes(&self) -> Vec<Volcano> {
        let mut v = Vec::new();
        if self.kind == CONVERGENT {
            let n = self.arc_count;
            for k in 0..n {
                let along =
                    (f64::from(k) - f64::from(n.saturating_sub(1)) * 0.5) * self.arc_spacing;
                let (x, z) = self.point(along, self.arc_distance);
                v.push(Volcano {
                    x,
                    z,
                    height: if self.arc_decay == 1.0 {
                        self.cone_height
                    } else {
                        self.cone_height * self.arc_decay.powi(k as i32)
                    },
                    radius: self.cone_radius,
                    crater_radius: self.crater_radius,
                    crater_depth: self.crater_depth,
                    chain: 0,
                });
            }
        }
        let a = self.hotspot_heading * PI / 180.0;
        for k in 0..self.hotspot_count {
            let d = f64::from(k) * self.hotspot_spacing;
            let age = f64::from(k);
            v.push(Volcano {
                x: self.hotspot_x + d * a.cos(),
                z: self.hotspot_z + d * a.sin(),
                height: self.hotspot_height * self.hotspot_decay.powf(age),
                radius: self.hotspot_radius * (1.0 + self.hotspot_widen * age),
                crater_radius: 0.0,
                crater_depth: 0.0,
                chain: k + 1,
            });
        }
        v
    }

    /// Where vent `v` stands (face-local m): its cone's summit (an arc cone's offset summit, a
    /// hotspot island's centre), moved `distance` m toward `angle`; `None` for a cone the layer
    /// doesn't have.
    pub fn vent_site(&self, v: &LavaVent) -> Option<(f64, f64)> {
        let all = self.volcanoes();
        let cone = match v.cone {
            Cone::Arc(i) => all.iter().filter(|c| c.chain == 0).nth(i as usize),
            Cone::Hotspot(i) => all.iter().find(|c| c.chain == i + 1),
        }?;
        let (x, z) = match v.cone {
            Cone::Arc(_) if self.shaped() => self.summit(cone),
            _ => (cone.x, cone.z),
        };
        let a = v.angle * PI / 180.0;
        Some((x + v.distance * a.cos(), z + v.distance * a.sin()))
    }

    /// The boundary's landform at (x, z) (m added), faded offshore by `coast` (the island's signed
    /// coast distance, m) so a range dies out under the sea.
    pub fn boundary(&self, x: f64, z: f64, coast: f64) -> f64 {
        let (along, across) = self.frame(x, z);
        let w = self.width.max(1.0);
        let land = ((coast + 3000.0) / 4000.0).clamp(0.0, 1.0);
        let land = land * land * (3.0 - 2.0 * land);
        let h = match self.kind {
            CONVERGENT => {
                self.range_height * bump((across - w) / w)
                    - self.trench_depth * bump((across + 0.5 * w) / (0.5 * w))
            }
            DIVERGENT => {
                -self.rift_depth * bump(across / w)
                    + 0.3 * self.rift_depth * bump((across.abs() - 1.5 * w) / w)
            }
            TRANSFORM => {
                // Ridges across the line, the + side's shifted along it by `shift`.
                let phase = if across > 0.0 {
                    along + self.shift
                } else {
                    along
                };
                let ridge = 0.5 + 0.5 * (2.0 * PI * phase / (4.0 * w)).cos();
                self.range_height * ridge * ridge * bump(across / (3.0 * w))
            }
            _ => 0.0,
        };
        h * land
    }

    /// A volcano's height profile at distance `r`: a concave stratovolcano (steeper near the top),
    /// with its crater cut into the summit.
    pub fn cone(v: &Volcano, r: f64) -> f64 {
        let u = r / v.radius.max(1.0);
        if u >= 1.0 {
            return 0.0;
        }
        let body = v.height * (1.0 - u).powf(1.8);
        if v.crater_radius > 0.0 && r < v.crater_radius {
            let rim = v.height
                * (1.0 - v.crater_radius / v.radius.max(1.0))
                    .max(0.0)
                    .powf(1.8);
            let t = r / v.crater_radius;
            rim - v.crater_depth * (1.0 - t * t)
        } else {
            body
        }
    }

    /// Whether the arc's cones take the Rainier shape (ridges, an offset summit, a shoulder,
    /// vents, an ice-filled crater); else [`Tectonics::cone`]'s plain profile.
    pub fn shaped(&self) -> bool {
        (self.ridge_count > 0 && self.ridge_depth > 0.0)
            || self.summit_offset > 0.0
            || self.shoulder_height > 0.0
            || (self.vent_count > 0 && self.vent_height > 0.0)
            || self.ice_above < v_max()
    }

    /// Whether the snow line is on (`snow_below_c` above −100 °C).
    pub fn snow_line(&self) -> bool {
        self.snow_below_c > -100.0
    }

    /// Whether the glacier is on (`ice_below_c` above −100 °C, with the snow line on).
    pub fn glacier(&self) -> bool {
        self.snow_line() && self.ice_below_c > -100.0
    }

    /// An arc cone's summit (face-local m): `summit_offset` m from its centre toward `summit_angle`.
    pub fn summit(&self, v: &Volcano) -> (f64, f64) {
        let a = self.summit_angle * PI / 180.0;
        (
            v.x + self.summit_offset * a.cos(),
            v.z + self.summit_offset * a.sin(),
        )
    }

    /// An arc cone shaped like Rainier at (x, z) (m added): the concave profile measured from the
    /// offset summit out to the unmoved foot circle, the crater cut (or domed with ice) round the
    /// summit, radial valleys cut between sharp ridges, then the shoulder and the flank vents.
    pub fn shaped_cone(&self, v: &Volcano, x: f64, z: f64) -> f64 {
        let (ax, az) = self.summit(v);
        let (dx, dz) = (x - ax, z - az);
        let r = (dx * dx + dz * dz).sqrt();
        let big_r = v.radius.max(1.0);
        // The distance from the summit to the foot circle along the ray through (x, z).
        let (ox, oz) = (ax - v.x, az - v.z);
        let (ux, uz) = if r > 1e-9 {
            (dx / r, dz / r)
        } else {
            (1.0, 0.0)
        };
        let od = ox * ux + oz * uz;
        let foot = -od
            + (od * od - (ox * ox + oz * oz) + big_r * big_r)
                .max(0.0)
                .sqrt();
        let foot = foot.max(1.0);
        let u = r / foot;
        let mut h = 0.0f64;
        if u < 1.0 {
            h = v.height * (1.0 - u).powf(1.8);
            if v.crater_radius > 0.0 && r < v.crater_radius {
                let rim = v.height * (1.0 - v.crater_radius / foot).max(0.0).powf(1.8);
                let t = r / v.crater_radius;
                h = if rim > self.ice_above {
                    rim + self.ice_dome * (1.0 - t * t)
                } else {
                    rim - v.crater_depth * (1.0 - t * t)
                };
            }
            if self.ridge_count > 0 && self.ridge_depth > 0.0 {
                let th = dz.atan2(dx);
                // |sin| has its cusps at the ridges' crests (sharp cleavers) and round floors
                // between (glacier-cut valleys); each valley's depth varies round the cone.
                let valley = self.valley(self.ridge_count, th).powf(0.6);
                let deep = Self::deep(th);
                let s = (u / 0.2).clamp(0.0, 1.0);
                let env = s * s * (3.0 - 2.0 * s) * ((1.0 - u) / 0.8).min(1.0);
                h -= self.ridge_depth * env * valley * deep;
            }
        }
        if self.shoulder_height > 0.0 {
            let a = self.shoulder_angle * PI / 180.0;
            let (sx, sz) = (
                ax + self.shoulder_distance * a.cos(),
                az + self.shoulder_distance * a.sin(),
            );
            let t = ((x - sx).powi(2) + (z - sz).powi(2)).sqrt() / self.shoulder_radius.max(1.0);
            if t < 1.0 {
                h += self.shoulder_height * (1.0 - t).powf(1.5);
            }
        }
        if self.vent_count > 0 && self.vent_height > 0.0 {
            for k in 0..self.vent_count {
                // Round the cone by the golden angle from the summit's side.
                let a = (self.summit_angle + 40.0 + 137.5 * f64::from(k)) * PI / 180.0;
                let (cx, cz) = (
                    v.x + self.vent_distance * a.cos(),
                    v.z + self.vent_distance * a.sin(),
                );
                let t = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt() / self.vent_radius.max(1.0);
                if t < 1.0 {
                    h += self.vent_height * (1.0 - t).powf(1.5);
                }
            }
        }
        h.max(0.0)
    }

    /// The ridges' angular pattern at heading `th` (radians from the summit) for `n` ridges: 0 on a
    /// crest, 1 mid-valley (|sin| of the warped angle, so the crests are cusps).
    fn valley(&self, n: u32, th: f64) -> f64 {
        let w = th
            + self.ridge_warp * (3.0 * th + 1.3).sin()
            + 0.5 * self.ridge_warp * (5.0 * th + 0.4).sin();
        (0.5 * f64::from(n) * w).sin().abs()
    }

    /// How deep each valley is round the cone (0.4..1).
    fn deep(th: f64) -> f64 {
        0.7 + 0.3 * (2.0 * th + 0.9).sin()
    }

    /// Whether the arc's cones take column-level detail (cleavers, a broken rim, crags, cirques).
    pub fn detailed(&self) -> bool {
        self.kind == CONVERGENT
            && self.arc_count > 0
            && ((self.cleaver_depth > 0.0 && (self.cleaver_count > 0 || self.ridge_count > 0))
                || (self.rim_notches > 0 && self.rim_notch_depth > 0.0)
                || (self.crag_count > 0 && self.crag_height > 0.0)
                || (self.cirque_count > 0 && self.cirque_depth > 0.0))
    }

    /// The arc cones' column-level detail, placed once (empty when [`Tectonics::detailed`] is
    /// off): each cirque at the head of the valley nearest its share of the circle, its floor
    /// `cirque_depth` m under `ground` (the map's height, m) there; the crags round the rim by the
    /// golden angle, the first the tallest.
    pub fn peak_details(&self, ground: impl Fn(f64, f64) -> f64) -> Vec<PeakDetail> {
        if !self.detailed() {
            return Vec::new();
        }
        let shaped = self.shaped();
        self.volcanoes()
            .into_iter()
            .filter(|v| v.chain == 0)
            .map(|v| {
                let apex = if shaped { self.summit(&v) } else { (v.x, v.z) };
                let n = self.cirque_count;
                let cirques = (0..if self.cirque_depth > 0.0 { n } else { 0 })
                    .map(|k| {
                        let k = f64::from(k);
                        let mut th = (self.summit_angle
                            + 15.0
                            + 360.0 * (k + 0.2 * frac(0.618 * k)) / f64::from(n))
                        .to_radians();
                        // The valley's head: the most valley-like heading within half a ridge's
                        // spacing.
                        if self.ridge_count > 0 {
                            let span = PI / f64::from(self.ridge_count);
                            let best = (0..=32)
                                .map(|i| th + span * (f64::from(i) / 16.0 - 1.0))
                                .fold((th, -1.0), |b, a| {
                                    let s = self.valley(self.ridge_count, a);
                                    if s > b.1 { (a, s) } else { b }
                                });
                            th = best.0;
                        }
                        let (x, z) = (
                            apex.0 + self.cirque_distance * th.cos(),
                            apex.1 + self.cirque_distance * th.sin(),
                        );
                        // Its floor under the ground there, cleavers and all.
                        let u = self.cirque_distance / self.detail_reach(&v);
                        let floor = ground(x, z) - self.cleaver_cut(u, th) - self.cirque_depth;
                        (x, z, th, floor)
                    })
                    .collect();
                let rim = v.crater_radius.max(50.0);
                let crags = (0..if self.crag_height > 0.0 {
                    self.crag_count
                } else {
                    0
                })
                    .map(|k| {
                        let kf = f64::from(k);
                        let a = (self.summit_angle + 70.0 + 137.5 * kf).to_radians();
                        let d = rim * (1.0 + 0.3 * frac(0.382 * kf));
                        let h = self.crag_height * (1.0 - 0.45 * frac(0.618 * kf));
                        (apex.0 + d * a.cos(), apex.1 + d * a.sin(), h)
                    })
                    .collect();
                PeakDetail {
                    cone: v,
                    apex,
                    cirques,
                    crags,
                }
            })
            .collect()
    }

    /// The cleavers' cut (m) at `u` of the way from the summit to the foot, heading `th`: the
    /// valleys between the crests cut `cleaver_depth` deeper (none at the summit and the foot).
    fn cleaver_cut(&self, u: f64, th: f64) -> f64 {
        let n = if self.cleaver_count > 0 {
            self.cleaver_count
        } else {
            self.ridge_count
        };
        if self.cleaver_depth <= 0.0 || n == 0 || u >= 1.0 {
            return 0.0;
        }
        let s = (u / 0.1).clamp(0.0, 1.0);
        let env = s * s * (3.0 - 2.0 * s) * ((1.0 - u) / 0.6).clamp(0.0, 1.0);
        self.cleaver_depth * env * self.valley(n, th).powf(0.35) * Self::deep(th)
    }

    /// The radius (m) the column-level detail reaches round an arc cone's summit.
    fn detail_reach(&self, v: &Volcano) -> f64 {
        v.radius.max(1.0) + self.summit_offset
    }

    /// The column-level detail at (x, z) on ground `h` m high (the map's, with the biomes' fine
    /// relief): `h` with the cleavers cut, the rim notched, the crags raised and the cirques
    /// carved; `h` itself away from every cone.
    pub fn peak_relief(&self, peaks: &[PeakDetail], x: f64, z: f64, h: f64) -> f64 {
        let mut out = h;
        for p in peaks {
            let v = &p.cone;
            let (dx, dz) = (x - p.apex.0, z - p.apex.1);
            let r2 = dx * dx + dz * dz;
            let big_r = self.detail_reach(v);
            if r2 >= big_r * big_r {
                continue;
            }
            let r = r2.sqrt();
            let th = dz.atan2(dx);
            let u = r / big_r;
            // Cleavers: the valleys between the ridges cut deeper, the crests left as cusps.
            out -= self.cleaver_cut(u, th);
            // The rim's notches: an irregular ring of cuts through the crater's rim.
            let rim = v.crater_radius.max(50.0);
            if self.rim_notches > 0 && self.rim_notch_depth > 0.0 && r < 2.5 * rim {
                let band = bump((r - rim) / (0.8 * rim));
                if band > 0.0 {
                    let m = f64::from(self.rim_notches);
                    let width = 0.35 * PI / m;
                    let notch = (0..self.rim_notches)
                        .map(|k| {
                            let k = f64::from(k);
                            let a = self.summit_angle.to_radians()
                                + 2.0 * PI * (k + 0.4 * (2.3 * k + 0.7).sin()) / m;
                            bump(wrap(th - a) / width)
                        })
                        .fold(0.0, f64::max);
                    out -= self.rim_notch_depth * band * notch;
                }
            }
            // Crags: sharp spires on and just outside the rim.
            for &(cx, cz, ch) in &p.crags {
                let t = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt() / self.crag_radius.max(1.0);
                if t < 1.0 {
                    out += ch * (1.0 - t).powf(1.3);
                }
            }
            // Cirques: a bowl under each, its floor level, a steep headwall on the summit side
            // (a short radius) and a lip downhill (a long one): the ground no higher than it.
            let cr = self.cirque_radius.max(1.0);
            for &(cx, cz, ch, floor) in &p.cirques {
                let (ex, ez) = (ch.cos(), ch.sin());
                let (px, pz) = (x - cx, z - cz);
                let s = px * ex + pz * ez;
                let c = -px * ez + pz * ex;
                let rs = if s < 0.0 { 0.5 * cr } else { 0.9 * cr };
                let t2 = (s / rs).powi(2) + (c / (0.75 * cr)).powi(2);
                if t2 < 9.0 {
                    out = out.min(floor + self.cirque_depth * t2);
                }
            }
        }
        out
    }

    /// Everything the layer adds at (x, z) (m), given the island's coast distance there.
    pub fn height(&self, volcanoes: &[Volcano], x: f64, z: f64, coast: f64) -> f64 {
        let shaped = self.shaped();
        let mut cones = 0.0f64;
        for v in volcanoes {
            let c = if shaped && v.chain == 0 {
                self.shaped_cone(v, x, z)
            } else {
                let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
                Self::cone(v, r)
            };
            cones = cones.max(c);
        }
        self.boundary(x, z, coast) + cones
    }

    /// How much the erosion is held off at (x, z) (0..`cone_hardness`): the arc's cones, fully at
    /// the summit and fading to none at the foot (a smooth step), so a tall cone keeps its relief.
    pub fn shield(&self, volcanoes: &[Volcano], x: f64, z: f64) -> f64 {
        if self.cone_hardness <= 0.0 {
            return 0.0;
        }
        let mut s = 0.0f64;
        for v in volcanoes.iter().filter(|v| v.chain == 0) {
            let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
            let t = (1.0 - r / v.radius.max(1.0)).clamp(0.0, 1.0);
            s = s.max(t * t * (3.0 - 2.0 * t));
        }
        self.cone_hardness.clamp(0.0, 1.0) * s
    }

    /// The tectonic rock province at (x, z): `Some(true)` on a volcano (its lava rock), `Some(false)`
    /// in the overriding plate's old core (`core_distance` m behind a convergent boundary, past the
    /// arc's cones), None elsewhere (the noise's provinces).
    pub fn province(&self, volcanoes: &[Volcano], x: f64, z: f64) -> Option<bool> {
        if !self.enabled() {
            return None;
        }
        if volcanoes
            .iter()
            .any(|v| (x - v.x).powi(2) + (z - v.z).powi(2) < (0.85 * v.radius).powi(2))
        {
            return Some(true);
        }
        (self.kind == CONVERGENT && self.frame(x, z).1 > self.core_distance).then_some(false)
    }
}
