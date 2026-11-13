//! Far trees as multi-angle impostors: the bake (docs/proposals/TREE_IMPOSTORS.md phase I-1,
//! WORK_QUEUE item 31). A species tree grown by [`species::grow`] is ray cast on the CPU from a
//! fixed set of views (around the tree and from above), each view writing albedo, a lighting
//! normal and depth per texel into one region of an atlas, so a far tree can be drawn as one quad
//! that is lit by the sun and fog like the near trees and meets the ground by its depth.
//!
//! The layout, the resolution per renderer tier, the stages and variants baked and the colours are
//! data (`shared/trees/impostors/impostors.json`, [`Config`]); the engine is a fixed vocabulary: a
//! `ring` of views, exact voxel shapes (a leaf cell is its 1 m × ½ m × 1 m box, a log the regular
//! octagonal prism of its width, scaled to its cell across a horizontal axis). The leaves' chamfer
//! is not baked (sub-texel at the views' sizes). Deterministic: the same species, seed, stage and
//! texel size give the same bytes.
//!
//! Face-local: +y up, the trunk's foot at the origin, as the tree builder grows it; the drawer
//! stands the quad on a face through `FaceFrame` like any tree. No drawing here.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

use crate::species::{Part, Species, grow};

/// The bake's data, as checked in.
pub const IMPOSTORS_JSON: &str = include_str!("../../../shared/trees/impostors/impostors.json");
const PALETTE_JSON: &str = include_str!("../../../shared/textures/palette.json");
const LEAVES_JSON: &str = include_str!("../../../shared/look/leaves.json");

/// A cell's size (m): 1 m across, ½ m tall.
const CELL: [f32; 3] = [1.0, 0.5, 1.0];

/// The renderer tiers a view's resolution is given for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Desktop,
    Phone,
    Cpu,
}

/// The bake's data ([`IMPOSTORS_JSON`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The views' directions, toward the camera, unit length (face-local).
    pub views: Vec<[f32; 3]>,
    /// The ring's shape: azimuths, elevations (radians), whether a top view ends the list.
    pub azimuths: u32,
    pub elevations: Vec<f32>,
    pub top: bool,
    /// A view's side in texels: desktop, phone, cpu.
    pub view_px: [u32; 3],
    pub stages: Vec<u32>,
    pub variants: u32,
    /// Linear-ish sRGB albedo (0–1) of a leaf cell and a log.
    pub leaf: [f32; 3],
    pub log: [f32; 3],
    pub margin: f32,
    /// The hand-over per Far trees tier, by the tier's name (`far.tiers`).
    pub far: Vec<(String, Far)>,
    /// How far a leaf's normal leans out of the crown (shared/look/leaves.json `canopyBlend`).
    pub canopy_blend: f32,
    /// How a leaf texel is shaded at draw time (`leafShade`, BUGS.md #106): the hemisphere
    /// light's share (the near leaves' baked sky light and corner occlusion, which a texel has
    /// none of) and the albedo's scale (the painted leaf tile's mean over the palette colour).
    pub leaf_ambient: f32,
    pub leaf_albedo: f32,
    /// The crown's ambient occlusion baked into each texel (`ao`): how much a texel darkens by
    /// the share of the tree's cells about its hit (0 none: opt-in until the owner approves the
    /// look), and the neighbourhood's half-width in metres.
    pub ao_strength: f32,
    pub ao_radius: f32,
    /// Whether the atlas is drawn with a mip chain (`mips`, [`Impostor::mip`]): far impostors read
    /// the averaged texels instead of one nearest texel (opt-in until the owner approves the look).
    pub mips: bool,
}

