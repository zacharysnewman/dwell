//! The clouds' weather field (docs/CLOUDS.md §11, WORK_QUEUE item 13): a coarse, slowly changing
//! map of the weather over the eye's region, which the volume's march reads per column (cover, cloud
//! type, the floor's height and the clouds' depth), so the sky is not one weather everywhere.
//!
//! The model follows the production weather maps (Schneider 2015 and 2017, Horizon Zero Dawn and
//! Nubis; Hillaire 2016, Frostbite; Bauer 2019, Red Dead Redemption 2): a 2D map at kilometre cells
//! drives the cloud volume, and a weather system writes it. Here the weather system is a small
//! climate model over noise, all data (`clouds.json` `weatherField`):
//!
//! - **Humidity**: three octaves of evolving 3D value noise (x, z and time), not periodic — synoptic
//!   systems ~160 km across, domain-warped into fronts, mesoscale cells and local cells — moving with
//!   the steering wind and sharpened about the mean, so there are clear patches, overcast regions and
//!   the fronts between them.
//! - **Terrain**: the ground under the column and upwind of it (the world's map). Where the wind
//!   climbs the ground the air is lifted and humidity rises (orographic cloud over windward slopes
//!   and summits); where the highest ground upwind is far over the column, the air has come down and
//!   dried (the rain shadow's clear lee). The sea is a little moister, with flatter, lower decks.
//! - **Type** by an instability field (and lift over mountains), **base** by dryness (dry air's
//!   condensation level is higher), **depth** by instability and humidity.
//!
//! Computed on the CPU in f64 world metres (so no wrap and no repetition), in the job workers
//! ([`bake`]); the shader samples the baked texture ([`encode`]'s channels).

/// The weather field's tunables (`clouds.json` `weatherField`; docs/CLOUDS.md §11).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherFieldTunables {
    /// The map's window: `size_cells`² texels `cell_m` across round the eye, moved once the eye is
    /// `recentre_m` from its centre, recomputed every `update_s`.
    pub size_cells: u32,
    pub cell_m: f64,
    pub update_s: f64,
    pub recentre_m: f64,
    /// The weather systems move with the wind at `wind.speedMps × steer`.
    pub steer: f64,
    pub synoptic_m: f64,
    pub synoptic_evolve_s: f64,
    pub front_warp_m: f64,
    pub meso_m: f64,
    pub meso_evolve_s: f64,
    pub meso_weight: f64,
    pub local_m: f64,
    pub local_evolve_s: f64,
    pub local_weight: f64,
    /// How much the humidity is sharpened about its mean (0: the raw noise's narrow spread).
    pub contrast: f64,
    pub instability_m: f64,
    pub instability_evolve_s: f64,
    /// The type's spread (±) about `weather.type` by the instability.
    pub type_range: f64,
    /// Orographic lift: humidity + `upslope_gain` × the ground's rise over `upwind_m` along the
    /// wind, + `elevation_gain` per km of ground.
    pub upwind_m: f64,
    pub upslope_gain: f64,
    pub elevation_gain: f64,
    /// The rain shadow: humidity − `shadow_gain` per km the highest ground within `shadow_m`
    /// upwind is over the column.
    pub shadow_m: f64,
    pub shadow_gain: f64,
    /// The type's and the depth's rise per unit of orographic lift.
    pub oro_type: f64,
    pub oro_depth: f64,
    /// Over the sea: humidity, type and the floor's lift (a share of `lift_max`) added.
    pub sea_humidity: f64,
    pub sea_type: f64,
    pub sea_lift: f64,
    /// The clouds' floor rises by up to `lift_max` of the layer in dry air.
    pub lift_max: f64,
    /// The type's height profile is scaled from `depth_min` to `depth_max`.
    pub depth_min: f64,
    pub depth_max: f64,
}

/// The weather at a column: `humidity` (0–1, 0.5 the mean: moves the cover about the coverage),
/// `kind` (along the cloud-type axis), `lift` (the clouds' floor over the layer's, a share of the
/// layer's depth) and `depth` (the type's height profile's scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherCell {
    pub humidity: f64,
    pub kind: f64,
    pub lift: f64,
    pub depth: f64,
}

