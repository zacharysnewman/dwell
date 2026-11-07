//! Explosives (docs/EXPLOSIVES.md): the table of explosive blocks — what lights them, their fuse,
//! their blast's radius, force and falloff, what they do to players, and the clients' effects —
//! and the rules the server and the clients' prediction share: lighting one (a Place with a torch
//! held, aimed at it, `check_block_edit`), the blast's falloff and the speed it gives a block.
//!
//! Face-local: the blast's lift is along the face's up at the blast (`FaceFrame::at_height`).
//! Data: `shared/explosives/explosives.json` → `explosives.gen.rs` (`node shared/explosives/gen.mjs`).

use dwell_blocks::{StateId, info, try_info};

use crate::{CELL_H, FaceFrame};

/// The server's bounds on blasts (`limits`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplosiveLimits {
    /// The most blasts set off a tick; more wait a tick.
    pub blasts_per_tick: u32,
    /// The most cells one blast flings (the nearest first).
    pub max_pieces: u32,
}

/// The flash: a short point light and a fireball billboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashLook {
    pub color: [f32; 3],
    pub strength: f64,
    pub light_radius_m: f64,
    pub seconds: f64,
    pub size_m: f64,
}

/// Puffs of smoke (rising, spreading) or dust (thrown out, falling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PuffLook {
    pub color: [f32; 3],
    pub count: u32,
    /// How many on a CPU adapter.
    pub cpu_count: u32,
    pub size_m: [f64; 2],
    pub seconds: [f64; 2],
    pub spread_m: f64,
    pub rise_ms: f64,
    pub opacity: f64,
    pub speed_ms: [f64; 2],
}

/// The boom's loudness: full within `full_m`, gone at `hear_m`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoomLook {
    pub full_m: f64,
    pub hear_m: f64,
}

/// The camera's shake within `radius_m` (strongest at the blast).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakeLook {
    pub radius_m: f64,
    pub degrees: f64,
    pub seconds: f64,
    pub hz: f64,
}

/// A blast's effects on the clients.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlastEffects {
    pub flash: FlashLook,
    pub smoke: PuffLook,
    pub dust: PuffLook,
    pub boom: BoomLook,
    pub shake: ShakeLook,
}

/// An explosive (`explosives` in the table; what each field means is in the data's `$comment`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplosiveData {
    pub name: &'static str,
    pub block: &'static str,
    pub lit: &'static str,
    pub ignited_by: &'static [&'static str],
    pub fuse_seconds: f64,
    pub radius_m: f64,
    /// Within it (at most `radius_m`) a blast destroys blocks outright; beyond, to `radius_m`,
    /// they fly as pieces. Players are hurt only within it.
    pub destroy_radius_m: f64,
    /// Out to it (at least `radius_m`) blocks become pieces too; those past `radius_m` are given
    /// no impulse (they slump and fall).
    pub loosen_radius_m: f64,
    pub impulse: f64,
    pub falloff: f64,
    /// The flung pieces' own falloff: a piece's speed is `impulse` × (1 − d / `radius_m`)^this over
    /// its mass (`fling_speed`; 0: every piece the same speed, so neighbours fly together rather
    /// than the inner ramming the outer). Players and bodies keep `falloff`.
    pub fling_falloff: f64,
    pub lift: f64,
    pub max_speed_ms: f64,
    pub reach_m: f64,
    pub knockback_ms: f64,
    pub damage: f64,
    pub water_radius_m: f64,
    /// Waves (docs/EXPLOSIVES.md §3.5): water within `wave_reach_m` is kicked outward,
    /// `wave_impulse` (m/s) × its depth × (1 − d / reach) of discharge (0: no wave); held water
    /// within `wave_free_m` is freed so the wave runs to the banks.
    pub wave_reach_m: f64,
    pub wave_impulse: f64,
    pub wave_free_m: f64,
    /// For this long (s) a blast's pieces don't touch one another; after it each touches the
    /// others again once it overlaps none.
    pub no_piece_contact_s: f64,
    /// The open side is sensed from the cells within it (m) of the charge (`open_dir`).
    pub open_sense_m: f64,
    /// How far (0–1, times the openness) a piece's push turns from its line toward the open side.
    pub open_blend: f64,
    /// A shell cell whose push meets rock within it (m) stays as terrain (`launching`; 0: all fly).
    pub open_path_m: f64,
    /// The least share (0–1) of a piece's push that goes sideways across the way out, along its own
    /// line's sideways part (`piece_dir`): a flush charge's pieces fly out in a cone, clear of the
    /// crater, not straight up and back in (0: as blended).
    pub open_spread_min: f64,
    pub chain_delay_seconds: [f64; 2],
    pub effects: BlastEffects,
}