impl Config {
    /// The checked-in data, parsed once (it is validated by the tests).
    pub fn get() -> &'static Config {
        static C: OnceLock<Config> = OnceLock::new();
        C.get_or_init(|| {
            Config::parse(IMPOSTORS_JSON).unwrap_or_else(|e| panic!("impostors.json: {e}"))
        })
    }

    /// Parses and checks `text`; a bad entry fails naming its field.
    pub fn parse(text: &str) -> Result<Config, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let obj = v.as_object().ok_or("must be an object")?;
        for k in obj.keys() {
            if !matches!(
                k.as_str(),
                "$comment"
                    | "views"
                    | "viewPx"
                    | "stages"
                    | "variants"
                    | "albedo"
                    | "marginM"
                    | "leafShade"
                    | "ao"
                    | "mips"
                    | "far"
            ) {
                return Err(format!("unknown field `{k}`"));
            }
        }
        let field = |k: &str| v.get(k).ok_or(format!("`{k}` is missing"));
        let num = |v: &Value, at: &str, lo: f64, hi: f64| -> Result<f64, String> {
            v.as_f64()
                .filter(|x| (lo..=hi).contains(x))
                .ok_or(format!("`{at}` must be a number in [{lo}, {hi}]"))
        };
        let views = field("views")?;
        if views.get("kind").and_then(Value::as_str) != Some("ring") {
            return Err("`views.kind` must be \"ring\"".into());
        }
        let azimuths = num(
            views.get("azimuths").unwrap_or(&Value::Null),
            "views.azimuths",
            1.0,
            64.0,
        )? as u32;
        let elev = views
            .get("elevationsDeg")
            .and_then(Value::as_array)
            .ok_or("`views.elevationsDeg` must be a list")?;
        let mut elevations = Vec::new();
        for (i, e) in elev.iter().enumerate() {
            let d = num(e, &format!("views.elevationsDeg[{i}]"), -89.0, 89.0)?;
            if elevations
                .last()
                .is_some_and(|&p: &f32| d as f32 <= p.to_degrees())
            {
                return Err("`views.elevationsDeg` must rise".into());
            }
            elevations.push((d as f32).to_radians());
        }
        let top = views
            .get("top")
            .and_then(Value::as_bool)
            .ok_or("`views.top` must be true or false")?;
        if elevations.is_empty() && !top {
            return Err("`views` holds no view".into());
        }
        let px = field("viewPx")?;
        let mut view_px = [0u32; 3];
        for (i, t) in ["desktop", "phone", "cpu"].iter().enumerate() {
            let n = num(
                px.get(*t).unwrap_or(&Value::Null),
                &format!("viewPx.{t}"),
                4.0,
                512.0,
            )? as u32;
            view_px[i] = n;
        }
        let stages = field("stages")?
            .as_array()
            .ok_or("`stages` must be a list")?
            .iter()
            .enumerate()
            .map(|(i, s)| num(s, &format!("stages[{i}]"), 0.0, 64.0).map(|s| s as u32))
            .collect::<Result<Vec<_>, _>>()?;
        if stages.is_empty() {
            return Err("`stages` is empty".into());
        }
        let variants = num(field("variants")?, "variants", 1.0, 64.0)? as u32;
        let albedo = field("albedo")?;
        let colour = |k: &str| -> Result<[f32; 3], String> {
            let name = albedo
                .get(k)
                .and_then(Value::as_str)
                .ok_or(format!("`albedo.{k}` must name a palette colour"))?;
            palette(name).ok_or(format!("`albedo.{k}`: no palette colour `{name}`"))
        };
        let (leaf, log) = (colour("leaf")?, colour("log")?);
        let margin = num(field("marginM")?, "marginM", 0.0, 4.0)? as f32;
        let far = parse_far(field("far")?)?;
        let shade = field("leafShade")?;
        for k in shade
            .as_object()
            .ok_or("`leafShade` must be an object")?
            .keys()
        {
            if !matches!(k.as_str(), "$comment" | "ambient" | "albedo") {
                return Err(format!("unknown field `leafShade.{k}`"));
            }
        }
        let leaf_ambient = num(
            shade.get("ambient").unwrap_or(&Value::Null),
            "leafShade.ambient",
            0.0,
            1.0,
        )? as f32;
        let leaf_albedo = num(
            shade.get("albedo").unwrap_or(&Value::Null),
            "leafShade.albedo",
            0.0,
            2.0,
        )? as f32;
        let (ao_strength, ao_radius) = match v.get("ao") {
            None => (0.0, 1.0),
            Some(ao) => {
                for k in ao.as_object().ok_or("`ao` must be an object")?.keys() {
                    if !matches!(k.as_str(), "$comment" | "strength" | "radiusM") {
                        return Err(format!("unknown field `ao.{k}`"));
                    }
                }
                (
                    num(
                        ao.get("strength").unwrap_or(&Value::Null),
                        "ao.strength",
                        0.0,
                        1.0,
                    )? as f32,
                    num(
                        ao.get("radiusM").unwrap_or(&Value::Null),
                        "ao.radiusM",
                        0.5,
                        4.0,
                    )? as f32,
                )
            }
        };
        let mips = match v.get("mips") {
            None => false,
            Some(m) => m.as_bool().ok_or("`mips` must be true or false")?,
        };
        let mut out = Config {
            views: Vec::new(),
            azimuths,
            elevations,
            top,
            view_px,
            stages,
            variants,
            leaf,
            log,
            margin,
            far,
            canopy_blend: canopy_blend(),
            leaf_ambient,
            leaf_albedo,
            ao_strength,
            ao_radius,
            mips,
        };
        out.views = out.layout();
        Ok(out)
    }

    /// The view directions in atlas order: each elevation's ring (azimuth 0 along +x, turning
    /// toward +z), then the top.
    fn layout(&self) -> Vec<[f32; 3]> {
        let mut v = Vec::new();
        for &e in &self.elevations {
            for k in 0..self.azimuths {
                let a = k as f32 * std::f32::consts::TAU / self.azimuths as f32;
                v.push([e.cos() * a.cos(), e.sin(), e.cos() * a.sin()]);
            }
        }
        if self.top {
            v.push([0.0, 1.0, 0.0]);
        }
        v
    }

    /// A view's side in texels for `tier`.
    pub fn px(&self, tier: Tier) -> u32 {
        self.view_px[tier as usize]
    }

    /// The hand-over of the Far trees tier named `tier` (`None`: no such tier).
    pub fn far(&self, tier: &str) -> Option<Far> {
        self.far.iter().find(|(n, _)| n == tier).map(|(_, f)| *f)
    }

    /// The one view a pixel draws when it dithers between [`Config::pick`]'s views: the view
    /// whose share of the cumulative weights holds `threshold` (0–1, the screen's bayer value),
    /// so across the screen each view shows on its weight's share of the pixels.
    pub fn dither(&self, dir: [f32; 3], threshold: f32) -> usize {
        let picks = self.pick(dir);
        let mut acc = 0.0;
        for &(i, w) in &picks {
            acc += w;
            if threshold < acc {
                return i;
            }
        }
        picks.last().map_or(0, |p| p.0)
    }

    /// Which baked tree a tree of `seed` at growth `stage` draws: (stage slot, variant), the
    /// nearest baked stage (the quad scaled to the tree's own size; a tie takes the lower) and
    /// `hash(seed) mod variants`.
    pub fn baked_for(&self, seed: u32, stage: u32) -> (usize, u32) {
        let slot = (0..self.stages.len())
            .min_by_key(|&i| (self.stages[i].abs_diff(stage), self.stages[i]))
            .unwrap_or(0);
        let h = seed.wrapping_mul(0x9E37_79B9).rotate_left(13) ^ seed;
        (slot, h % self.variants)
    }

    /// The views a camera looking from direction `dir` (toward the camera, face-local) blends,
    /// with weights summing to 1: the two azimuths either side in the two elevation bands either
    /// side (the top standing for every azimuth above the last band). Up to four.
    pub fn pick(&self, dir: [f32; 3]) -> Vec<(usize, f32)> {
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
            .sqrt()
            .max(1e-9);
        let d = dir.map(|c| c / len);
        let elev = d[1].clamp(-1.0, 1.0).asin();
        let az = d[2].atan2(d[0]).rem_euclid(std::f32::consts::TAU);
        let n = self.azimuths as usize;
        let step = std::f32::consts::TAU / n as f32;
        let fa = az / step;
        let (a0, ta) = (fa.floor() as usize % n, fa - fa.floor());
        let a1 = (a0 + 1) % n;
        // The bands: each elevation, then the top at 90°.
        let mut bands: Vec<(f32, Option<usize>)> = (0..self.elevations.len())
            .map(|b| (self.elevations[b], Some(b)))
            .collect();
        if self.top {
            bands.push((std::f32::consts::FRAC_PI_2, None));
        }
        let ring = |b: Option<usize>, a: usize| match b {
            Some(b) => b * n + a,
            None => self.elevations.len() * n,
        };
        let mut out: Vec<(usize, f32)> = Vec::new();
        let mut add = |i: usize, w: f32| {
            if w <= 1e-6 {
                return;
            }
            match out.iter_mut().find(|(j, _)| *j == i) {
                Some(e) => e.1 += w,
                None => out.push((i, w)),
            }
        };
        let band_pair = if elev <= bands[0].0 {
            (0, 0, 0.0)
        } else if elev >= bands[bands.len() - 1].0 {
            (bands.len() - 1, bands.len() - 1, 0.0)
        } else {
            let b = bands.iter().rposition(|&(e, _)| e <= elev).unwrap_or(0);
            let t = (elev - bands[b].0) / (bands[b + 1].0 - bands[b].0);
            (b, b + 1, t)
        };
        let (b0, b1, tb) = band_pair;
        for (b, wb) in [(b0, 1.0 - tb), (b1, tb)] {
            add(ring(bands[b].1, a0), wb * (1.0 - ta));
            add(ring(bands[b].1, a1), wb * ta);
        }
        out
    }
}

/// Where far trees become impostors for one Far trees tier (`far.tiers`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Far {
    /// Whether impostors draw at all (`draw: "impostor"`; `"none"` draws no far trees).
    pub impostor: bool,
    pub from: f32,
    pub fade: f32,
    /// Where far trees stop (`f32::INFINITY` for never).
    pub to: f32,
}

impl Far {
    /// The impostor's share of a tree `dist` m from the eye: 0 nearer than `from` (the real
    /// tree), rising linearly across `fade` to 1, and 0 again beyond `to` (nothing drawn).
    pub fn share(&self, dist: f32) -> f32 {
        if !self.impostor || dist > self.to {
            return 0.0;
        }
        if self.fade <= 0.0 {
            return if dist >= self.from { 1.0 } else { 0.0 };
        }
        ((dist - self.from) / self.fade).clamp(0.0, 1.0)
    }

    /// The near tree's share at `dist`: what the impostor leaves, so each pixel of the
    /// crossfade shows exactly one of them (the shares sum to 1 inside `to`).
    pub fn near_share(&self, dist: f32) -> f32 {
        if !self.impostor {
            return 1.0;
        }
        if dist > self.to {
            return 0.0;
        }
        1.0 - self.share(dist)
    }
}

fn parse_far(v: &Value) -> Result<Vec<(String, Far)>, String> {
    let tiers = v
        .get("tiers")
        .and_then(Value::as_object)
        .ok_or("`far.tiers` must be an object")?;
    let mut out = Vec::new();
    for (name, t) in tiers {
        if name.starts_with('$') {
            continue;
        }
        let at = |k: &str| format!("far.tiers.{name}.{k}");
        let bad_draw = || format!("`{}` must be \"impostor\" or \"none\"", at("draw"));
        let far = match t.get("draw").and_then(Value::as_str) {
            Some("none") => Far {
                impostor: false,
                from: 0.0,
                fade: 0.0,
                to: 0.0,
            },
            Some("impostor") => {
                let num = |k: &str| -> Result<f32, String> {
                    t.get(k)
                        .and_then(Value::as_f64)
                        .filter(|x| (0.0..=100_000.0).contains(x))
                        .map(|x| x as f32)
                        .ok_or(format!("`{}` must be a number in [0, 100000]", at(k)))
                };
                let (from, fade, to) = (num("fromM")?, num("fadeM")?, num("toM")?);
                if to != 0.0 && to < from + fade {
                    return Err(format!("`{}` must be 0 or past fromM + fadeM", at("toM")));
                }
                Far {
                    impostor: true,
                    from,
                    fade,
                    to: if to == 0.0 { f32::INFINITY } else { to },
                }
            }
            _ => return Err(bad_draw()),
        };
        out.push((name.clone(), far));
    }
    if out.is_empty() {
        return Err("`far.tiers` holds no tier".into());
    }
    Ok(out)
}