impl WeatherCell {
    /// The mean weather: mean humidity, the data's type, the floor unlifted, the type's own depth.
    pub fn neutral(t: &crate::clouds::CloudTunables) -> Self {
        Self {
            humidity: 0.5,
            kind: t.weather.r#type,
            lift: 0.0,
            depth: 1.0,
        }
    }
}

/// The ground under a point (x, z world metres): its height over sea level (negative: the sea
/// floor), or None where unknown (no terrain: flat or test worlds; outside the disc).
pub type Ground<'a> = &'a dyn Fn(f64, f64) -> Option<f64>;

/// No terrain: every column as flat land at sea level.
pub fn no_ground(_: f64, _: f64) -> Option<f64> {
    None
}

fn mix64(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

fn hash3(seed: u64, x: i64, y: i64, z: i64) -> f64 {
    let h = mix64(
        seed ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
            ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9),
    );
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn quintic(f: f64) -> f64 {
    f * f * f * (f * (f * 6.0 - 15.0) + 10.0)
}

/// Value noise in 3D (0–1), on an integer lattice with no period: a hash of the lattice point.
pub fn value3(seed: u64, p: [f64; 3]) -> f64 {
    let i = p.map(f64::floor);
    let u = [
        quintic(p[0] - i[0]),
        quintic(p[1] - i[1]),
        quintic(p[2] - i[2]),
    ];
    let [x, y, z] = i.map(|c| c as i64);
    let l = |a: f64, b: f64, k: f64| a + (b - a) * k;
    let h = |dx, dy, dz| hash3(seed, x + dx, y + dy, z + dz);
    l(
        l(
            l(h(0, 0, 0), h(1, 0, 0), u[0]),
            l(h(0, 1, 0), h(1, 1, 0), u[0]),
            u[1],
        ),
        l(
            l(h(0, 0, 1), h(1, 0, 1), u[0]),
            l(h(0, 1, 1), h(1, 1, 1), u[0]),
            u[1],
        ),
        u[2],
    )
}

/// One evolving field: value noise over (x, z) in cells `scale_m` across (turned by `turn` radians
/// so the octaves' lattices don't line up) and time in steps of `evolve_s`.
fn field(seed: u64, p: [f64; 2], scale_m: f64, s: f64, evolve_s: f64, turn: f64) -> f64 {
    let (sn, cs) = turn.sin_cos();
    let q = [
        (cs * p[0] - sn * p[1]) / scale_m,
        (sn * p[0] + cs * p[1]) / scale_m,
    ];
    value3(seed, [q[0], q[1], s / evolve_s])
}

fn heading(deg: f64) -> [f64; 2] {
    let r = deg.to_radians();
    [r.cos(), r.sin()]
}

/// A face's own weather: face B's field is another part of the noise.
pub fn face_seed(world_seed: u64, face_b: bool) -> u64 {
    mix64(
        world_seed
            ^ if face_b {
                0x5eed_b0b0_cafe_f00d
            } else {
                0x0c10_0d5e_ed00_0001
            },
    )
}

/// The free atmosphere's humidity at a point (no terrain): the three octaves, moved with the
/// steering wind, the synoptic one domain-warped into fronts, sharpened about the mean.
pub fn humidity_aloft(t: &crate::clouds::CloudTunables, seed: u64, x: f64, z: f64, s: f64) -> f64 {
    let f = &t.weather_field;
    let [wx, wz] = heading(t.wind.direction_deg).map(|c| c * t.wind.speed_mps * f.steer * s);
    let p = [x - wx, z - wz];
    let warp = |k: u64| {
        2.0 * field(
            seed.wrapping_add(k),
            p,
            0.6 * f.synoptic_m,
            s,
            f.synoptic_evolve_s,
            0.7,
        ) - 1.0
    };
    let ps = [
        p[0] + f.front_warp_m * warp(11),
        p[1] + f.front_warp_m * warp(12),
    ];
    let syn = field(
        seed.wrapping_add(1),
        ps,
        f.synoptic_m,
        s,
        f.synoptic_evolve_s,
        0.0,
    );
    let meso = field(seed.wrapping_add(2), p, f.meso_m, s, f.meso_evolve_s, 1.1);
    let local = field(seed.wrapping_add(3), p, f.local_m, s, f.local_evolve_s, 2.3);
    let raw = syn * (1.0 - f.meso_weight - f.local_weight)
        + meso * f.meso_weight
        + local * f.local_weight;
    0.5 + 0.5 * (2.0 * f.contrast * (raw - 0.5)).tanh()
}

/// The instability (0–1) at a point: how convective the air is (towering cumulus over stratiform
/// decks), its own evolving field moving with the steering wind.
pub fn instability(t: &crate::clouds::CloudTunables, seed: u64, x: f64, z: f64, s: f64) -> f64 {
    let f = &t.weather_field;
    let [wx, wz] = heading(t.wind.direction_deg).map(|c| c * t.wind.speed_mps * f.steer * s);
    let v = field(
        seed.wrapping_add(4),
        [x - wx, z - wz],
        f.instability_m,
        s,
        f.instability_evolve_s,
        0.4,
    );
    (0.5 + 1.6 * (v - 0.5)).clamp(0.0, 1.0)
}

/// The terrain's part at a point (x, z): the orographic lift (≥ 0: humidity added where the wind
/// climbs the ground and over high ground), the rain shadow (≥ 0: humidity taken in the lee of
/// higher ground upwind) and whether it is sea.
pub fn terrain_terms(
    t: &crate::clouds::CloudTunables,
    ground: Ground,
    x: f64,
    z: f64,
) -> (f64, f64, bool) {
    let f = &t.weather_field;
    let Some(g) = ground(x, z) else {
        return (0.0, 0.0, false);
    };
    let sea = g < 0.0;
    let e = g.max(0.0);
    let [dx, dz] = heading(t.wind.direction_deg);
    let up = |d: f64| ground(x - dx * d, z - dz * d).map_or(e, |h| h.max(0.0));
    let slope = (e - up(f.upwind_m)) / f.upwind_m;
    let lift = slope.max(0.0) * f.upslope_gain + e / 1000.0 * f.elevation_gain;
    let ridge = (1..=4)
        .map(|k| up(f.shadow_m * f64::from(k) / 4.0))
        .fold(e, f64::max);
    let shadow = (ridge - e) / 1000.0 * f.shadow_gain;
    (lift, shadow, sea)
}

/// The weather at a point (x, z world metres) on a face (`seed`: [`face_seed`]) at the clouds'
/// clock `s` (seconds), over `ground`.
pub fn cell_at(
    t: &crate::clouds::CloudTunables,
    seed: u64,
    x: f64,
    z: f64,
    s: f64,
    ground: Ground,
) -> WeatherCell {
    let f = &t.weather_field;
    let (lift, shadow, sea) = terrain_terms(t, ground, x, z);
    let sea = if sea { 1.0 } else { 0.0 };
    let h =
        (humidity_aloft(t, seed, x, z, s) + lift - shadow + sea * f.sea_humidity).clamp(0.0, 1.0);
    let i = instability(t, seed, x, z, s);
    let kind =
        (t.weather.r#type + f.type_range * (2.0 * i - 1.0) + f.oro_type * lift + sea * f.sea_type)
            .clamp(0.0, 1.0);
    let floor = ((1.0 - h) + sea * f.sea_lift).clamp(0.0, 1.0) * f.lift_max;
    let deep = (0.6 * i + 0.4 * h + f.oro_depth * lift).clamp(0.0, 1.0);
    WeatherCell {
        humidity: h,
        kind,
        lift: floor,
        depth: f.depth_min + (f.depth_max - f.depth_min) * deep,
    }
}

/// A cell as the map's RGBA8 texel: humidity, type, the floor's lift over `lift_max`, the depth
/// over its range.
pub fn encode(t: &crate::clouds::CloudTunables, c: &WeatherCell) -> [u8; 4] {
    let f = &t.weather_field;
    let q = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let span = (f.depth_max - f.depth_min).max(1e-9);
    [
        q(c.humidity),
        q(c.kind),
        q(if f.lift_max > 0.0 {
            c.lift / f.lift_max
        } else {
            0.0
        }),
        q((c.depth - f.depth_min) / span),
    ]
}

/// The texel back as a cell (the shader's `cloud_weather_map_at`).
pub fn decode(t: &crate::clouds::CloudTunables, b: [u8; 4]) -> WeatherCell {
    let f = &t.weather_field;
    let u = |v: u8| f64::from(v) / 255.0;
    WeatherCell {
        humidity: u(b[0]),
        kind: u(b[1]),
        lift: u(b[2]) * f.lift_max,
        depth: f.depth_min + u(b[3]) * (f.depth_max - f.depth_min),
    }
}

/// The map's corner (world x, z metres) for a window centred near `eye` (snapped to whole cells, so
/// a moved window samples the same field at the same points).
pub fn corner_for(t: &crate::clouds::CloudTunables, eye: [f64; 2]) -> [f64; 2] {
    let f = &t.weather_field;
    let half = 0.5 * f64::from(f.size_cells) * f.cell_m;
    eye.map(|c| ((c - half) / f.cell_m).round() * f.cell_m)
}

/// Whether a window at `corner` still serves an eye at `eye` (within `recentre_m` of its centre).
pub fn serves(t: &crate::clouds::CloudTunables, corner: [f64; 2], eye: [f64; 2]) -> bool {
    let f = &t.weather_field;
    let half = 0.5 * f64::from(f.size_cells) * f.cell_m;
    let d = [eye[0] - corner[0] - half, eye[1] - corner[1] - half];
    d[0].abs() <= f.recentre_m && d[1].abs() <= f.recentre_m
}

/// The map a renderer holds: its face, corner (world metres) and the clock it was made at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapAt {
    pub face_b: bool,
    pub corner: [f64; 2],
    pub seconds: f64,
}

/// The next map to ask for, if one is due: none while one is being made (`pending`); else when
/// there is none, the eye's face changed, the eye left the window's `recentre_m`, or `update_s` of
/// the clock passed (or it went back: a new world, a reset clock).
pub fn due(
    t: &crate::clouds::CloudTunables,
    have: Option<MapAt>,
    pending: bool,
    face_b: bool,
    eye: [f64; 2],
    seconds: f64,
) -> Option<MapAt> {
    if pending {
        return None;
    }
    let next = MapAt {
        face_b,
        corner: corner_for(t, eye),
        seconds,
    };
    let Some(h) = have else {
        return Some(next);
    };
    let age = seconds - h.seconds;
    if h.face_b != face_b || !serves(t, h.corner, eye) {
        return Some(next);
    }
    (!(0.0..t.weather_field.update_s).contains(&age)).then_some(MapAt {
        corner: h.corner,
        ..next
    })
}

/// The map (RGBA8, `size_cells`² texels, rows along +z) at `corner` (world metres) at clock `s`:
/// each texel the weather at its centre. The ground is sampled once on a grid of the texels'
/// centres with an apron reaching `shadow_m` upwind and read bilinearly, so the terrain costs a
/// fixed number of lookups however the terms sample it.
pub fn bake(
    t: &crate::clouds::CloudTunables,
    seed: u64,
    corner: [f64; 2],
    s: f64,
    ground: Ground,
) -> Vec<u8> {
    let f = &t.weather_field;
    let n = f.size_cells as i64;
    let cell = f.cell_m;
    let pad = ((f.shadow_m.max(f.upwind_m)) / cell).ceil() as i64 + 2;
    let side = n + 2 * pad;
    let origin = [
        corner[0] + 0.5 * cell - pad as f64 * cell,
        corner[1] + 0.5 * cell - pad as f64 * cell,
    ];
    let grid: Vec<Option<f64>> = (0..side * side)
        .map(|i| {
            let (gx, gz) = (i % side, i / side);
            ground(origin[0] + gx as f64 * cell, origin[1] + gz as f64 * cell)
        })
        .collect();
    let at = |gx: i64, gz: i64| -> Option<f64> {
        let (gx, gz) = (gx.clamp(0, side - 1), gz.clamp(0, side - 1));
        grid[(gz * side + gx) as usize]
    };
    let gridded = |x: f64, z: f64| -> Option<f64> {
        let u = (x - origin[0]) / cell;
        let v = (z - origin[1]) / cell;
        let (i, k) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - k);
        let (i, k) = (i as i64, k as i64);
        let c = [at(i, k), at(i + 1, k), at(i, k + 1), at(i + 1, k + 1)];
        if c.iter().all(Option::is_none) {
            return None;
        }
        let known: Vec<f64> = c.iter().flatten().copied().collect();
        let fill = known.iter().sum::<f64>() / known.len() as f64;
        let c = c.map(|h| h.unwrap_or(fill));
        let l = |a: f64, b: f64, k: f64| a + (b - a) * k;
        Some(l(l(c[0], c[1], fu), l(c[2], c[3], fu), fv))
    };
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for k in 0..n {
        for i in 0..n {
            let x = corner[0] + (i as f64 + 0.5) * cell;
            let z = corner[1] + (k as f64 + 0.5) * cell;
            out.extend_from_slice(&encode(t, &cell_at(t, seed, x, z, s, &gridded)));
        }
    }
    out
}

