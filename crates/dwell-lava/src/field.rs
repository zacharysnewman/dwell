//! A lava field: one hot fluid on its own surface grid of 1 m columns (a bed height, a lava depth
//! and a heat per column), its vents, and the near/far schedule (docs/proposals/VOLCANOES.md §6).
//!
//! **The law.** Lava is a viscous gravity current: the flow per metre of width across a face is
//! `g × h³ × slope / (3 × ν)` (the lubrication law for a thin layer, ν the kinematic viscosity),
//! through the depth `h` that stands above the higher bed less the fluid's yield depth (a Bingham
//! plug: thin lava does not move, so a front stands as a lobe). It is explicit and diffusive, so a
//! substep is at most `cfl × cell² / (4 × g h³ / 3ν)`. Water's grid (`dwell-sim` `water.rs`) moves
//! by the local-inertial law instead; lava's is slower by orders of magnitude for the same slope.
//!
//! **Heat and cooling.** A column's heat runs from 1 (as a vent gives it) to 0 (rock). Heat is
//! carried with the flow (upwind), and each column loses it at `1 / coolingSeconds` a second, faster
//! by `edgeCooling` per dry neighbour (its edges) and in proportion to `thinDepthM` over its depth
//! (its top's share: thin lava cools fast, a deep lake slowly). A
//! column at 0 turns to the fluid's rock: its lava is added to its bed and the cells whose centres
//! it now covers become the rock block, so a river crusts over behind its front and builds land.
//!
//! A lake vent keeps the lava joined to it below its level molten (a lava lake convects); what
//! runs over the rim cools.
//!
//! **Quench.** Lava in or beside a water column turns to the quench rock at once, with a steam
//! event that takes `steamWaterM` of that water (the water grid applies it once wired).
//!
//! **Schedule.** Like the water: a field with a player within `nearRadiusM` runs every lava tick;
//! a far one owes its time and runs it in one go once it owes `farEverySeconds` (fast-forward). A
//! run substeps at the stable step either way, so the two agree.
//!
//! Face-local: a field's columns, beds and depths are face-local (+y up); a world point or cell
//! crosses through [`FaceFrame`] only ([`Field::submerged`], [`Field::is_near`], [`LavaEvent::cells`]).

use std::ops::Range;

use dwell_world::FaceFrame;

use crate::data::{HotFluid, LavaData, VentDef, VentKind};

const G: f32 = 9.81;

/// The cell row boundary nearest a face-local height (m): rows are `CELL_H` tall (½ m,
/// HALF_HEIGHT_GRID.md §2), so a column's lava and rock are written in whole rows.
pub fn row_at(m: f32) -> i32 {
    (f64::from(m) / dwell_world::CELL_H).round() as i32
}
/// A depth below which a column holds no lava (m).
const FILM: f32 = 1e-4;

/// A vent in a field: its column and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vent {
    pub kind: VentKind,
    pub x: usize,
    pub z: usize,
    pub rate_m3s: f32,
    /// A lake's held surface (face-local m).
    pub level_m: f32,
}

impl From<&VentDef> for Vent {
    fn from(v: &VentDef) -> Self {
        Self {
            kind: v.kind,
            x: v.x as usize,
            z: v.z as usize,
            rate_m3s: v.rate_m3s,
            level_m: v.level_m,
        }
    }
}

/// What a step did that the world (blocks, the water grid, the clients) applies.
#[derive(Clone, Debug, PartialEq)]
pub enum LavaEvent {
    /// Lava turned to `rock` in a column (face-local column of the field): the face-local rows
    /// `rows` are now that block.
    Rock {
        x: usize,
        z: usize,
        rows: Range<i32>,
        rock: &'static str,
    },
    /// Lava met water in a column: `water_m` of the water's depth there becomes steam.
    Steam { x: usize, z: usize, water_m: f32 },
}

impl LavaEvent {
    /// A rock event's world cells on face `f`, for the field at face-local `origin`.
    pub fn cells(&self, f: FaceFrame, origin: [i32; 2]) -> Vec<[i32; 3]> {
        match self {
            Self::Rock { x, z, rows, .. } => rows
                .clone()
                .map(|r| f.cell([origin[0] + *x as i32, r, origin[1] + *z as i32]))
                .collect(),
            Self::Steam { .. } => Vec::new(),
        }
    }
}