/// The face-local direction from a tree's foot at `tree` toward the eye at `eye` (both world
/// positions on `face`): what [`Config::pick`] takes. Crosses the face only through `FaceFrame`.
pub fn view_dir(face: dwell_world::FaceFrame, tree: [f64; 3], eye: [f64; 3]) -> [f32; 3] {
    let (t, e) = (face.point(tree), face.point(eye));
    [
        (e[0] - t[0]) as f32,
        (e[1] - t[1]) as f32,
        (e[2] - t[2]) as f32,
    ]
}

/// A palette colour by `ramp.stop`, a bare ramp's base or a swatch, as 0–1 sRGB.
fn palette(name: &str) -> Option<[f32; 3]> {
    let v: Value = serde_json::from_str(PALETTE_JSON).ok()?;
    let (ramp, stop) = name.split_once('.').unwrap_or((name, "base"));
    let hex = v
        .get("ramps")
        .and_then(|r| r.get(ramp))
        .and_then(|r| r.get(stop))
        .or_else(|| v.get("swatches").and_then(|s| s.get(name)))?
        .as_str()?;
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let c = |i: usize| {
        u8::from_str_radix(&hex[i..i + 2], 16)
            .ok()
            .map(|b| b as f32 / 255.0)
    };
    Some([c(0)?, c(2)?, c(4)?])
}

fn canopy_blend() -> f32 {
    serde_json::from_str::<Value>(LEAVES_JSON)
        .ok()
        .and_then(|v| v.get("canopyBlend").and_then(Value::as_f64))
        .unwrap_or(0.0) as f32
}

/// One tree's impostor: every view of the layout at `px` texels a side, in an atlas `cols` views
/// across (row-major), each texel albedo (RGB, alpha the coverage) and normal (RGB, face-local
/// −1..1 as 0..255) with depth in its alpha (0 nearest the camera, 255 farthest, over the bounding
/// sphere's diameter; 255 where the view is empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Impostor {
    pub px: u32,
    pub cols: u32,
    pub rows: u32,
    /// The bounding sphere (face-local m): the quad's centre and half-size in every view.
    pub centre: [f32; 3],
    pub radius: f32,
    pub views: Vec<[f32; 3]>,
    pub albedo: Vec<[u8; 4]>,
    pub normal_depth: Vec<[u8; 4]>,
}

impl Impostor {
    /// The mip levels a view's square takes down to one texel (1 when `px` isn't a power of two:
    /// a view's square must halve exactly, or its levels would bleed into the next view's).
    pub fn mip_levels(px: u32) -> u32 {
        if px.is_power_of_two() {
            px.trailing_zeros() + 1
        } else {
            1
        }
    }