// Included (not a `#[path]` module) so rustfmt leaves the generated file as written.
mod explosives_generated {
    use super::*;
    include!("explosives.gen.rs");
}
pub use explosives_generated::{EXPLOSIVES, LIMITS};

/// The block a state belongs to (its name without properties).
fn block_name(m: StateId) -> Option<&'static str> {
    let name = try_info(m)?.name;
    Some(name.split('[').next().unwrap_or(name))
}

/// Which explosive of `table` state `m` is, and whether it is lit.
pub fn explosive_of(table: &[ExplosiveData], m: StateId) -> Option<(usize, bool)> {
    let name = block_name(m)?;
    table.iter().enumerate().find_map(|(i, e)| {
        if e.block == name {
            Some((i, false))
        } else if e.lit == name {
            Some((i, true))
        } else {
            None
        }
    })
}

/// The lit state of explosive `e` (None if the registry lacks it).
pub fn lit_state(e: &ExplosiveData) -> Option<StateId> {
    dwell_blocks::state(e.lit)
}

/// What a Place of `material` aimed at `target` does to an explosive (docs/EXPLOSIVES.md §2): its
/// lit state when `target` is an unlit explosive and `material`'s block lights it (a torch held
/// against it), else None (an ordinary place).
pub fn ignites(table: &[ExplosiveData], target: StateId, material: StateId) -> Option<StateId> {
    let (i, lit) = explosive_of(table, target)?;
    let by = block_name(material)?;
    (!lit && table[i].ignited_by.contains(&by))
        .then(|| lit_state(&table[i]))
        .flatten()
}

/// The share (0–1) of a blast's force at `d` m from it, reaching `reach` m: (1 − d / reach)^falloff.
pub fn share(d: f64, reach: f64, falloff: f64) -> f64 {
    if reach <= 0.0 || d >= reach {
        return 0.0;
    }
    (1.0 - d.max(0.0) / reach).powf(falloff)
}

/// Whether blast `e` destroys a block `d` m from it outright (within `destroy_radius_m`) rather
/// than flinging it as a piece.
pub fn destroys(e: &ExplosiveData, d: f64) -> bool {
    d <= e.destroy_radius_m
}

/// The damage blast `e` does a player `d` m from it (to its body's nearest point): `damage` ×
/// (1 − d / `destroy_radius_m`)^`falloff`, nothing past the destroy radius (owner, 2026-11-06).
pub fn blast_damage(e: &ExplosiveData, d: f64) -> f64 {
    e.damage * share(d, e.destroy_radius_m, e.falloff)
}

/// The cell offsets (x, rows, z) whose centres lie within `radius` m of a cell's centre (rows are
/// `CELL_H` tall), nearest first.
pub fn cells_within(radius: f64) -> Vec<[i32; 3]> {
    let r = radius.floor() as i32;
    let rows = (radius / CELL_H).floor() as i32;
    let mut out = Vec::new();
    for dy in -rows..=rows {
        for dz in -r..=r {
            for dx in -r..=r {
                let d = offset_m([dx, dy, dz]);
                if length(d) <= radius {
                    out.push([dx, dy, dz]);
                }
            }
        }
    }
    out.sort_by(|a, b| length(offset_m(*a)).total_cmp(&length(offset_m(*b))));
    out
}