/// One hot fluid's surface grid.
#[derive(Clone, Debug)]
pub struct Field {
    pub w: usize,
    pub h: usize,
    /// The face-local x, z of column (0, 0).
    pub origin: [i32; 2],
    /// Bed top (face-local m): the ground, raised as lava cools on it.
    pub ground: Vec<f32>,
    /// Lava depth (m).
    pub depth: Vec<f32>,
    /// Heat, 1 at the vent to 0 (rock).
    pub heat: Vec<f32>,
    /// Columns where water stands (the water grid's, once wired).
    pub water: Vec<bool>,
    pub vents: Vec<Vent>,
    pub fluid: &'static HotFluid,
    pub rules: LavaData,
    /// Lava time owed while far (s).
    pub owed_s: f32,
    /// m³ the vents brought, m³ turned to rock.
    pub brought: f64,
    pub rocked: f64,
    /// What steps did since the world last took them.
    pub events: Vec<LavaEvent>,
    qx: Vec<f32>,
    qz: Vec<f32>,
    scale: Vec<f32>,
    /// This step's lake columns (held molten by their vent).
    held: Vec<bool>,
}

impl Field {
    /// A dry field of `w × h` columns on flat ground at 0, of the fluid `fluid` (by name) under
    /// `rules`. An error names a fluid the data doesn't have.
    pub fn new(rules: &LavaData, fluid: &str, w: usize, h: usize) -> Result<Self, String> {
        let fluid = rules
            .fluid(fluid)
            .ok_or_else(|| format!("no hot fluid named {fluid:?}"))?;
        let n = w * h;
        Ok(Self {
            w,
            h,
            origin: [0, 0],
            ground: vec![0.0; n],
            depth: vec![0.0; n],
            heat: vec![0.0; n],
            water: vec![false; n],
            vents: Vec::new(),
            fluid,
            rules: *rules,
            owed_s: 0.0,
            brought: 0.0,
            rocked: 0.0,
            events: Vec::new(),
            qx: vec![0.0; n],
            qz: vec![0.0; n],
            scale: vec![1.0; n],
            held: vec![false; n],
        })
    }

    /// Adds a vent from data; an error names a vent of another fluid or off the field.
    pub fn add_vent(&mut self, v: &VentDef) -> Result<(), String> {
        if v.fluid != self.fluid.name {
            return Err(format!(
                "vent {}: feeds {}, the field holds {}",
                v.name, v.fluid, self.fluid.name
            ));
        }
        if v.x as usize >= self.w || v.z as usize >= self.h {
            return Err(format!(
                "vent {}: column ({}, {}) is off the {}×{} field",
                v.name, v.x, v.z, self.w, self.h
            ));
        }
        self.vents.push(Vent::from(v));
        Ok(())
    }

    pub fn at(&self, x: usize, z: usize) -> usize {
        z * self.w + x
    }

    /// The lava surface of column `i` (face-local m).
    pub fn surface(&self, i: usize) -> f32 {
        self.ground[i] + self.depth[i]
    }

    /// The face-local cell rows column `i`'s lava fills (empty when it holds none): from its bed's
    /// row boundary to its surface's, so a cell is lava once lava covers most of it.
    pub fn lava_rows(&self, i: usize) -> Range<i32> {
        if self.depth[i] <= 0.0 {
            let r = row_at(self.ground[i]);
            return r..r;
        }
        row_at(self.ground[i])..row_at(self.surface(i)).max(row_at(self.ground[i]))
    }

    /// The lava stored (m³).
    pub fn stored(&self) -> f64 {
        self.depth.iter().map(|&d| f64::from(d)).sum()
    }

    /// The depth of lava over the higher of two beds that moves (m), from the higher surface.
    fn moving(&self, up: usize, down: usize) -> f32 {
        let h = self.surface(up) - self.ground[up].max(self.ground[down]);
        (h.min(self.depth[up]) - self.fluid.yield_depth_m).max(0.0)
    }

    /// The flow across the face from `a` to `b` (m²/s, negative toward `a`).
    fn face(&self, a: usize, b: usize) -> f32 {
        let (ea, eb) = (self.surface(a), self.surface(b));
        let (up, down, sign) = if ea >= eb { (a, b, 1.0) } else { (b, a, -1.0) };
        let he = self.moving(up, down);
        if he <= 0.0 {
            return 0.0;
        }
        sign * G * he * he * he * (ea - eb).abs() / (3.0 * self.fluid.viscosity_m2s)
    }

    /// The longest stable substep now (s).
    pub fn max_step(&self) -> f32 {
        let deepest = self.depth.iter().copied().fold(0.0f32, f32::max);
        let he = (deepest - self.fluid.yield_depth_m).max(0.05);
        let d = G * he * he * he / (3.0 * self.fluid.viscosity_m2s);
        (self.rules.cfl / (4.0 * d)).min(5.0)
    }