    /// Mip `level` of the atlas (0 itself): each view's square halved `level` times on its own
    /// (a texel never takes its neighbour view's), as (albedo, normal + depth) at `size() >> level`.
    /// A texel is covered when half or more of the base texels under it are, with the commonest
    /// covered colour (so a leaf texel stays the palette's leaf the draw tells by colour), the
    /// covered texels' mean occlusion, normal (renormalized) and depth: the far crowns keep their
    /// coverage instead of thinning to nearest-texel noise (WORK_QUEUE item 31).
    pub fn mip(&self, level: u32) -> (Vec<[u8; 4]>, Vec<[u8; 4]>) {
        let (w, h) = self.size();
        let step = 1u32 << level.min(31);
        let (lw, lh) = ((w / step).max(1), (h / step).max(1));
        let mut albedo = vec![[0u8; 4]; (lw * lh) as usize];
        let mut nd = vec![[128u8, 128, 128, 255]; (lw * lh) as usize];
        for y in 0..lh {
            for x in 0..lw {
                let mut colours: Vec<([u8; 3], u32)> = Vec::new();
                let (mut ao, mut n, mut depth, mut covered) = (0.0f32, [0.0f32; 3], 0.0f32, 0u32);
                for sy in 0..step {
                    for sx in 0..step {
                        let (bx, by) = (x * step + sx, y * step + sy);
                        if bx >= w || by >= h {
                            continue;
                        }
                        let i = (by * w + bx) as usize;
                        let a = self.albedo[i];
                        if a[3] < 128 {
                            continue;
                        }
                        covered += 1;
                        let rgb = [a[0], a[1], a[2]];
                        match colours.iter_mut().find(|(c, _)| *c == rgb) {
                            Some(e) => e.1 += 1,
                            None => colours.push((rgb, 1)),
                        }
                        ao += texel_ao(a[3]);
                        let t = self.normal_depth[i];
                        for k in 0..3 {
                            n[k] += f32::from(t[k]) / 255.0 * 2.0 - 1.0;
                        }
                        depth += f32::from(t[3]);
                    }
                }
                if covered * 2 < step * step {
                    continue;
                }
                let o = (y * lw + x) as usize;
                let rgb = colours
                    .iter()
                    .max_by_key(|(_, k)| *k)
                    .map_or([0; 3], |c| c.0);
                albedo[o] = [rgb[0], rgb[1], rgb[2], ao_alpha(ao / covered as f32)];
                let n = norm(n);
                let q = |c: f32| ((c * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                nd[o] = [
                    q(n[0]),
                    q(n[1]),
                    q(n[2]),
                    (depth / covered as f32).round() as u8,
                ];
            }
        }
        (albedo, nd)
    }

    /// The atlas's size in texels.
    pub fn size(&self) -> (u32, u32) {
        (self.cols * self.px, self.rows * self.px)
    }

    /// Texel (`i`, `j`) of view `v` (j = 0 at the view's top row).
    pub fn at(&self, v: usize, i: u32, j: u32) -> usize {
        let (cx, cy) = (v as u32 % self.cols, v as u32 / self.cols);
        let (w, _) = self.size();
        ((cy * self.px + j) * w + cx * self.px + i) as usize
    }

    /// The bytes on the GPU: two RGBA8 textures.
    pub fn bytes(&self) -> usize {
        (self.albedo.len() + self.normal_depth.len()) * 4
    }
}

/// A view's screen basis: right and up, for a camera at direction `v` looking back at the tree.
fn basis(v: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let h = (v[0] * v[0] + v[2] * v[2]).sqrt();
    let right = if h > 1e-6 {
        [v[2] / h, 0.0, -v[0] / h]
    } else {
        [0.0, 0.0, -1.0]
    };
    (right, cross(v, right))
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    a.map(|c| c / l)
}

/// A cell's shape as slabs about its centre: `|n · p| ≤ d` for each (n, d).
fn slabs(part: Part) -> Vec<([f32; 3], f32)> {
    let h = CELL.map(|c| c * 0.5);
    match part {
        Part::Leaf => vec![
            ([1.0, 0.0, 0.0], h[0]),
            ([0.0, 1.0, 0.0], h[1]),
            ([0.0, 0.0, 1.0], h[2]),
        ],
        Part::Log(axis, quarters) => {
            // The builder's axes: 0 along y (standing), 1 along x, 2 along z.
            let axis = [1, 0, 2][usize::from(axis.min(2))];
            let w = f32::from(quarters.clamp(1, 4)) / 4.0;
            // The two axes across the log, each half-extent the width's share of the cell.
            let across: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
            let (u, v) = (across[0], across[1]);
            let (hu, hv) = (h[u] * w, h[v] * w);
            let e = |a: usize, s: f32| {
                let mut n = [0.0; 3];
                n[a] = s;
                n
            };
            let diag = |su: f32| {
                let mut n = [0.0; 3];
                n[u] = su / hu;
                n[v] = 1.0 / hv;
                (n, std::f32::consts::SQRT_2)
            };
            vec![
                (e(axis, 1.0), h[axis]),
                (e(u, 1.0 / hu), 1.0),
                (e(v, 1.0 / hv), 1.0),
                diag(1.0),
                diag(-1.0),
            ]
        }
    }
}

/// The ray `o + t·d` against convex slabs about `c`: the entry t and its outward normal.
fn hit(
    o: [f32; 3],
    d: [f32; 3],
    c: [f32; 3],
    slabs: &[([f32; 3], f32)],
) -> Option<(f32, [f32; 3])> {
    let p = [o[0] - c[0], o[1] - c[1], o[2] - c[2]];
    let (mut t0, mut t1, mut n0) = (f32::MIN, f32::MAX, [0.0; 3]);
    for &(n, lim) in slabs {
        let (pn, dn) = (dot(p, n), dot(d, n));
        if dn.abs() < 1e-9 {
            if pn.abs() > lim {
                return None;
            }
            continue;
        }
        let (a, b) = ((-lim - pn) / dn, (lim - pn) / dn);
        let (near, far, sign) = if a < b { (a, b, -1.0) } else { (b, a, 1.0) };
        // Entering through the face the ray meets first: −n for the −lim plane.
        if near > t0 {
            t0 = near;
            n0 = n.map(|c| c * sign);
        }
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    (t1 >= 0.0).then_some((t0.max(0.0), norm(n0)))
}

/// Bakes the impostor of `species` grown from `seed` to `stage`, every view of `config` at
/// `px` texels a side.
pub fn bake(species: &Species, seed: u32, stage: u32, config: &Config, px: u32) -> Impostor {
    bake_cells(&grow(species, seed, stage).cells, config, px)
}

/// Bakes the impostor of a grown tree's `cells` (the builder's [`crate::species::Grown::cells`]).
pub fn bake_cells(
    cells: &std::collections::BTreeMap<[i32; 3], Part>,
    config: &Config,
    px: u32,
) -> Impostor {
    let n = config.views.len() as u32;
    let cols = (n as f32).sqrt().ceil().max(1.0) as u32;
    let rows = n.div_ceil(cols);
    let (w, hgt) = (cols * px, rows * px);
    let mut out = Impostor {
        px,
        cols,
        rows,
        centre: [0.5, 0.0, 0.5],
        radius: 0.0,
        views: config.views.clone(),
        albedo: vec![[0; 4]; (w * hgt) as usize],
        normal_depth: vec![[128, 128, 128, 255]; (w * hgt) as usize],
    };
    if cells.is_empty() {
        return out;
    }
    // Bounds: the cells' box (m) and the bounding sphere about the trunk's axis.
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    for k in cells.keys() {
        for a in 0..3 {
            lo[a] = lo[a].min(k[a]);
            hi[a] = hi[a].max(k[a]);
        }
    }
    let (y0, y1) = (lo[1] as f32 * CELL[1], (hi[1] + 1) as f32 * CELL[1]);
    let centre = [0.5, (y0 + y1) * 0.5, 0.5];
    let mut r2: f32 = 0.0;
    for k in cells.keys() {
        for corner in 0..8 {
            let p = [
                (k[0] + (corner & 1)) as f32 * CELL[0] - centre[0],
                (k[1] + ((corner >> 1) & 1)) as f32 * CELL[1] - centre[1],
                (k[2] + ((corner >> 2) & 1)) as f32 * CELL[2] - centre[2],
            ];
            r2 = r2.max(dot(p, p));
        }
    }
    let radius = r2.sqrt() + config.margin;
    out.centre = centre;
    out.radius = radius;
    // The crown's centroid: where a leaf's normal leans away from.
    let leaves: Vec<[f32; 3]> = cells
        .iter()
        .filter(|(_, p)| matches!(p, Part::Leaf))
        .map(|(k, _)| cell_centre(*k))
        .collect();
    let crown = if leaves.is_empty() {
        centre
    } else {
        let s = leaves
            .iter()
            .fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
        s.map(|c| c / leaves.len() as f32)
    };
    let grid: HashMap<[i32; 3], Part> = cells.iter().map(|(k, p)| (*k, *p)).collect();
    let shapes: HashMap<Part, Vec<([f32; 3], f32)>> = cells
        .values()
        .map(|p| (*p, slabs(*p)))
        .collect::<HashMap<_, _>>();
    for (vi, &v) in config.views.iter().enumerate() {
        let (right, up) = basis(v);
        let d = v.map(|c| -c);
        for j in 0..px {
            for i in 0..px {
                let sx = ((i as f32 + 0.5) / px as f32 * 2.0 - 1.0) * radius;
                let sy = (1.0 - (j as f32 + 0.5) / px as f32 * 2.0) * radius;
                let o: [f32; 3] =
                    std::array::from_fn(|a| centre[a] + right[a] * sx + up[a] * sy + v[a] * radius);
                let Some((t, part, n)) = march(o, d, lo, hi, &grid, &shapes) else {
                    continue;
                };
                let p: [f32; 3] = std::array::from_fn(|a| o[a] + d[a] * t);
                let (albedo, normal) = match part {
                    Part::Leaf => {
                        let out = norm(std::array::from_fn(|a| p[a] - crown[a]));
                        let b = config.canopy_blend;
                        (
                            config.leaf,
                            norm(std::array::from_fn(|a| n[a] * (1.0 - b) + out[a] * b)),
                        )
                    }
                    Part::Log(..) => (config.log, n),
                };
                let at = out.at(vi, i, j);
                let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                let ao = occlusion(&grid, p, config);
                out.albedo[at] = [q(albedo[0]), q(albedo[1]), q(albedo[2]), ao_alpha(ao)];
                let depth = t / (2.0 * radius);
                out.normal_depth[at] = [
                    q(normal[0] * 0.5 + 0.5),
                    q(normal[1] * 0.5 + 0.5),
                    q(normal[2] * 0.5 + 0.5),
                    q(depth),
                ];
            }
        }
    }
    out
}

/// A covered texel's albedo alpha for its ambient occlusion `ao` (1 open, 0 dark): 128–255, so
/// coverage stays alpha ≥ ½ and the draw reads the occlusion back as (alpha − ½) × 2
/// ([`texel_ao`]); an uncovered texel's alpha is 0.
pub fn ao_alpha(ao: f32) -> u8 {
    128 + (ao.clamp(0.0, 1.0) * 127.0).round() as u8
}

/// The ambient occlusion an albedo alpha holds ([`ao_alpha`]; 1 for an uncovered texel).
pub fn texel_ao(alpha: u8) -> f32 {
    if alpha < 128 {
        1.0
    } else {
        f32::from(alpha - 128) / 127.0
    }
}

/// The baked occlusion at a hit point `p` (m, the tree's frame): 1 − strength × the share of the
/// cells within `ao.radiusM` of it the tree fills (its own cell left out), so a texel deep in the
/// crown darkens as the near leaves' corner occlusion and sky light do (WORK_QUEUE item 31). 1
/// when `ao.strength` is 0.
fn occlusion(grid: &HashMap<[i32; 3], Part>, p: [f32; 3], config: &Config) -> f32 {
    if config.ao_strength <= 0.0 {
        return 1.0;
    }
    let r = config.ao_radius;
    let c: [i32; 3] = std::array::from_fn(|a| (p[a] / CELL[a]).floor() as i32);
    let reach: [i32; 3] = std::array::from_fn(|a| (r / CELL[a]).ceil() as i32);
    let (mut filled, mut total) = (0u32, 0u32);
    for dy in -reach[1]..=reach[1] {
        for dz in -reach[2]..=reach[2] {
            for dx in -reach[0]..=reach[0] {
                if (dx, dy, dz) == (0, 0, 0) {
                    continue;
                }
                let k = [c[0] + dx, c[1] + dy, c[2] + dz];
                let q = cell_centre(k);
                let d: [f32; 3] = std::array::from_fn(|a| q[a] - p[a]);
                if dot(d, d) > r * r {
                    continue;
                }
                total += 1;
                filled += u32::from(grid.contains_key(&k));
            }
        }
    }
    if total == 0 {
        return 1.0;
    }
    1.0 - config.ao_strength * filled as f32 / total as f32
}

fn cell_centre(k: [i32; 3]) -> [f32; 3] {
    std::array::from_fn(|a| (k[a] as f32 + 0.5) * CELL[a])
}

/// Walks the ray through the cells' grid (Amanatides–Woo over 1 m × ½ m × 1 m cells) and returns
/// the first shape it enters: t, the part, the outward normal.
fn march(
    o: [f32; 3],
    d: [f32; 3],
    lo: [i32; 3],
    hi: [i32; 3],
    grid: &HashMap<[i32; 3], Part>,
    shapes: &HashMap<Part, Vec<([f32; 3], f32)>>,
) -> Option<(f32, Part, [f32; 3])> {
    // Clip to the cells' box.
    let (mut t0, mut t1) = (0.0f32, f32::MAX);
    for a in 0..3 {
        let (bmin, bmax) = (lo[a] as f32 * CELL[a], (hi[a] + 1) as f32 * CELL[a]);
        if d[a].abs() < 1e-9 {
            if o[a] < bmin || o[a] > bmax {
                return None;
            }
            continue;
        }
        let (ta, tb) = ((bmin - o[a]) / d[a], (bmax - o[a]) / d[a]);
        t0 = t0.max(ta.min(tb));
        t1 = t1.min(ta.max(tb));
    }
    if t0 > t1 {
        return None;
    }
    let p: [f32; 3] = std::array::from_fn(|a| o[a] + d[a] * (t0 + 1e-5));
    let mut c: [i32; 3] =
        std::array::from_fn(|a| ((p[a] / CELL[a]).floor() as i32).clamp(lo[a], hi[a]));
    let step: [i32; 3] = std::array::from_fn(|a| if d[a] >= 0.0 { 1 } else { -1 });
    let delta: [f32; 3] = std::array::from_fn(|a| {
        if d[a].abs() < 1e-9 {
            f32::MAX
        } else {
            CELL[a] / d[a].abs()
        }
    });
    let mut next: [f32; 3] = std::array::from_fn(|a| {
        if d[a].abs() < 1e-9 {
            f32::MAX
        } else {
            let edge = (c[a] + i32::from(d[a] >= 0.0)) as f32 * CELL[a];
            (edge - o[a]) / d[a]
        }
    });
    loop {
        if let Some(part) = grid.get(&c)
            && let Some((t, n)) = hit(o, d, cell_centre(c), &shapes[part])
        {
            return Some((t, *part, n));
        }
        let a = (0..3)
            .min_by(|&x, &y| next[x].total_cmp(&next[y]))
            .unwrap_or(0);
        if next[a] > t1 {
            return None;
        }
        c[a] += step[a];
        if c[a] < lo[a] || c[a] > hi[a] {
            return None;
        }
        next[a] += delta[a];
    }
}

/// The seed variant `v` of every species and stage is baked from: a fixed spread, so the variants
/// differ (a tree's own seed only picks one, [`Config::baked_for`]).
pub fn variant_seed(v: u32) -> u32 {
    v.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15)
}

/// A species tree in a far tree list (proposal I-2): what an impostor instance is built from. Its
/// anchor is face-local (the voxel above the ground it stands on); `species` indexes
/// [`crate::species::SPECIES`]; `seed` and `stage` are the placement's (ADR 0079), so the near
/// tree and its impostor are the same tree. `stem` is its stem's bottom cell (face-local) and
/// `stem_cells` the solid cells standing straight up from it (`Planter::stem`): a client that holds
/// one of those cells as air sees the tree felled and draws no impostor for it ([`standing`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FarTree {
    pub anchor: [i32; 3],
    pub species: u16,
    pub seed: u32,
    pub stage: u32,
    pub stem: [i32; 3],
    pub stem_cells: u8,
}

/// Whether a far tree still stands, as a client sees it (WORK_QUEUE item 31: felled trees left
/// out): `held` answers a face-local cell's state where the client holds its chunk (`None` where
/// it doesn't). `Some(false)`: felled, a stem cell it holds is air (a cut takes the trunk above it
/// away as a piece, docs/FELLING.md, leaving air over the stump); `Some(true)`: standing, seen (a
/// stem cell held, none air); `None`: none of its stem held (far away the generator's tree is all
/// there is).
pub fn stem_seen(
    tree: &FarTree,
    held: impl Fn([i32; 3]) -> Option<dwell_blocks::StateId>,
) -> Option<bool> {
    let air = dwell_world::M.air;
    let [x, y, z] = tree.stem;
    let mut seen = None;
    for h in 0..i32::from(tree.stem_cells) {
        match held([x, y + h, z]) {
            Some(m) if m == air => return Some(false),
            Some(_) => seen = Some(true),
            None => {}
        }
    }
    seen
}

/// [`stem_seen`] short of felled: the tree is drawn.
pub fn standing(tree: &FarTree, held: impl Fn([i32; 3]) -> Option<dwell_blocks::StateId>) -> bool {
    stem_seen(tree, held) != Some(false)
}

/// One impostor to draw (proposal §6, 16 bytes on the GPU): the face-local foot of the tree
/// (the anchor's column centre at the anchor's floor), the baked tree's array layer, and its turn
/// in quarter turns from its seed (so 4 variants × 4 turns read as 16 trees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub foot: [f32; 3],
    pub layer: u32,
    pub quarter_turns: u32,
}

