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
//!   (eroded down the chain, Hawaii-style); the youngest has a crater.
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
    pub cone_radius: f64,
    pub crater_radius: f64,
    pub crater_depth: f64,
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
                let along = (f64::from(k) - f64::from(n.saturating_sub(1)) * 0.5) * self.arc_spacing;
                let (x, z) = self.point(along, self.arc_distance);
                v.push(Volcano {
                    x,
                    z,
                    height: self.cone_height,
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
                crater_radius: if k == 0 { self.crater_radius } else { 0.0 },
                crater_depth: if k == 0 { self.crater_depth } else { 0.0 },
                chain: k + 1,
            });
        }
        v
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
                let phase = if across > 0.0 { along + self.shift } else { along };
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
            let rim = v.height * (1.0 - v.crater_radius / v.radius.max(1.0)).max(0.0).powf(1.8);
            let t = r / v.crater_radius;
            rim - v.crater_depth * (1.0 - t * t)
        } else {
            body
        }
    }

    /// Everything the layer adds at (x, z) (m), given the island's coast distance there.
    pub fn height(&self, volcanoes: &[Volcano], x: f64, z: f64, coast: f64) -> f64 {
        let mut cones = 0.0f64;
        for v in volcanoes {
            let r = ((x - v.x).powi(2) + (z - v.z).powi(2)).sqrt();
            cones = cones.max(Self::cone(v, r));
        }
        self.boundary(x, z, coast) + cones
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