fn offset_m(o: [i32; 3]) -> [f64; 3] {
    [f64::from(o[0]), f64::from(o[1]) * CELL_H, f64::from(o[2])]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The way a blast at `centre` pushes what is at `at` (world m): out along the line from it, lifted
/// toward the face's up by `lift` (a block level with the blast still flies up out of the crater),
/// a unit vector. Straight up where the two meet.
pub fn push_dir(centre: [f64; 3], at: [f64; 3], lift: f64) -> [f64; 3] {
    let up = FaceFrame::at_height(centre[1]).vector([0.0, 1.0, 0.0]);
    let d = [at[0] - centre[0], at[1] - centre[1], at[2] - centre[2]];
    let n = length(d);
    let out = if n > 1e-6 { d.map(|v| v / n) } else { up };
    let v = [
        out[0] + up[0] * lift,
        out[1] + up[1] * lift,
        out[2] + up[2] * lift,
    ];
    let n = length(v);
    if n > 1e-6 { v.map(|c| c / n) } else { up }
}

/// The way out of a blast (docs/EXPLOSIVES.md §3): the mean, over the cells within `sense_m` of the
/// charge's cell (offsets, rows `CELL_H` tall; the charge's own left out), of the unit direction to
/// each open cell less that to each solid one (`solid(offset)`), doubled and at most 1 long, so a
/// half space open (flat ground, a cliff face, a cave roof) gives its normal at length 1 and a
/// buried charge about nothing. Computed from cells, so it is face-free: on face B the ground's
/// cells are mirrored and so is the way out.
pub fn open_dir(sense_m: f64, mut solid: impl FnMut([i32; 3]) -> bool) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for o in cells_within(sense_m).into_iter().skip(1) {
        let d = offset_m(o);
        let l = length(d);
        let sign = if solid(o) { -1.0 } else { 1.0 };
        for k in 0..3 {
            sum[k] += sign * d[k] / l;
        }
        n += 1.0;
    }
    if n == 0.0 {
        return [0.0; 3];
    }
    let v = sum.map(|c| 2.0 * c / n);
    let l = length(v);
    if l > 1.0 { v.map(|c| c / l) } else { v }
}

/// A flung piece's push: its unit line from the blast `radial` turned toward the way out `open`
/// (`open_dir`) by `blend` × its openness (its length), a unit vector; a buried charge's stays
/// radial. A push going out (along the way out) keeps at least `spread` of it sideways, along the
/// radial's part across the way out (owner, 2026-11-06: a flush charge's debris "does fall right
/// back in"): a cone of ejecta, not a column. A piece straight over the charge (no sideways part)
/// keeps its push.
pub fn piece_dir(radial: [f64; 3], open: [f64; 3], blend: f64, spread: f64) -> [f64; 3] {
    let openness = length(open);
    if openness < 1e-9 {
        return radial;
    }
    let up = open.map(|c| c / openness);
    let b = (blend * openness).clamp(0.0, 1.0);
    let v: [f64; 3] = std::array::from_fn(|k| (1.0 - b) * radial[k] + b * up[k]);
    let l = length(v);
    if l <= 1e-9 {
        return up;
    }
    let v = v.map(|c| c / l);
    let along = dot(v, up);
    let s = spread.clamp(0.0, 1.0);
    let ra = dot(radial, up);
    let side: [f64; 3] = std::array::from_fn(|k| radial[k] - ra * up[k]);
    let sl = length(side);
    let across = (1.0 - along * along).max(0.0).sqrt();
    if along <= 0.0 || across >= s || sl < 1e-6 {
        return v;
    }
    let a = (1.0 - s * s).sqrt();
    std::array::from_fn(|k| s * side[k] / sl + a * up[k])
}