impl Config {
    /// How many baked trees a species holds (its array layers): stages × variants.
    pub fn layers_per_species(&self) -> u32 {
        self.stages.len() as u32 * self.variants
    }

    /// The array layer of a tree of `species` (index), `seed` and growth `stage`: the species'
    /// block of layers, then the nearest baked stage, then the seed's variant
    /// ([`Config::baked_for`]).
    pub fn layer(&self, species: u16, seed: u32, stage: u32) -> u32 {
        let (slot, variant) = self.baked_for(seed, stage);
        u32::from(species) * self.layers_per_species() + slot as u32 * self.variants + variant
    }

    /// How many baked trees the atlas holds: every species' [`Config::layers_per_species`].
    pub fn layer_count(&self) -> u32 {
        crate::species::SPECIES.len() as u32 * self.layers_per_species()
    }

    /// What array layer `layer` holds: (species index, the growth stage baked, the seed baked),
    /// the inverse of [`Config::layer`]'s blocks (the stage clamped to the species' last; variant
    /// `v` baked from seed `v`'s [`variant_seed`]). `None` past [`Config::layer_count`].
    pub fn layer_tree(&self, layer: u32) -> Option<(u16, u32, u32)> {
        let per = self.layers_per_species();
        let species = crate::species::SPECIES.get((layer / per.max(1)) as usize)?;
        let within = layer % per;
        let (slot, variant) = (within / self.variants, within % self.variants);
        let stage = self.stages[slot as usize].min(species.stages.saturating_sub(1));
        Some(((layer / per) as u16, stage, variant_seed(variant)))
    }

    /// Bakes array layer `layer` at `px` texels a view (the drawer's bake job; `None` past the
    /// layers).
    pub fn bake_layer(&self, layer: u32, px: u32) -> Option<Impostor> {
        let (species, stage, seed) = self.layer_tree(layer)?;
        let sp = &crate::species::SPECIES[species as usize];
        Some(bake(sp, seed, stage, self, px))
    }