/// The map's texel nearest world point (x, z) in a baked `map` at `corner` (tests and tools).
pub fn texel_at(
    t: &crate::clouds::CloudTunables,
    map: &[u8],
    corner: [f64; 2],
    x: f64,
    z: f64,
) -> WeatherCell {
    let f = &t.weather_field;
    let n = f.size_cells as i64;
    let i = (((x - corner[0]) / f.cell_m).floor() as i64).clamp(0, n - 1);
    let k = (((z - corner[1]) / f.cell_m).floor() as i64).clamp(0, n - 1);
    let o = ((k * n + i) * 4) as usize;
    decode(t, [map[o], map[o + 1], map[o + 2], map[o + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clouds::CLOUDS;

    const SEED: u64 = 0x00c1_0d00_0000_002a;

    fn seed() -> u64 {
        face_seed(SEED, false)
    }

    /// The normalised autocorrelation of `v` at lag `lag` samples.
    fn autocorrelation(v: &[f64], lag: usize) -> f64 {
        let n = v.len() - lag;
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64;
        let cov = (0..n)
            .map(|i| (v[i] - mean) * (v[i + lag] - mean))
            .sum::<f64>()
            / n as f64;
        cov / var.max(1e-12)
    }

    fn transect(s: f64, km: usize, step_m: f64, along: fn(f64) -> [f64; 2]) -> Vec<f64> {
        (0..km)
            .map(|i| {
                let [x, z] = along(i as f64 * step_m);
                cell_at(&CLOUDS, seed(), x, z, s, &no_ground).humidity
            })
            .collect()
    }

    #[test]
    fn the_humidity_varies_at_macro_scale_with_clear_and_overcast_regions() {
        // 600 km × 600 km at 2 km: the field's spread, its clear and overcast shares.
        let mut v = Vec::new();
        for k in 0..300 {
            for i in 0..300 {
                let c = cell_at(
                    &CLOUDS,
                    seed(),
                    i as f64 * 2000.0,
                    k as f64 * 2000.0,
                    0.0,
                    &no_ground,
                );
                v.push(c.humidity);
            }
        }
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        let clear = v.iter().filter(|&&h| h < 0.25).count() as f64 / v.len() as f64;
        let overcast = v.iter().filter(|&&h| h > 0.75).count() as f64 / v.len() as f64;
        eprintln!("humidity mean {mean:.3} sd {sd:.3} clear {clear:.3} overcast {overcast:.3}");
        assert!((0.4..0.6).contains(&mean), "mean {mean}");
        assert!(sd > 0.15, "the field is nearly uniform: sd {sd}");
        assert!(
            clear > 0.08 && overcast > 0.08,
            "clear {clear} overcast {overcast}"
        );
    }

    #[test]
    fn the_field_does_not_repeat_at_any_macro_scale() {
        // A 2048 km transect at 1 km: the autocorrelation falls off past the systems' size and never
        // comes back near 1 at any lag (a periodic field would at its period: the old regional
        // fields' 128 km wrap, checked against the deck's lattice below).
        let v = transect(0.0, 2048, 1000.0, |d| [d * 0.83, d * 0.56 + 3000.0]);
        let near = autocorrelation(&v, 2);
        assert!(near > 0.8, "smooth at 2 km: {near}");
        let worst = (200..1024)
            .map(|lag| autocorrelation(&v, lag))
            .fold(f64::MIN, f64::max);
        eprintln!("autocorrelation: 2 km {near:.3}, worst past 200 km {worst:.3}");
        assert!(worst < 0.5, "a lag past 200 km correlates by {worst}");
        // Nor with the deck's lattice (shape.wrapM, 128 km), which the old regional fields (8 and
        // 16 km cells on it) repeated with exactly.
        let wrap = CLOUDS.shape.wrap_m;
        let at = |x: f64| cell_at(&CLOUDS, seed(), x, 1234.0, 0.0, &no_ground).humidity;
        let same = (0..64)
            .filter(|&i| (at(i as f64 * 997.0) - at(i as f64 * 997.0 + wrap)).abs() < 1e-9)
            .count();
        assert!(
            same < 4,
            "the field repeats with the deck's wrap at {same} of 64 points"
        );
    }

    #[test]
    fn the_spectrum_has_no_spike() {
        // The power spectrum of a 1024 km transect at 1 km: no single frequency band (past the
        // lowest) carries a spike over the smooth fall-off (a repeating pattern would).
        let v = transect(0.0, 1024, 1000.0, |d| [d, -0.3 * d]);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let n = v.len();
        let power: Vec<f64> = (1..n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, x) in v.iter().enumerate() {
                    let a = -2.0 * std::f64::consts::PI * (k * i) as f64 / n as f64;
                    re += (x - mean) * a.cos();
                    im += (x - mean) * a.sin();
                }
                re * re + im * im
            })
            .collect();
        // Each bin against the median of its neighbourhood (±8 bins).
        let mut worst: f64 = 0.0;
        for k in 4..power.len() - 8 {
            let mut nb: Vec<f64> = power[k.saturating_sub(8)..k + 8].to_vec();
            nb.sort_by(f64::total_cmp);
            let med = nb[nb.len() / 2].max(1e-12);
            worst = worst.max(power[k] / med);
        }
        eprintln!("spectrum's worst bin over its neighbourhood: {worst:.1}x");
        assert!(worst < 40.0, "a spike {worst}x its neighbours");
    }

    #[test]
    fn base_and_top_heights_vary_by_region() {
        let cells: Vec<WeatherCell> = (0..400)
            .map(|i| {
                let (x, z) = ((i % 20) as f64 * 15000.0, (i / 20) as f64 * 15000.0);
                cell_at(&CLOUDS, seed(), x, z, 0.0, &no_ground)
            })
            .collect();
        let spread = |f: &dyn Fn(&WeatherCell) -> f64| {
            let v: Vec<f64> = cells.iter().map(f).collect();
            let lo = v.iter().copied().fold(f64::MAX, f64::min);
            let hi = v.iter().copied().fold(f64::MIN, f64::max);
            (lo, hi)
        };
        let h = CLOUDS.volume.top_m - CLOUDS.volume.base_m;
        let (lo, hi) = spread(&|c| c.lift * h);
        eprintln!("floor lift {lo:.0}–{hi:.0} m");
        assert!(hi - lo > 400.0, "the clouds' base varies by {} m", hi - lo);
        // The top of a cumulus column: its profile's end × depth over the lifted floor.
        let cu = crate::clouds::type_profile(CLOUDS.cloud_types, 0.6);
        let (lo, hi) = spread(&|c| (c.lift + cu.top[1] * c.depth) * h);
        eprintln!("cumulus tops {lo:.0}–{hi:.0} m over the layer's floor");
        assert!(hi - lo > 800.0, "the tops vary by {} m", hi - lo);
        let (lo, hi) = spread(&|c| c.kind);
        assert!(hi - lo > 0.4, "the type varies by {}", hi - lo);
    }

    #[test]
    fn the_field_drifts_with_the_wind_and_evolves() {
        let f = &CLOUDS.weather_field;
        let dt = 600.0;
        let [dx, dz] =
            heading(CLOUDS.wind.direction_deg).map(|c| c * CLOUDS.wind.speed_mps * f.steer * dt);
        let pts: Vec<[f64; 2]> = (0..400)
            .map(|i| [(i % 20) as f64 * 7000.0, (i / 20) as f64 * 7000.0])
            .collect();
        let at = |p: [f64; 2], s: f64| cell_at(&CLOUDS, seed(), p[0], p[1], s, &no_ground).humidity;
        let still: f64 = pts
            .iter()
            .map(|&p| (at(p, dt) - at(p, 0.0)).abs())
            .sum::<f64>()
            / 400.0;
        let carried: f64 = pts
            .iter()
            .map(|&p| (at([p[0] + dx, p[1] + dz], dt) - at(p, 0.0)).abs())
            .sum::<f64>()
            / 400.0;
        eprintln!("10 min: at a place {still:.4}, carried by the wind {carried:.4}");
        assert!(carried < still, "the weather moves with the wind");
        assert!(carried > 1e-3, "and changes as it goes");
        // Over hours a place's weather changes a lot.
        let later: f64 = pts
            .iter()
            .map(|&p| (at(p, 4.0 * 3600.0) - at(p, 0.0)).abs())
            .sum::<f64>()
            / 400.0;
        assert!(
            later > 0.12,
            "4 h later the weather is much the same: {later}"
        );
    }

    /// A ridge across the wind: a Gaussian 1.6 km high, its crest at `crest_m` along the wind.
    fn ridge(crest_m: f64) -> impl Fn(f64, f64) -> Option<f64> {
        let [dx, dz] = heading(CLOUDS.wind.direction_deg);
        move |x, z| {
            let along = x * dx + z * dz - crest_m;
            Some(1600.0 * (-(along / 6000.0).powi(2)).exp())
        }
    }

    #[test]
    fn clouds_thicken_over_windward_mountains_and_clear_in_their_lee() {
        let [dx, dz] = heading(CLOUDS.wind.direction_deg);
        let ground = ridge(0.0);
        let at = |along: f64, g: Ground| {
            let (mut h, mut k, mut d) = (0.0, 0.0, 0.0);
            // The mean over many places along the ridge and times (the weather aloft averaged out).
            for j in 0..200 {
                let across = j as f64 * 3100.0;
                let x = dx * along - dz * across;
                let z = dz * along + dx * across;
                let c = cell_at(&CLOUDS, seed(), x, z, j as f64 * 900.0, g);
                h += c.humidity / 200.0;
                k += c.kind / 200.0;
                d += c.depth / 200.0;
            }
            (h, k, d)
        };
        let flat = at(-5000.0, &no_ground);
        let windward = at(-5000.0, &ground);
        let lee = at(14000.0, &ground);
        eprintln!("flat {flat:?} windward {windward:?} lee {lee:?}");
        assert!(windward.0 > flat.0 + 0.1, "moister on the windward slope");
        assert!(windward.1 > flat.1 + 0.05, "taller types there");
        assert!(windward.2 > flat.2, "deeper clouds there");
        assert!(lee.0 < flat.0 - 0.1, "drier in the lee");
    }

    #[test]
    fn the_sea_has_lower_flatter_decks() {
        let sea = |_: f64, _: f64| Some(-40.0);
        let land = |_: f64, _: f64| Some(0.0);
        let (mut ks, mut kl, mut ls, mut ll) = (0.0, 0.0, 0.0, 0.0);
        for j in 0..200 {
            let (x, z) = (j as f64 * 9000.0, j as f64 * 4000.0);
            let s = cell_at(&CLOUDS, seed(), x, z, 0.0, &sea);
            let l = cell_at(&CLOUDS, seed(), x, z, 0.0, &land);
            ks += s.kind;
            kl += l.kind;
            ls += s.lift;
            ll += l.lift;
        }
        assert!(ks < kl && ls < ll);
    }

    #[test]
    fn the_baked_map_is_the_field_at_its_texels_and_round_trips() {
        let t = &CLOUDS;
        let ground = ridge(20000.0);
        let corner = corner_for(t, [12345.0, -6789.0]);
        let map = bake(t, seed(), corner, 100.0, &ground);
        let n = t.weather_field.size_cells as usize;
        assert_eq!(map.len(), n * n * 4);
        let mut worst: f64 = 0.0;
        for (i, k) in [(0, 0), (5, 9), (31, 32), (63, 63), (40, 2)] {
            let x = corner[0] + (i as f64 + 0.5) * t.weather_field.cell_m;
            let z = corner[1] + (k as f64 + 0.5) * t.weather_field.cell_m;
            let want = cell_at(t, seed(), x, z, 100.0, &ground);
            let got = texel_at(t, &map, corner, x, z);
            worst = worst
                .max((want.humidity - got.humidity).abs())
                .max((want.kind - got.kind).abs());
        }
        // The ground is read from a grid of the texels (bilinear) and the texel is 8 bits.
        assert!(worst < 0.04, "the bake is off the field by {worst}");
        // A window snapped to whole cells: the same point is the same texel value after a move.
        let moved = corner_for(t, [12345.0 + 3000.0, -6789.0]);
        let map2 = bake(t, seed(), moved, 100.0, &ground);
        let (x, z) = (corner[0] + 40.5 * 1024.0, corner[1] + 30.5 * 1024.0);
        assert_eq!(
            texel_at(t, &map, corner, x, z),
            texel_at(t, &map2, moved, x, z)
        );
        assert!(serves(t, corner, [12345.0, -6789.0]));
        assert!(!serves(
            t,
            corner,
            [12345.0 + 2.0 * t.weather_field.recentre_m, -6789.0]
        ));
    }

    // both-faces: a map on one face is due again on the other.
    #[test]
    fn a_new_map_is_due_by_the_clock_the_eye_and_the_face() {
        let t = &CLOUDS;
        let f = &t.weather_field;
        let eye = [5000.0, -2000.0];
        let first = due(t, None, false, false, eye, 10.0).unwrap();
        assert!(serves(t, first.corner, eye));
        let have = Some(first);
        assert_eq!(due(t, have, true, false, eye, 1e6), None, "one at a time");
        assert_eq!(
            due(t, have, false, false, eye, 10.0 + 0.5 * f.update_s),
            None
        );
        let later = due(t, have, false, false, eye, 10.0 + f.update_s).unwrap();
        assert_eq!(later.corner, first.corner, "same window, newer weather");
        let moved = [eye[0] + 2.0 * f.recentre_m, eye[1]];
        let re = due(t, have, false, false, moved, 10.1).unwrap();
        assert!(serves(t, re.corner, moved) && re.corner != first.corner);
        assert!(due(t, have, false, true, eye, 10.1).unwrap().face_b);
        assert!(
            due(t, have, false, false, eye, 3.0).is_some(),
            "a clock gone back"
        );
    }

    #[test]
    fn the_faces_have_their_own_weather() {
        let a = face_seed(SEED, false);
        let b = face_seed(SEED, true);
        let diff: f64 = (0..100)
            .map(|i| {
                let x = i as f64 * 11000.0;
                (cell_at(&CLOUDS, a, x, 0.0, 0.0, &no_ground).humidity
                    - cell_at(&CLOUDS, b, x, 0.0, 0.0, &no_ground).humidity)
                    .abs()
            })
            .sum::<f64>()
            / 100.0;
        assert!(diff > 0.05);
    }

    #[test]
    #[ignore]
    fn bake_cost() {
        let ground = ridge(0.0);
        let t0 = std::time::Instant::now();
        for i in 0..10 {
            std::hint::black_box(bake(&CLOUDS, seed(), [0.0, 0.0], i as f64, &ground));
        }
        eprintln!("bake: {:.2} ms", t0.elapsed().as_secs_f64() * 100.0);
    }
}