/// Which of a blast's shell cells fly (docs/EXPLOSIVES.md §3; owner, 2026-11-06: "don't simulate
/// what can't move", and the rock in the way "would need to exclude the pieces that are about to be
/// thrown"). `shell`: each candidate's offset from the charge's cell, its push (unit), and whether it
/// flies whatever (a loosened cell). Each cell's path is its push out to `path_m`, ending where it
/// reaches open space (`solid` false: air, liquid, the cleared core); it flies if the path meets
/// no rock (`solid`, not a shell cell) and every shell cell it passes flies. Spread from the
/// opening until nothing changes, so the result does not depend on the order: a cell whose path
/// meets rock, or a shell cell left standing, stays as terrain. `path_m` 0: all fly.
pub fn launching(
    shell: &[([i32; 3], [f64; 3], bool)],
    path_m: f64,
    mut solid: impl FnMut([i32; 3]) -> bool,
) -> Vec<bool> {
    if path_m <= 0.0 {
        return vec![true; shell.len()];
    }
    let index: std::collections::HashMap<[i32; 3], usize> =
        shell.iter().enumerate().map(|(i, s)| (s.0, i)).collect();
    // Each cell's path up to open space: the shell cells it passes (it flies once all of them
    // do), or None if rock comes first.
    let step = 0.05;
    let paths: Vec<Option<Vec<usize>>> = shell
        .iter()
        .map(|&(o, dir, always)| {
            if always {
                return Some(Vec::new());
            }
            let start = offset_m(o);
            let mut seen = vec![o];
            let mut via = Vec::new();
            let mut t = step;
            while t <= path_m + 1e-9 {
                let q: [f64; 3] = std::array::from_fn(|k| start[k] + dir[k] * t);
                let c = [
                    q[0].round() as i32,
                    (q[1] / CELL_H).round() as i32,
                    q[2].round() as i32,
                ];
                t += step;
                if seen.contains(&c) {
                    continue;
                }
                seen.push(c);
                if let Some(&j) = index.get(&c) {
                    via.push(j);
                } else if solid(c) {
                    return None;
                } else {
                    break;
                }
            }
            Some(via)
        })
        .collect();
    let mut flies = vec![false; shell.len()];
    loop {
        let mut changed = false;
        for i in 0..shell.len() {
            if !flies[i]
                && let Some(via) = &paths[i]
                && via.iter().all(|&j| flies[j])
            {
                flies[i] = true;
                changed = true;
            }
        }
        if !changed {
            return flies;
        }
    }
}

/// The unit line from a blast at `centre` to `at` (world m); the face's up where they meet.
pub fn radial_dir(centre: [f64; 3], at: [f64; 3]) -> [f64; 3] {
    push_dir(centre, at, 0.0)
}

/// The speed (m/s) blast `e` gives a block of state `m` at `d` m from it: its impulse times the
/// falloff share over the cell's mass (½ m³ × its density), at most `max_speed_ms`.
pub fn fling_speed(e: &ExplosiveData, m: StateId, d: f64) -> f64 {
    let mass = (f64::from(info(m).density_kg_m3) * CELL_H).max(1.0);
    (e.impulse * share(d, e.radius_m, e.fling_falloff) / mass).min(e.max_speed_ms)
}

/// A chained explosive's fuse (s): drawn from `chain_delay_seconds` by a hash of its cell and `salt`
/// (the tick), so a run is the same every time.
pub fn chain_delay(e: &ExplosiveData, cell: [i32; 3], salt: u32) -> f64 {
    let mut h = (cell[0] as u32).wrapping_mul(0x9e37_79b1)
        ^ (cell[1] as u32).wrapping_mul(0x85eb_ca77)
        ^ (cell[2] as u32).wrapping_mul(0xc2b2_ae3d)
        ^ salt.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    let t = f64::from(h) / f64::from(u32::MAX);
    let [lo, hi] = e.chain_delay_seconds;
    lo + (hi - lo) * t
}

/// How loud blast `e`'s boom is at `d` m (0–1): full within `full_m`, falling to nothing at `hear_m`
/// (as 1 / distance would, eased to zero at the end).
pub fn boom_level(e: &ExplosiveData, d: f64) -> f64 {
    let b = e.effects.boom;
    if d <= b.full_m {
        return 1.0;
    }
    if d >= b.hear_m {
        return 0.0;
    }
    let t = (d - b.full_m) / (b.hear_m - b.full_m);
    (b.full_m.max(1.0) / d).sqrt() * (1.0 - t)
}