    fn neighbours(&self, i: usize) -> impl Iterator<Item = Option<usize>> + '_ {
        let (x, z) = (i % self.w, i / self.w);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < self.w).then_some(i + 1),
            (z > 0).then(|| i - self.w),
            (z + 1 < self.h).then_some(i + self.w),
        ]
        .into_iter()
    }

    /// Turns column `i`'s lava into `rock`, raising its bed.
    fn solidify(&mut self, i: usize, rock: &'static str) {
        let d = self.depth[i];
        if d <= 0.0 {
            return;
        }
        let from = self.ground[i];
        self.ground[i] += d;
        self.depth[i] = 0.0;
        self.heat[i] = 0.0;
        self.rocked += f64::from(d);
        let rows = row_at(from)..row_at(self.ground[i]);
        if !rows.is_empty() {
            self.events.push(LavaEvent::Rock {
                x: i % self.w,
                z: i / self.w,
                rows,
                rock,
            });
        }
    }

    /// One substep of `dt` lava seconds (at most [`Field::max_step`]).
    pub fn step(&mut self, dt: f32) {
        let (w, h) = (self.w, self.h);
        for k in 0..self.vents.len() {
            let v = self.vents[k];
            let i = self.at(v.x, v.z);
            let add = match v.kind {
                VentKind::River => v.rate_m3s * dt,
                VentKind::Lake => (v.level_m - self.surface(i)).clamp(0.0, v.rate_m3s * dt),
            };
            if add > 0.0 {
                self.depth[i] += add;
                self.brought += f64::from(add);
            }
            // The vent's own column is as hot as the vent: a lake's stays molten as it crusts.
            if self.depth[i] > 0.0 {
                self.heat[i] = 1.0;
            }
        }
        let mut qx = std::mem::take(&mut self.qx);
        let mut qz = std::mem::take(&mut self.qz);
        let mut scale = std::mem::take(&mut self.scale);
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                qx[i] = if x + 1 < w && (self.depth[i] > 0.0 || self.depth[i + 1] > 0.0) {
                    self.face(i, i + 1)
                } else {
                    0.0
                };
                qz[i] = if z + 1 < h && (self.depth[i] > 0.0 || self.depth[i + w] > 0.0) {
                    self.face(i, i + w)
                } else {
                    0.0
                };
            }
        }
        // No column sends more than it holds.
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                let mut out = qx[i].max(0.0) + qz[i].max(0.0);
                if x > 0 {
                    out += (-qx[i - 1]).max(0.0);
                }
                if z > 0 {
                    out += (-qz[i - w]).max(0.0);
                }
                let out = out * dt;
                scale[i] = if out > self.depth[i] {
                    self.depth[i] / out
                } else {
                    1.0
                };
            }
        }
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                qx[i] *= if qx[i] > 0.0 {
                    scale[i]
                } else if x + 1 < w {
                    scale[i + 1]
                } else {
                    1.0
                };
                qz[i] *= if qz[i] > 0.0 {
                    scale[i]
                } else if z + 1 < h {
                    scale[i + w]
                } else {
                    1.0
                };
            }
        }
        // Depth and heat content, the heat carried upwind.
        let mut content: Vec<f32> = (0..w * h).map(|i| self.depth[i] * self.heat[i]).collect();
        let heat = &self.heat;
        let mut moved = |a: usize, b: usize, q: f32, depth: &mut [f32]| {
            let v = q * dt;
            let t = if v > 0.0 { heat[a] } else { heat[b] };
            depth[a] -= v;
            depth[b] += v;
            content[a] -= v * t;
            content[b] += v * t;
        };
        let mut depth = std::mem::take(&mut self.depth);
        for z in 0..h {
            for x in 0..w {
                let i = z * w + x;
                if qx[i] != 0.0 {
                    moved(i, i + 1, qx[i], &mut depth);
                }
                if qz[i] != 0.0 {
                    moved(i, i + w, qz[i], &mut depth);
                }
            }
        }
        for i in 0..w * h {
            if depth[i] <= FILM {
                // A film too thin to hold: it cools on the bed.
                self.ground[i] += depth[i].max(0.0);
                self.rocked += f64::from(depth[i].max(0.0));
                depth[i] = 0.0;
                self.heat[i] = 0.0;
            } else {
                self.heat[i] = (content[i] / depth[i]).clamp(0.0, 1.0);
            }
        }
        self.depth = depth;
        self.qx = qx;
        self.qz = qz;
        self.scale = scale;
        self.held.iter_mut().for_each(|h| *h = false);
        self.hold_lakes();
        // Quench, then cooling.
        let fl = self.fluid;
        for i in 0..w * h {
            if self.depth[i] <= 0.0 {
                continue;
            }
            let wet = std::iter::once(Some(i))
                .chain(self.neighbours(i))
                .flatten()
                .find(|&j| self.water[j]);
            if let Some(j) = wet {
                self.solidify(i, fl.quench.rock);
                self.events.push(LavaEvent::Steam {
                    x: j % w,
                    z: j / w,
                    water_m: fl.quench.steam_water_m,
                });
                continue;
            }
            if self.held[i] {
                continue;
            }
            let dry = self
                .neighbours(i)
                .map(|n| n.is_none_or(|j| self.depth[j] <= 0.0))
                .filter(|&d| d)
                .count() as f32;
            // From its top: its top's share of it, so deep lava keeps its heat longer.
            let thin = if fl.thin_depth_m > 0.0 {
                fl.thin_depth_m / self.depth[i]
            } else {
                1.0
            };
            let rate = (1.0 + fl.edge_cooling * dry) * thin / fl.cooling_seconds;
            self.heat[i] -= rate * dt;
            if self.heat[i] <= 0.0 {
                self.solidify(i, fl.rock);
            }
        }
    }

    /// A lake vent keeps its lake molten (it convects): every molten column joined to the vent
    /// through columns whose bed is below the lake's level is as hot as the vent and does not cool. A flow over the
    /// rim leaves the lake there and cools as any other.
    fn hold_lakes(&mut self) {
        for k in 0..self.vents.len() {
            let v = self.vents[k];
            if v.kind != VentKind::Lake {
                continue;
            }
            let start = self.at(v.x, v.z);
            if self.depth[start] <= 0.0 {
                continue;
            }
            let mut seen = std::mem::take(&mut self.held);
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(i) = stack.pop() {
                self.heat[i] = 1.0;
                for j in self.neighbours(i).flatten().collect::<Vec<_>>() {
                    if !seen[j] && self.depth[j] > 0.0 && self.ground[j] < v.level_m {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            self.held = seen;
        }
    }

    /// Runs `t` lava seconds in stable substeps (at most `maxSubsteps`; the rest is lost).
    /// Returns the lava seconds run.
    pub fn run(&mut self, t: f32) -> f32 {
        let mut left = t;
        let mut n = 0;
        while left > 1e-6 && n < self.rules.max_substeps {
            let dt = self.max_step().min(left);
            self.step(dt);
            left -= dt;
            n += 1;
        }
        t - left
    }

    /// A lava tick of `real_s` real seconds: run now when near a player, else owe it and run what
    /// is owed in one go once it reaches `farEverySeconds`. Returns the lava seconds run.
    pub fn advance(&mut self, real_s: f32, near: bool) -> f32 {
        self.owed_s += real_s * self.rules.speed;
        if near || self.owed_s >= self.rules.far_every_seconds {
            let t = std::mem::take(&mut self.owed_s);
            self.run(t)
        } else {
            0.0
        }
    }

    /// Whether a player (world points) is within `nearRadiusM` of the field on face `f`.
    pub fn is_near(&self, f: FaceFrame, players: &[[f64; 3]]) -> bool {
        let cx = f64::from(self.origin[0]) + self.w as f64 / 2.0;
        let cz = f64::from(self.origin[1]) + self.h as f64 / 2.0;
        let cy = self.ground.iter().map(|&g| f64::from(g)).sum::<f64>() / self.ground.len() as f64;
        let r = f64::from(self.rules.near_radius_m);
        let reach = r + (self.w.max(self.h) as f64) / 2.0;
        players.iter().any(|&p| {
            let [x, y, z] = f.point(p);
            let (dx, dy, dz) = (x - cx, y - cy, z - cz);
            dx * dx + dy * dy + dz * dz <= reach * reach
        })
    }

    /// Whether a player's feet (a world point on face `f`) stand in this field's lava.
    pub fn submerged(&self, f: FaceFrame, feet: [f64; 3]) -> bool {
        let [x, y, z] = f.point(feet);
        let cx = x.floor() as i64 - i64::from(self.origin[0]);
        let cz = z.floor() as i64 - i64::from(self.origin[1]);
        if cx < 0 || cz < 0 || cx as usize >= self.w || cz as usize >= self.h {
            return false;
        }
        let i = self.at(cx as usize, cz as usize);
        let y = y as f32;
        self.depth[i] > 0.0 && y >= self.ground[i] - 0.25 && y < self.surface(i)
    }

    /// The events since the last take.
    pub fn take_events(&mut self) -> Vec<LavaEvent> {
        std::mem::take(&mut self.events)
    }
}