    /// The instances of `trees` (a section's list), in the list's order.
    pub fn instances(&self, trees: &[FarTree]) -> Vec<Instance> {
        trees
            .iter()
            .map(|t| Instance {
                foot: [
                    t.anchor[0] as f32 + 0.5,
                    t.anchor[1] as f32 * dwell_world::CELL_H as f32,
                    t.anchor[2] as f32 + 0.5,
                ],
                layer: self.layer(t.species, t.seed, t.stage),
                quarter_turns: (t.seed >> 7).wrapping_mul(0x2545_F491) >> 30,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // face-a-only: the bake is face-local (+y up, no world position); the drawer crosses faces.
    use super::*;
    use crate::species::{SPECIES, find, species_gen_fixtures};

    fn small() -> Config {
        let mut c = Config::get().clone();
        c.view_px = [32, 16, 8];
        c
    }

    #[test]
    fn every_layer_names_the_tree_its_instances_draw() {
        // face-a-only: data only.
        let c = small();
        assert_eq!(
            c.layer_count(),
            SPECIES.len() as u32 * c.layers_per_species()
        );
        for species in 0..SPECIES.len() as u16 {
            for seed in [1u32, 77, 4096, 123_456] {
                for stage in 0..8 {
                    let layer = c.layer(species, seed, stage);
                    let (s, baked_stage, _) = c.layer_tree(layer).expect("in range");
                    assert_eq!(s, species);
                    let (slot, _) = c.baked_for(seed, stage);
                    let last = SPECIES[species as usize].stages.saturating_sub(1);
                    assert_eq!(baked_stage, c.stages[slot].min(last));
                }
            }
        }
        assert_eq!(c.layer_tree(c.layer_count()), None);
        let one = c.bake_layer(0, 8).expect("layer 0");
        assert_eq!(one.px, 8);
        assert!(
            one.albedo.iter().any(|t| t[3] >= 128),
            "a tree, not an empty atlas"
        );
    }

    #[test]
    fn the_data_parses_and_lays_out_the_ring_and_top() {
        // face-a-only: data only.
        let c = Config::get();
        assert_eq!(
            c.views.len(),
            (c.azimuths as usize) * c.elevations.len() + usize::from(c.top)
        );
        for v in &c.views {
            assert!((dot(*v, *v) - 1.0).abs() < 1e-5);
        }
        assert_eq!(c.leaf, palette("leaf.base").unwrap());
        assert!(c.canopy_blend > 0.0, "leaves.json's canopyBlend is read");
    }

    #[test]
    fn each_species_stage_and_variant_has_its_own_layer() {
        // face-a-only: data only.
        let c = Config::get();
        let mut seen = std::collections::HashSet::new();
        for species in 0..3u16 {
            for &stage in &c.stages {
                for seed in 0..64u32 {
                    let l = c.layer(species, seed, stage);
                    assert!(l < 3 * c.layers_per_species());
                    assert_eq!(l / c.layers_per_species(), u32::from(species));
                    seen.insert(l);
                }
            }
        }
        assert_eq!(
            seen.len() as u32,
            3 * c.layers_per_species(),
            "every layer is used"
        );
        let t = FarTree {
            anchor: [4, 20, -9],
            species: 1,
            seed: 77,
            stage: 5,
            stem: [4, 20, -9],
            stem_cells: 4,
        };
        let i = c.instances(&[t])[0];
        assert_eq!(i.foot, [4.5, 10.0, -8.5]);
        assert_eq!(i.layer, c.layer(1, 77, 5));
        assert!(i.quarter_turns < 4);
    }

    #[test]
    fn a_bad_entry_fails_naming_its_field() {
        // face-a-only: data only.
        let bad = IMPOSTORS_JSON.replace("\"azimuths\": 8", "\"azimuths\": 0");
        assert!(Config::parse(&bad).unwrap_err().contains("views.azimuths"));
        let bad = IMPOSTORS_JSON.replace("\"leaf.base\"", "\"nope.base\"");
        assert!(Config::parse(&bad).unwrap_err().contains("albedo.leaf"));
        let bad = IMPOSTORS_JSON.replace("\"variants\"", "\"variantz\"");
        assert!(Config::parse(&bad).unwrap_err().contains("variantz"));
        let bad = IMPOSTORS_JSON.replace("[0, 40]", "[40, 0]");
        assert!(Config::parse(&bad).unwrap_err().contains("rise"));
    }

    #[test]
    fn a_layout_added_by_data_alone_bakes() {
        // face-a-only: data only.
        let text = IMPOSTORS_JSON
            .replace("\"azimuths\": 8", "\"azimuths\": 3")
            .replace("[0, 40]", "[10]")
            .replace("\"top\": true", "\"top\": false");
        let c = Config::parse(&text).unwrap();
        assert_eq!(c.views.len(), 3);
        let im = bake(find("dwell:oak").unwrap(), 1, 7, &c, 8);
        assert_eq!((im.cols, im.rows), (2, 2));
        assert!(im.albedo.iter().any(|a| a[3] >= 128));
    }

    #[test]
    fn the_bake_is_deterministic_per_seed() {
        // face-a-only: face-local bake.
        let c = small();
        for s in SPECIES.iter() {
            let a = bake(s, 42, s.stages - 1, &c, 16);
            assert_eq!(a, bake(s, 42, s.stages - 1, &c, 16), "{}", s.name);
            assert_ne!(
                a.albedo,
                bake(s, 43, s.stages - 1, &c, 16).albedo,
                "{}: another seed, another tree",
                s.name
            );
        }
    }

    /// The covered texels' extent in view `v`: (width, height) in texels.
    fn extent(im: &Impostor, v: usize) -> (u32, u32) {
        let (mut x0, mut x1, mut y0, mut y1) = (u32::MAX, 0, u32::MAX, 0);
        for j in 0..im.px {
            for i in 0..im.px {
                if im.albedo[im.at(v, i, j)][3] >= 128 {
                    (x0, x1, y0, y1) = (x0.min(i), x1.max(i), y0.min(j), y1.max(j));
                }
            }
        }
        assert!(x0 <= x1, "view {v} is empty");
        (x1 - x0 + 1, y1 - y0 + 1)
    }

    #[test]
    fn a_pines_top_view_is_round_and_its_side_view_taller_than_wide() {
        // face-a-only: face-local bake.
        let c = small();
        let pine = find("dwell:pine").unwrap();
        let im = bake(pine, 7, pine.stages - 1, &c, 48);
        let top = c.views.len() - 1;
        let (w, h) = extent(&im, top);
        let r = w as f32 / h as f32;
        assert!((0.75..=1.33).contains(&r), "top {w}×{h}");
        // Roughly round: the covered share of the bounding square near π/4.
        let covered = (0..im.px)
            .flat_map(|j| (0..im.px).map(move |i| (i, j)))
            .filter(|&(i, j)| im.albedo[im.at(top, i, j)][3] >= 128)
            .count() as f32;
        let fill = covered / (w * h) as f32;
        assert!((0.55..=0.95).contains(&fill), "top fill {fill}");
        for v in 0..c.azimuths as usize {
            let (w, h) = extent(&im, v);
            assert!(h > w, "side view {v}: {w}×{h}");
        }
    }

    #[test]
    fn normals_point_outward_on_the_crowns_silhouette() {
        // face-a-only: face-local bake.
        let c = small();
        for name in ["dwell:oak", "dwell:pine"] {
            let s = find(name).unwrap();
            let im = bake(s, 3, s.stages - 1, &c, 32);
            for (vi, &v) in c.views.iter().enumerate() {
                let (right, up) = basis(v);
                let (mut good, mut all) = (0, 0);
                for j in 0..im.px {
                    for i in 0..im.px {
                        let cov = |i: i32, j: i32| {
                            (0..im.px as i32).contains(&i)
                                && (0..im.px as i32).contains(&j)
                                && im.albedo[im.at(vi, i as u32, j as u32)][3] >= 128
                        };
                        let (ii, jj) = (i as i32, j as i32);
                        if !cov(ii, jj)
                            || (cov(ii - 1, jj)
                                && cov(ii + 1, jj)
                                && cov(ii, jj - 1)
                                && cov(ii, jj + 1))
                        {
                            continue;
                        }
                        let a = im.albedo[im.at(vi, i, j)];
                        if [a[0], a[1], a[2]] != c.leaf.map(|x| (x * 255.0).round() as u8) {
                            continue;
                        }
                        // The texel's screen offset from the view's centre, and its normal on screen.
                        let sx = i as f32 + 0.5 - im.px as f32 / 2.0;
                        let sy = im.px as f32 / 2.0 - (j as f32 + 0.5);
                        let nd = im.normal_depth[im.at(vi, i, j)];
                        let n = [0, 1, 2].map(|k| nd[k] as f32 / 255.0 * 2.0 - 1.0);
                        let (nx, ny) = (dot(n, right), dot(n, up));
                        all += 1;
                        if nx * sx + ny * sy > 0.0 {
                            good += 1;
                        }
                    }
                }
                assert!(all > 0, "{name} view {vi}: no silhouette");
                let share = good as f32 / all as f32;
                assert!(share > 0.8, "{name} view {vi}: {good}/{all} outward");
            }
        }
    }

    #[test]
    fn an_empty_tree_bakes_an_empty_atlas() {
        // face-a-only: face-local bake.
        let c = small();
        let im = bake_cells(&Default::default(), &c, 8);
        assert!(im.albedo.iter().all(|a| a[3] == 0));
        assert!(im.normal_depth.iter().all(|n| n[3] == 255));
        assert_eq!(im.size(), (im.cols * 8, im.rows * 8));
    }

    #[test]
    fn a_fixture_species_bakes_by_data_alone() {
        // face-a-only: face-local bake.
        let shrub = species_gen_fixtures()
            .iter()
            .find(|s| s.name == "test:shrub")
            .unwrap();
        let c = small();
        let im = bake(shrub, 5, shrub.stages - 1, &c, 16);
        let grown = grow(shrub, 5, shrub.stages - 1);
        assert!(!grown.cells.is_empty());
        assert!(im.albedo.iter().any(|a| a[3] >= 128));
        // Smaller than an oak: the bounding sphere follows the tree.
        let oak = bake(find("dwell:oak").unwrap(), 5, 7, &c, 16);
        assert!(im.radius < oak.radius);
        // Its trunk shows as bark low in a side view.
        let bark = c.log.map(|x| (x * 255.0).round() as u8);
        let any_bark = (0..im.px)
            .flat_map(|j| (0..im.px).map(move |i| (i, j)))
            .any(|(i, j)| im.albedo[im.at(0, i, j)][..3] == bark);
        assert!(any_bark, "the shrub's trunk is baked");
    }

    #[test]
    fn a_thin_trunk_is_baked_at_its_width() {
        // face-a-only: face-local bake.
        let mut cells = std::collections::BTreeMap::new();
        for y in 0..8 {
            cells.insert([0, y, 0], Part::Log(0, 1));
        }
        let c = small();
        let thin = bake_cells(&cells, &c, 32);
        for y in 0..8 {
            cells.insert([0, y, 0], Part::Log(0, 4));
        }
        let full = bake_cells(&cells, &c, 32);
        let (wt, _) = extent(&thin, 0);
        let (wf, _) = extent(&full, 0);
        assert!(wt * 3 <= wf + 1, "¼ m trunk {wt} texels, 1 m trunk {wf}");
        // A standing log's width is the same down its whole length (its octagon lies across y).
        let widths: Vec<usize> = (0..full.px)
            .map(|j| {
                (0..full.px)
                    .filter(|&i| full.albedo[full.at(0, i, j)][3] >= 128)
                    .count()
            })
            .filter(|&w| w > 0)
            .collect();
        assert!(widths.iter().all(|&w| w == widths[0]), "{widths:?}");
        // The full log's side normal faces the camera in view 0 (from +x).
        let mid = full.at(0, full.px / 2, full.px / 2);
        assert!(
            full.normal_depth[mid][0] > 230,
            "{:?}",
            full.normal_depth[mid]
        );
    }

    #[test]
    fn pick_takes_a_view_exactly_and_blends_between() {
        // face-a-only: data only.
        let c = Config::get();
        for (i, &v) in c.views.iter().enumerate() {
            let p = c.pick(v);
            assert_eq!(p.len(), 1, "view {i}: {p:?}");
            assert_eq!(p[0].0, i);
        }
        let between = norm([1.0, 0.0, (std::f32::consts::PI / 8.0).tan()]);
        let p = c.pick(between);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|&(_, w)| (w - 0.5).abs() < 1e-3), "{p:?}");
        let sum: f32 = c.pick([0.3, 0.8, -0.2]).iter().map(|x| x.1).sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_far_tiers_parse_and_a_bad_one_fails_naming_its_field() {
        // face-a-only: data only.
        let c = Config::get();
        let on = c.far("on").expect("the `on` tier");
        assert!(on.impostor && on.from > 0.0 && on.fade > 0.0, "{on:?}");
        assert!(!c.far("off").expect("the `off` tier").impostor);
        let fade = format!("\"fadeM\": {}", on.fade);
        assert!(
            IMPOSTORS_JSON.contains(&fade),
            "the `on` tier's fade as written"
        );
        let bad = IMPOSTORS_JSON.replace(&fade, "\"fadeM\": -1");
        let e = Config::parse(&bad).unwrap_err();
        assert!(e.contains("far.tiers.on.fadeM"), "{e}");
        let bad = IMPOSTORS_JSON.replace("\"draw\": \"none\"", "\"draw\": \"blob\"");
        assert!(
            Config::parse(&bad)
                .unwrap_err()
                .contains("far.tiers.off.draw")
        );
        // A tier added by data alone.
        let added = IMPOSTORS_JSON.replace(
            "\"off\": { \"draw\": \"none\" }",
            "\"off\": { \"draw\": \"none\" }, \"near\": { \"draw\": \"impostor\", \"fromM\": 32, \"fadeM\": 8, \"toM\": 400 }",
        );
        let n = Config::parse(&added).unwrap().far("near").unwrap();
        assert_eq!((n.from, n.fade, n.to), (32.0, 8.0, 400.0));
        assert_eq!(n.share(401.0), 0.0);
    }

    #[test]
    fn the_hand_over_crossfades_with_shares_summing_to_one() {
        // face-a-only: distances only.
        let f = Config::get().far("on").unwrap();
        assert_eq!(f.share(f.from - 1.0), 0.0);
        assert_eq!(f.near_share(f.from - 1.0), 1.0);
        assert_eq!(f.share(f.from + f.fade + 1.0), 1.0);
        let mid = f.from + f.fade / 2.0;
        assert!((f.share(mid) - 0.5).abs() < 1e-5);
        for d in [0.0, f.from, mid, f.from + f.fade, 5000.0] {
            assert!((f.share(d) + f.near_share(d) - 1.0).abs() < 1e-5, "at {d}");
        }
        let off = Config::get().far("off").unwrap();
        assert_eq!((off.share(500.0), off.near_share(500.0)), (0.0, 1.0));
    }

    #[test]
    fn dithering_shows_each_view_on_its_weights_share_of_the_pixels() {
        // face-a-only: data only.
        let c = Config::get();
        let dir = [0.6, 0.35, 0.25];
        let picks = c.pick(dir);
        assert!(picks.len() >= 2, "{picks:?}");
        let n = 1000;
        let mut counts = std::collections::HashMap::new();
        for k in 0..n {
            let t = (k as f32 + 0.5) / n as f32;
            *counts.entry(c.dither(dir, t)).or_insert(0) += 1;
        }
        for (i, w) in picks {
            let got = counts.get(&i).copied().unwrap_or(0) as f32 / n as f32;
            assert!((got - w).abs() < 0.002, "view {i}: {got} vs {w}");
        }
        // On an exact view every pixel draws it.
        assert!((0..10).all(|k| c.dither(c.views[3], k as f32 / 10.0) == 3));
    }

    #[test]
    fn a_tree_takes_the_nearest_baked_stage_and_a_variant_by_its_seed() {
        // face-a-only: data only.
        let c = Config::get();
        assert_eq!(c.stages, vec![2, 4, 7]);
        assert_eq!(c.baked_for(1, 0).0, 0);
        assert_eq!(c.baked_for(1, 3).0, 0, "3 is as near 2 as 4: the lower");
        assert_eq!(c.baked_for(1, 5).0, 1);
        assert_eq!(c.baked_for(1, 6).0, 2);
        assert_eq!(c.baked_for(1, 99).0, 2);
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64 {
            let (_, v) = c.baked_for(seed, 4);
            assert!(v < c.variants);
            assert_eq!(c.baked_for(seed, 4).1, v, "deterministic");
            seen.insert(v);
        }
        assert_eq!(seen.len() as u32, c.variants, "every variant is used");
    }
}

#[cfg(test)]
mod face_tests {
    use super::*;
    use dwell_world::FaceFrame;

    dwell_world::both_faces!(
        fn the_view_a_far_tree_shows_is_the_same_on_both_faces(f: FaceFrame) {
            // A tree and an eye placed face-locally through `f` pick the views face A's do.
            let c = Config::get();
            let tree = [12.0, 40.0, -7.0];
            for eye in [[60.0, 52.0, -7.0], [12.0, 140.0, -6.0], [-30.0, 41.0, 25.0]] {
                let a = c.pick(view_dir(FaceFrame::A, tree, eye));
                let here = c.pick(view_dir(f, f.point(tree), f.point(eye)));
                assert_eq!(a.len(), here.len(), "{eye:?}");
                for ((i, w), (j, v)) in a.iter().zip(&here) {
                    assert_eq!(i, j, "{eye:?}");
                    assert!((w - v).abs() < 1e-5);
                }
            }
            // An eye straight over the tree (face-locally) sees the top view.
            let top = c.views.len() - 1;
            let d = view_dir(f, f.point(tree), f.point([12.0, 300.0, -7.0]));
            assert_eq!(c.dither(d, 0.7), top);
        }
    );
}

#[cfg(test)]
mod crown_match {
    // face-a-only: the bake and the draw's sampling are face-local (+y up); the drawer crosses faces.
    //! BUGS.md #106: an impostor's crown, as the draw samples it (render/impostors.rs `imp_fs`: the
    //! quad point, the view `Config::dither` picks per pixel, the texel), against the real tree's
    //! silhouette from the same side: the same size and density.
    use super::*;
    use crate::species::{SPECIES, grow, wild};

    /// shaders.rs `pixel_noise` (the dither's threshold per pixel).
    fn pixel_noise(i: u32, j: u32) -> f32 {
        let x = (i as f32 * 0.067_110_56 + j as f32 * 0.005_837_15).fract();
        (52.982_918 * x).fract()
    }

    #[test]
    fn a_far_crown_covers_what_the_real_crown_does() {
        let c = Config::get();
        let px = c.px(Tier::Desktop);
        let (mut truth, mut drawn, mut both) = (0u32, 0u32, 0u32);
        for sp in SPECIES.iter() {
            for (seed, stage) in wild(sp).into_iter().take(3) {
                let real = grow(sp, seed, stage);
                let b = bake_cells(&real.cells, c, px);
                let grid: HashMap<[i32; 3], Part> =
                    real.cells.iter().map(|(k, p)| (*k, *p)).collect();
                let shapes: HashMap<Part, Vec<([f32; 3], f32)>> =
                    real.cells.values().map(|p| (*p, slabs(*p))).collect();
                let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
                for k in real.cells.keys() {
                    for a in 0..3 {
                        lo[a] = lo[a].min(k[a]);
                        hi[a] = hi[a].max(k[a]);
                    }
                }
                let r = b.radius;
                // Seen from a far eye a little above the horizon, between and on the baked views.
                for az_deg in [7.0f32, 22.5, 31.0] {
                    let (el, az) = (3.0f32.to_radians(), az_deg.to_radians());
                    let eye = [el.cos() * az.cos(), el.sin(), el.cos() * az.sin()];
                    let (right, up) = basis(eye);
                    let n = 96;
                    for j in 0..n {
                        for i in 0..n {
                            let sx = ((i as f32 + 0.5) / n as f32 * 2.0 - 1.0) * r;
                            let sy = (1.0 - (j as f32 + 0.5) / n as f32 * 2.0) * r;
                            let local: [f32; 3] =
                                std::array::from_fn(|a| right[a] * sx + up[a] * sy);
                            let o: [f32; 3] =
                                std::array::from_fn(|a| b.centre[a] + local[a] + eye[a] * r);
                            let t = march(o, eye.map(|x| -x), lo, hi, &grid, &shapes).is_some();
                            let k = c.dither(eye, pixel_noise(i, j));
                            let (rk, uk) = basis(c.views[k]);
                            let (u, v) = (dot(local, rk) / r, dot(local, uk) / r);
                            let d = u.abs() < 1.0 && v.abs() < 1.0 && {
                                let q = |x: f32| ((x * px as f32) as u32).min(px - 1);
                                b.albedo[b.at(k, q(u * 0.5 + 0.5), q(0.5 - v * 0.5))][3] > 127
                            };
                            truth += u32::from(t);
                            drawn += u32::from(d);
                            both += u32::from(t && d);
                        }
                    }
                }
            }
        }
        let area = drawn as f32 / truth as f32;
        let iou = both as f32 / (truth + drawn - both) as f32;
        // Measured 2026-11-12: area 1.02, overlap 0.80 (the dither between views 45° apart).
        assert!(
            (0.85..1.2).contains(&area),
            "impostor crown area {area:.2} of the real"
        );
        assert!(iou > 0.7, "impostor crown overlap {iou:.2}");
    }

    #[test]
    fn ao_is_off_by_default_and_darkens_texels_deep_in_the_crown_when_on() {
        // face-a-only: the bake's own frame.
        let c = Config::get();
        assert_eq!(
            c.ao_strength, 0.0,
            "opt-in until the owner approves the look"
        );
        let s = &crate::species::SPECIES[0];
        let plain = bake(s, 3, 7, c, 32);
        assert!(
            plain.albedo.iter().all(|a| a[3] == 0 || a[3] == 255),
            "AO off: every covered texel open"
        );
        let text = include_str!("../../../shared/trees/impostors/impostors.json");
        let mut v: serde_json::Value = serde_json::from_str(text).unwrap();
        v["ao"] = serde_json::json!({ "strength": 0.6, "radiusM": 1.5 });
        let on = Config::parse(&v.to_string()).unwrap();
        // A full block of leaves: its middle is darker than a lone cell (nothing about it).
        let mut cells = std::collections::BTreeMap::new();
        for x in 0..5 {
            for y in 0..10 {
                for z in 0..5 {
                    cells.insert([x, y, z], Part::Leaf);
                }
            }
        }
        let grid: HashMap<[i32; 3], Part> = cells.iter().map(|(k, p)| (*k, *p)).collect();
        let deep = occlusion(&grid, [2.5, 2.5, 2.5], &on);
        let lone: HashMap<[i32; 3], Part> = [([0, 0, 0], Part::Leaf)].into_iter().collect();
        let open = occlusion(&lone, [0.5, 0.25, 0.5], &on);
        assert_eq!(open, 1.0);
        assert!(
            (deep - 0.4).abs() < 1e-4,
            "{deep}: inside, every neighbour filled"
        );
        // The real tree baked with it: some texels darker, coverage unchanged.
        let dark = bake(s, 3, 7, &on, 32);
        let covered = |im: &Impostor| im.albedo.iter().filter(|a| a[3] >= 128).count();
        assert_eq!(covered(&dark), covered(&plain));
        let mean = dark
            .albedo
            .iter()
            .filter(|a| a[3] >= 128)
            .map(|a| texel_ao(a[3]))
            .sum::<f32>()
            / covered(&dark) as f32;
        assert!((0.5..0.98).contains(&mean), "mean occlusion {mean}");
        assert_eq!(texel_ao(ao_alpha(0.37)), (0.37f32 * 127.0).round() / 127.0);
        v["ao"] = serde_json::json!({ "strength": 2.0, "radiusM": 1.5 });
        assert!(
            Config::parse(&v.to_string())
                .unwrap_err()
                .contains("ao.strength")
        );
        v["ao"] = serde_json::json!({ "strength": 0.5, "radiusM": 1.5, "x": 1 });
        assert!(Config::parse(&v.to_string()).unwrap_err().contains("ao.x"));
    }

    #[test]
    fn a_mip_halves_each_view_alone_keeping_coverage_and_palette_colours() {
        // face-a-only: the atlas's texels.
        let c = Config::get();
        assert!(!c.mips, "opt-in until the owner approves the look");
        let (leaf, log) = ([10u8, 200, 30], [90u8, 60, 20]);
        // Two views side by side, 4 px each: view 0 three-quarters covered (3 leaf, 1 log of
        // each 2×2), view 1 a quarter covered.
        let mut im = Impostor {
            px: 4,
            cols: 2,
            rows: 1,
            centre: [0.5, 5.0, 0.5],
            radius: 5.0,
            views: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            albedo: vec![[0; 4]; 32],
            normal_depth: vec![[128, 128, 128, 255]; 32],
        };
        for j in 0..4 {
            for i in 0..4 {
                let (a, b) = (im.at(0, i, j), im.at(1, i, j));
                let q = (i % 2, j % 2);
                if q != (1, 1) {
                    let rgb = if q == (0, 0) { log } else { leaf };
                    im.albedo[a] = [rgb[0], rgb[1], rgb[2], ao_alpha(0.5)];
                    im.normal_depth[a] = [255, 128, 128, 100];
                }
                if q == (0, 0) {
                    im.albedo[b] = [leaf[0], leaf[1], leaf[2], 255];
                }
            }
        }
        assert_eq!(Impostor::mip_levels(4), 3);
        assert_eq!(Impostor::mip_levels(48), 1, "not a power of two: no chain");
        let (a1, n1) = im.mip(1);
        assert_eq!(a1.len(), 8, "4 × 2 texels");
        for y in 0..2 {
            for x in 0..4 {
                let t = a1[y * 4 + x];
                if x < 2 {
                    assert_eq!(&t[..3], &leaf, "the commonest colour, a palette leaf");
                    assert_eq!(t[3], ao_alpha(0.5));
                    assert_eq!(n1[y * 4 + x], [255, 128, 128, 100]);
                } else {
                    assert!(
                        t[3] < 128,
                        "a quarter covered: empty, view 0 never bleeds in"
                    );
                }
            }
        }
        let (a2, _) = im.mip(2);
        assert_eq!(a2.len(), 2);
        assert!(a2[0][3] >= 128 && a2[1][3] < 128);
        // A real bake's chain ends at one texel a view.
        let b = bake(&crate::species::SPECIES[0], 3, 7, c, 16);
        let last = Impostor::mip_levels(16) - 1;
        assert_eq!(b.mip(last).0.len(), (b.cols * b.rows) as usize);
    }
}