/// How hard the camera shakes at `d` m from blast `e` (0–1).
pub fn shake_level(e: &ExplosiveData, d: f64) -> f64 {
    share(d, e.effects.shake.radius_m, 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;

    fn dynamite() -> &'static ExplosiveData {
        EXPLOSIVES
            .iter()
            .find(|e| e.name == "dwell:dynamite")
            .expect("dynamite in the table")
    }

    #[test]
    fn the_tables_blocks_are_in_the_registry() {
        for e in EXPLOSIVES {
            assert!(dwell_blocks::state(e.block).is_some(), "{}", e.block);
            assert!(lit_state(e).is_some(), "{}", e.lit);
            for b in e.ignited_by {
                assert!(dwell_blocks::state(b).is_some(), "{b}");
            }
        }
    }

    #[test]
    fn a_torch_lights_dynamite_and_nothing_else_does() {
        let (dyn_, lit, torch) = (
            require("dwell:dynamite"),
            require("dwell:lit_dynamite"),
            require("dwell:torch"),
        );
        assert_eq!(ignites(EXPLOSIVES, dyn_, torch), Some(lit));
        assert_eq!(ignites(EXPLOSIVES, dyn_, require("dwell:stone")), None);
        assert_eq!(ignites(EXPLOSIVES, lit, torch), None, "already lit");
        assert_eq!(ignites(EXPLOSIVES, require("dwell:stone"), torch), None);
        assert_eq!(explosive_of(EXPLOSIVES, lit), Some((0, true)));
    }

    #[test]
    fn a_fixture_explosive_by_data_alone_is_lit_by_what_it_names() {
        // A table entry added by data alone: lit by a log held against it.
        let fixture = ExplosiveData {
            name: "test:charge",
            ignited_by: &["dwell:log"],
            radius_m: 2.0,
            ..*dynamite()
        };
        let table = [fixture];
        let log = require("dwell:log[axis=y]");
        let dyn_ = require("dwell:dynamite");
        assert_eq!(
            ignites(&table, dyn_, log),
            Some(require("dwell:lit_dynamite"))
        );
        assert_eq!(ignites(&table, dyn_, require("dwell:torch")), None);
        assert!(cells_within(table[0].radius_m).len() < cells_within(4.0).len());
    }

    #[test]
    fn force_falls_off_with_distance_and_heavy_blocks_move_less() {
        // A falling-off fling by data (dynamite's own flings every piece alike, below).
        let e = &ExplosiveData {
            fling_falloff: 1.5,
            ..*dynamite()
        };
        let (stone, leaves) = (
            require("dwell:stone[flooded=false]"),
            require("dwell:leaves"),
        );
        let near = fling_speed(e, stone, 0.5);
        let mid = fling_speed(e, stone, 2.0);
        let edge = fling_speed(e, stone, 4.9);
        assert!(near > mid && mid > edge, "{near} {mid} {edge}");
        assert!(edge < 0.2, "a stone at the edge barely moves: {edge} m/s");
        assert!(fling_speed(e, leaves, 2.0) > mid, "lighter goes faster");
        assert!(fling_speed(e, leaves, 0.0) <= e.max_speed_ms);
        assert_eq!(share(e.radius_m, e.radius_m, e.falloff), 0.0);
        // Dynamite (owner, 2026-11-06: a flush charge's debris "does fall right back in"): its
        // pieces fly alike within the radius, a stone at ~22 m/s, so the inner don't ram the outer.
        let d = dynamite();
        let (a, b) = (fling_speed(d, stone, 2.1), fling_speed(d, stone, 4.9));
        assert_eq!(a, b);
        assert!((15.0..=30.0).contains(&a), "{a} m/s");
    }

    #[test]
    fn the_cells_of_the_radius_are_the_ball_nearest_first() {
        let cells = cells_within(4.0);
        assert_eq!(cells[0], [0, 0, 0]);
        // A 4 m ball of 1 × ½ × 1 m cells (the felling blast's 513).
        assert_eq!(cells.len(), 513);
        assert!(cells.contains(&[4, 0, 0]) && cells.contains(&[0, 8, 0]));
        assert!(!cells.contains(&[3, 6, 3]));
        // Dynamite's 5 m ball and its 2 m core (0.49.46, under the open side's rules; was 4 m,
        // 513 cells): 1,013 cells, 61 of them destroyed outright, the 952 between flung or left by
        // the open side; nothing loosened past it.
        let e = dynamite();
        assert_eq!((e.radius_m, e.destroy_radius_m), (5.0, 2.0));
        assert_eq!(e.loosen_radius_m, e.radius_m);
        assert_eq!(cells_within(e.radius_m).len(), 1013);
        assert_eq!(cells_within(e.destroy_radius_m).len(), 61);
        for o in cells_within(e.radius_m) {
            let d = length(offset_m(o));
            assert_eq!(destroys(e, d), d <= 2.0, "{o:?}");
        }
    }

    #[test]
    fn players_are_hurt_only_within_the_destroy_radius_and_pushed_to_the_reach() {
        let e = dynamite();
        assert_eq!(blast_damage(e, 0.0), e.damage, "standing on it");
        let (a, b) = (blast_damage(e, 0.5), blast_damage(e, 1.5));
        assert!(a > b && b > 0.0, "{a} {b}");
        for d in [2.0, 3.0, 5.0] {
            assert_eq!(blast_damage(e, d), 0.0, "{d} m: unhurt");
            assert!(share(d, e.reach_m, e.falloff) > 0.0, "{d} m: still pushed");
        }
        // A fixture by data alone: a wider core hurts farther.
        let wide = ExplosiveData {
            destroy_radius_m: 4.0,
            ..*e
        };
        assert!(blast_damage(&wide, 3.0) > 0.0);
    }

    /// World cell offset `o`'s face-local offset (x, rows, z) in face `f`.
    fn local(f: FaceFrame, o: [i32; 3]) -> [i32; 3] {
        let v = f.vector(offset_m(o));
        [
            v[0].round() as i32,
            (v[1] / CELL_H).round() as i32,
            v[2].round() as i32,
        ]
    }

    /// A named face-local fixture (which cells are solid) and the way out it should give.
    type Case = (&'static str, fn([i32; 3]) -> bool, [f64; 3]);

    crate::both_faces!(
        fn the_way_out_is_the_open_sides_normal(f: FaceFrame) {
            let e = dynamite();
            // Face-local fixtures: the solid cells round the charge's (air) cell.
            let cases: [Case; 3] = [
                ("flat ground", |l| l[1] < 0, [0.0, 1.0, 0.0]),
                ("a cliff face", |l| l[0] < 0, [1.0, 0.0, 0.0]),
                ("a cave roof", |l| l[1] > 0, [0.0, -1.0, 0.0]),
            ];
            for (name, solid, want) in cases {
                let w = open_dir(e.open_sense_m, |o| solid(local(f, o)));
                let l = f.vector(w);
                assert!(length(l) > 0.9, "{name}: open {:.2}", length(l));
                assert!(dot(l, want) > 0.95, "{name}: {l:?}");
            }
            let buried = open_dir(e.open_sense_m, |_| true);
            assert!(length(buried) < 1e-9, "buried: {buried:?}");
            // A piece right under a charge on the ground goes up, out of the crater; buried, down.
            let up = f.vector([0.0, 1.0, 0.0]);
            let down = up.map(|c| -c);
            let ground = open_dir(e.open_sense_m, |o| local(f, o)[1] < 0);
            assert!(dot(piece_dir(down, ground, e.open_blend, e.open_spread_min), up) > 0.99);
            assert_eq!(
                piece_dir(down, buried, e.open_blend, e.open_spread_min),
                down
            );
        }
    );

    /// The shell of a charge `e` on flat ground in face `f` (its solid cells past the core), each
    /// with its push.
    fn ground_shell(f: FaceFrame, e: &ExplosiveData) -> Vec<([i32; 3], [f64; 3], bool)> {
        let open = open_dir(e.open_sense_m, |o| local(f, o)[1] < 0);
        cells_within(e.radius_m)
            .into_iter()
            .filter(|&o| length(offset_m(o)) > e.destroy_radius_m && local(f, o)[1] < 0)
            .map(|o| {
                let r = radial_dir([0.0; 3], offset_m(o));
                (
                    o,
                    piece_dir(r, open, e.open_blend, e.open_spread_min),
                    false,
                )
            })
            .collect()
    }

    crate::both_faces!(
        fn a_surface_blasts_upper_shell_flies_whole_and_the_deep_ground_stays(f: FaceFrame) {
            // Owner (2026-11-06): the rock in a piece's way "would need to exclude the pieces that
            // are about to be thrown".
            let e = dynamite();
            let shell = ground_shell(f, e);
            // The ground once the core is cleared.
            let after =
                |o: [i32; 3]| local(f, o)[1] < 0 && length(offset_m(o)) > e.destroy_radius_m;
            let flies = launching(&shell, e.open_path_m, after);
            // Each cell judged alone, the shell's other cells as rock: fewer fly.
            let alone = shell
                .iter()
                .filter(|s| launching(&[**s], e.open_path_m, after)[0])
                .count();
            let n = flies.iter().filter(|&&b| b).count();
            assert!(n > alone, "the flood {n}, each alone {alone}");
            // The upper shell (within 1 m of the surface, but its rim, whose pushes graze the
            // ground past the ball) flies whole.
            for (s, &b) in shell.iter().zip(&flies) {
                let l = local(f, s.0);
                let wide = f64::from(l[0] * l[0] + l[2] * l[2]).sqrt();
                assert!(
                    b || !(l[1] >= -2 && wide <= 2.5),
                    "{:?} (local {l:?}) stayed",
                    s.0
                );
            }
            // With no sideways floor (`open_spread_min` 0) every cell under the crater flies too,
            // pushed up through the cleared core and the cells over it; dynamite's floor turns
            // those near the axis aside, into the crater's wall, where they stay.
            let column = ExplosiveData {
                open_spread_min: 0.0,
                ..*e
            };
            let shell0 = ground_shell(f, &column);
            for (s, &b) in shell0.iter().zip(&launching(&shell0, e.open_path_m, after)) {
                let l = local(f, s.0);
                let wide = f64::from(l[0] * l[0] + l[2] * l[2]).sqrt();
                assert!(b || wide > 1.5, "{:?} (local {l:?}) stayed", s.0);
            }
            // Some ground is left standing: the cells whose push runs into the ground past the ball.
            assert!(n < shell.len(), "{n} of {} fly", shell.len());
            // The order does not matter.
            let mut rev = shell.clone();
            rev.reverse();
            let mut back = launching(&rev, e.open_path_m, after);
            back.reverse();
            assert_eq!(back, flies);
            // A path of 0 flings every cell, as before.
            assert!(launching(&shell, 0.0, after).iter().all(|&b| b));
            println!("{n} of {} fly, {alone} alone", shell.len());
        }
    );

    #[test]
    fn a_buried_charges_shell_stays_as_terrain() {
        // face-a-only: everything is solid, so the faces are the same.
        let e = dynamite();
        let shell: Vec<_> = cells_within(e.radius_m)
            .into_iter()
            .filter(|&o| length(offset_m(o)) > e.destroy_radius_m)
            .map(|o| (o, radial_dir([0.0; 3], offset_m(o)), false))
            .collect();
        let rock = |o: [i32; 3]| length(offset_m(o)) > e.destroy_radius_m;
        assert!(launching(&shell, e.open_path_m, rock).iter().all(|&b| !b));
        // A loosened cell flies whatever.
        let mut loose = shell.clone();
        loose[0].2 = true;
        assert!(launching(&loose, e.open_path_m, rock)[0]);
    }

    crate::both_faces!(
        fn the_push_lifts_toward_the_faces_up(f: FaceFrame) {
            let e = dynamite();
            let c = f.point([0.5, 40.25, 0.5]);
            let at = f.point([3.5, 40.25, 0.5]);
            let d = push_dir(c, at, e.lift);
            let local = f.vector(d);
            assert!(local[0] > 0.5 && local[1] > 0.2, "out and up: {local:?}");
            // Straight above it: straight up.
            let up = f.vector(push_dir(c, c, e.lift));
            assert!((up[1] - 1.0).abs() < 1e-9);
        }
    );

    #[test]
    fn chain_delays_lie_in_the_range_and_differ_by_cell() {
        let e = dynamite();
        let a = chain_delay(e, [1, 2, 3], 7);
        let b = chain_delay(e, [4, 2, 3], 7);
        for t in [a, b] {
            assert!(t >= e.chain_delay_seconds[0] && t <= e.chain_delay_seconds[1]);
        }
        assert_ne!(a, b);
        assert_eq!(a, chain_delay(e, [1, 2, 3], 7));
    }

    #[test]
    fn the_boom_is_quieter_far_away() {
        let e = dynamite();
        assert_eq!(boom_level(e, 2.0), 1.0);
        let (a, b) = (boom_level(e, 30.0), boom_level(e, 120.0));
        assert!(a > b && b > 0.0, "{a} {b}");
        assert_eq!(boom_level(e, e.effects.boom.hear_m), 0.0);
        assert!(shake_level(e, 1.0) > shake_level(e, 20.0));
    }
}
