//! Tools and chopping (docs/adr/0067-stone-axe-and-chopping.md), as data
//! (`shared/tools/tools.json`, generated into `tools.gen.rs` by `shared/tools/gen.mjs`).
//!
//! A wood cell (a log) has a **grain** (its `axis`) and four **sides**, the faces across the grain.
//! Each side has an open **notch** (wood cut out, metres deep) and, in each of its two **bands**
//! (halves along the grain), a **kerf**: a cut past the notch whose walls bind the axe. A swing's
//! **share** is how well its plane crosses the grain; it cuts `bite × share` into open wood, less
//! the deeper its kerf already is. When both bands of a side are cut past the notch, the chunk
//! between comes out: the notch deepens to the shallower kerf. So chopping in, then a little above
//! or below, cuts a trunk in about 9 swings, and hacking one spot takes about four times as many.
//! The server keeps a [`Cut`] per wood cell it has been swung at ([`Cuts`]); clients draw it.

use crate::bytes::{ByteReader, ByteWriter};

#[rustfmt::skip]
mod table {
    include!("tools.gen.rs");
}

pub use table::{BREAK_WITH, TOOLS, WOODS};

/// A tool's swing (`swing` in the data): its feel (the client's) and its path (both sides').
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingDef {
    /// Before the arc (the wind-up).
    pub windup_s: f64,
    /// The arc's sweep: the head's path, sampled at `samples` steps.
    pub active_s: f64,
    /// The pause on impact (the hit-stop).
    pub hit_stop_s: f64,
    /// After a hit, and after a miss (a looser follow-through).
    pub recover_hit_s: f64,
    pub recover_miss_s: f64,
    /// The head's distance from the eye (m) and the arc it sweeps (degrees), centred on the look.
    pub reach_m: f64,
    pub arc_deg: f64,
    pub samples: u32,
    /// The camera's jolt on a hit (degrees).
    pub kick_deg: f64,
    /// What a swing will draw from stamina when stamina exists (unused until then).
    pub stamina_cost: f64,
    /// Sound events (shared/sounds): the arc's swish, a bounce off what is not a wood.
    pub swish: Option<&'static str>,
    pub bounce: Option<&'static str>,
    /// The sound event of a swing that cleared blocks on its way (its `clears`), and the leaves
    /// each cleared cell scatters (the client's particles).
    pub rustle: Option<&'static str>,
    pub rustle_leaves: u32,
}

impl SwingDef {
    /// When along the swing (s from its start) the head reaches arc step `step`.
    pub fn time_of_step(&self, step: u32) -> f64 {
        let n = self.samples.max(2) - 1;
        self.windup_s + self.active_s * f64::from(step.min(n)) / f64::from(n)
    }
    /// How long a swing that hit at arc step `step` lasts, and one that missed.
    pub fn hit_length(&self, step: u32) -> f64 {
        self.time_of_step(step) + self.hit_stop_s + self.recover_hit_s
    }
    pub fn miss_length(&self) -> f64 {
        self.windup_s + self.active_s + self.recover_miss_s
    }
}

/// Leaves a hit shakes down from a tree (`leaves` in the data; the client's particles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaves {
    /// The leaf block they fall from (none: no leaves).
    pub block: Option<&'static str>,
    pub count: u32,
    /// sRGB.
    pub color: [u8; 3],
    /// Where leaf blocks are looked for: within this of the hit across, this above it.
    pub radius_m: f64,
    pub height_m: f64,
}

/// A standing tree's sway as it is cut (`sway` in the data; drawn by the client).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sway {
    /// It begins once this much of the cross-section is cut,
    pub from_fraction: f64,
    /// and leans up to this (degrees) at the fell fraction.
    pub max_deg: f64,
}

impl WoodDef {
    /// The sway's lean (degrees) of a standing trunk cut `cross` of the way: none before
    /// `from_fraction`, rising to `max_deg` at the fell fraction (eased in: more toward the fall).
    pub fn sway_deg(&self, cross: f64) -> f64 {
        let span = (self.fell_fraction - self.sway.from_fraction).max(1e-6);
        let u = ((cross - self.sway.from_fraction) / span).clamp(0.0, 1.0);
        self.sway.max_deg * u * u
    }
    /// A hit's sound event at a cut `cross` of the way through: the last stage begun.
    pub fn sound_at(&self, cross: f64) -> Option<&'static str> {
        self.sounds
            .iter()
            .rev()
            .find(|(from, _)| cross >= *from)
            .map(|(_, e)| *e)
    }
}

/// The tool kinds the engine understands (`kind` in the data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    /// Chops the woods.
    Axe,
}

/// A tool (an item, by name) and how it cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolDef {
    pub name: &'static str,
    pub kind: ToolKind,
    /// The item a hand shows holding it (`shared/characters/characters.json`'s `items`).
    pub held: Option<&'static str>,
    /// The least time between two swings.
    pub swing_seconds: f64,
    /// Metres a swing straight across the grain cuts into open wood.
    pub bite_m: f64,
    /// A kerf this deep past the notch halves a swing into it.
    pub bind_m: f64,
    /// The share of a bite with the swing's plane across the grain, and along it.
    pub across: f64,
    pub along: f64,
    /// The share's falloff from across to along: `along + (across − along) |cos θ|^sharpness`.
    pub sharpness: f64,
    /// The item a hand shows holding it for a vertical swing (the second hold).
    pub held_vertical: Option<&'static str>,
    /// The swing: its timing, its arc, its feel.
    pub swing: SwingDef,
    /// The blocks (by name) its swing goes through, breaking each it touches (owner, 2026-11-09:
    /// the axe clears leaves); never a wood.
    pub clears: &'static [&'static str],
}

impl ToolDef {
    /// Whether its swing clears the block of this name on its way.
    pub fn clears_block(&self, block: &str) -> bool {
        self.clears.contains(&block)
    }
    /// The held item for the swing's plane.
    pub fn held_for(&self, vertical: bool) -> Option<&'static str> {
        if vertical {
            self.held_vertical.or(self.held)
        } else {
            self.held
        }
    }
}

/// A block an axe cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodDef {
    pub block: &'static str,
    /// A standing one falls when this much of its cross-section is cut.
    pub fell_fraction: f64,
    /// A lying one is cut through at this much.
    pub buck_fraction: f64,
    /// What a cut-through lying length drops.
    pub buck_drops: &'static [crate::inventory::DropRule],
    /// The chips a swing knocks out of it (the client's particles).
    pub chips: Chips,
    /// The leaves a hit shakes down, the standing tree's sway, each hit's sound by the cut.
    pub leaves: Leaves,
    pub sway: Sway,
    pub sounds: &'static [(f64, &'static str)],
    /// How its cuts are carved into its cell's mesh (the client's, `dwell-mesh`).
    pub cut: CutLook,
    /// The stored-ramp block a felled cell splits into (Phase 55 T2): the stump keeps its ramp
    /// rising away from the notch, the trunk's bottom its upside-down ramp. None: the cell goes.
    pub fell_wedge: Option<&'static str>,
    /// The trunk's upside-down ramp lies parallel to the stump's (`fellWedgeTrunk: matching`)
    /// rather than as the notch's upper face (`open`, the default; `dwell_sim::felling::WedgeTrunk`).
    pub fell_wedge_matching: bool,
}

/// How a wood's cuts are carved into its cell's mesh (`cut` in the data): the notch as a V across
/// the side, each band's kerf past it as a slit, the faces inside fresh wood.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutLook {
    /// The atlas tile of the cut faces (none: the block's end-grain tile).
    pub tile: Option<&'static str>,
    /// A colour multiplying the tile (sRGB).
    pub tint: [u8; 3],
    /// The notch's opening along the grain per metre of its depth.
    pub open: f64,
    /// The kerf slits' width along the grain (m).
    pub kerf_width_m: f64,
}

/// The chips a swing knocks out of a wood (`chips` in the data).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chips {
    pub count: u32,
    /// sRGB.
    pub color: [u8; 3],
    /// Their speed out of the cut (m/s).
    pub speed: f64,
    /// Their size (m).
    pub size: f64,
}

/// The tool kind breaking a block needs where blocks are not unlimited (its `breakWith`), if any.
pub fn break_with(block: &str) -> Option<ToolKind> {
    BREAK_WITH
        .iter()
        .find(|(b, _)| *b == block)
        .map(|(_, k)| *k)
}

/// Whether a break of `block` holding `held` (an item name, none: an empty hand) is allowed where
/// blocks are not unlimited: a block with a `breakWith` needs a tool of that kind.
pub fn may_break_holding(block: &str, held: Option<&str>) -> bool {
    match break_with(block) {
        None => true,
        Some(kind) => held.and_then(tool).is_some_and(|t| t.kind == kind),
    }
}

/// The tool of an item's name.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The wood of a block's name.
pub fn wood(block: &str) -> Option<&'static WoodDef> {
    WOODS.iter().find(|w| w.block == block)
}

/// Sides of a wood cell: the faces across its grain.
pub const SIDES: usize = 4;
/// Bands of a side: its halves along the grain.
pub const BANDS: usize = 2;

/// A cell's size along a world axis (0 x, 1 y, 2 z): ½ m rows, 1 m across.
pub fn extent(axis: usize) -> f64 {
    if axis == 1 { 0.5 } else { 1.0 }
}

/// The two axes across `grain`, in order.
fn across_axes(grain: usize) -> [usize; 2] {
    match grain {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// The side of a cell with grain `grain` that a face with outward normal along `axis` (`positive`)
/// is; none for an end-grain face (along the grain).
pub fn side_of(grain: usize, axis: usize, positive: bool) -> Option<usize> {
    let j = across_axes(grain).iter().position(|&a| a == axis)?;
    Some(2 * j + usize::from(positive))
}

/// A side's outward normal (world).
pub fn side_normal(grain: usize, side: usize) -> [i32; 3] {
    let mut n = [0; 3];
    n[across_axes(grain)[side / 2]] = if side % 2 == 1 { 1 } else { -1 };
    n
}

/// The world axis a side's depth is measured along.
pub fn side_axis(grain: usize, side: usize) -> usize {
    across_axes(grain)[side / 2]
}

/// How well a swing whose plane has normal `normal` cuts wood whose grain is `grain` (unit vectors,
/// any frame both share): `across` when the plane is square to the grain, `along` in line with it.
pub fn share(tool: &ToolDef, normal: [f64; 3], grain: [f64; 3]) -> f64 {
    let c = (normal[0] * grain[0] + normal[1] * grain[1] + normal[2] * grain[2]).abs();
    tool.along + (tool.across - tool.along) * c.min(1.0).powf(tool.sharpness)
}

/// A swing plane's normal, face-local (+y up): a horizontal swing's is up; a vertical swing's is
/// level, square to the look's heading (the plane holds the look and up). A look straight down has
/// no heading: its vertical plane is taken along x.
pub fn swing_normal(vertical: bool, look: [f64; 3]) -> [f64; 3] {
    if !vertical {
        return [0.0, 1.0, 0.0];
    }
    // look × up = (−look.z, 0, look.x), level.
    let (x, z) = (-look[2], look[0]);
    let l = (x * x + z * z).sqrt();
    if l < 1e-6 {
        [0.0, 0.0, 1.0]
    } else {
        [x / l, 0.0, z / l]
    }
}

/// The cut in one wood cell (metres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    /// Open wood cut out of each side.
    pub notch: [f32; SIDES],
    /// Each side's band's cut past its notch.
    pub kerf: [[f32; BANDS]; SIDES],
}

impl Cut {
    /// How deep a side is cut (its notch and its deepest kerf), metres.
    pub fn depth(&self, side: usize) -> f64 {
        f64::from(self.notch[side] + self.kerf[side][0].max(self.kerf[side][1]))
    }

    /// The share of the cross-section cut (0..1): the most of any two opposite sides together,
    /// over the cell's size across them.
    pub fn cross(&self, grain: usize) -> f64 {
        (0..2)
            .map(|j| {
                let e = extent(across_axes(grain)[j]);
                ((self.depth(2 * j) + self.depth(2 * j + 1)) / e).min(1.0)
            })
            .fold(0.0, f64::max)
    }

    /// The side cut deepest (its share of the size across it); none when uncut.
    pub fn deepest(&self, grain: usize) -> Option<usize> {
        (0..SIDES)
            .filter(|&s| self.depth(s) > 0.0)
            .max_by(|&a, &b| {
                let da = self.depth(a) / extent(side_axis(grain, a));
                let db = self.depth(b) / extent(side_axis(grain, b));
                da.total_cmp(&db)
            })
    }

    pub fn is_empty(&self) -> bool {
        *self == Cut::default()
    }

    /// One swing of `tool` with `share` (`share`) into band `band` of side `side`: the kerf there
    /// deepens by `bite × share / (1 + kerf / bind)`, at most to the far side; when both bands are
    /// cut past the notch the chunk between comes out. Returns the metres cut.
    pub fn swing(&mut self, tool: &ToolDef, grain: usize, side: usize, band: usize, s: f64) -> f64 {
        let e = extent(side_axis(grain, side));
        let k = f64::from(self.kerf[side][band]);
        let room = (e - f64::from(self.notch[side]) - k).max(0.0);
        let gain = (tool.bite_m * s / (1.0 + k / tool.bind_m)).min(room);
        self.kerf[side][band] = (k + gain) as f32;
        let m = self.kerf[side][0].min(self.kerf[side][1]);
        if m > 0.0 {
            self.notch[side] += m;
            self.kerf[side][0] -= m;
            self.kerf[side][1] -= m;
        }
        gain
    }

    pub fn write(&self, w: &mut ByteWriter) {
        for s in 0..SIDES {
            w.f32(self.notch[s]);
            w.f32(self.kerf[s][0]);
            w.f32(self.kerf[s][1]);
        }
    }

    pub fn read(r: &mut ByteReader) -> Cut {
        let mut c = Cut::default();
        for s in 0..SIDES {
            c.notch[s] = r.f32();
            c.kerf[s][0] = r.f32();
            c.kerf[s][1] = r.f32();
        }
        c
    }
}

/// The cuts in a world: wood cells swung at, by cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cuts {
    pub cells: std::collections::HashMap<[i32; 3], Cut>,
}

/// The save record's version.
const CUTS_VERSION: u8 = 1;

impl Cuts {
    /// The cells in a stable order (saves and messages).
    pub fn sorted(&self) -> Vec<([i32; 3], Cut)> {
        let mut v: Vec<_> = self.cells.iter().map(|(&c, &k)| (c, k)).collect();
        v.sort_by_key(|(c, _)| *c);
        v
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = ByteWriter::new(&mut out);
        w.u8(CUTS_VERSION);
        let all = self.sorted();
        w.u32(all.len() as u32);
        for (c, k) in all {
            for v in c {
                w.i32(v);
            }
            k.write(&mut w);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Cuts> {
        let mut r = ByteReader::new(bytes);
        if r.u8() != CUTS_VERSION {
            return None;
        }
        let n = r.u32() as usize;
        let mut cells = std::collections::HashMap::new();
        for _ in 0..n.min(bytes.len() / 60 + 1) {
            let c = [r.i32(), r.i32(), r.i32()];
            cells.insert(c, Cut::read(&mut r));
        }
        (r.ok() && r.at_end()).then_some(Cuts { cells })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axe() -> &'static ToolDef {
        tool("dwell:stone_axe").expect("the stone axe is data")
    }
    const UP: [f64; 3] = [0.0, 1.0, 0.0];
    const Y: usize = 1;

    #[test]
    fn a_hit_stops_short_and_a_miss_follows_through_longer() {
        let s = axe().swing;
        // A hit early on the arc ends sooner than one at its end, and every hit sooner than a miss.
        assert!(s.hit_length(0) < s.hit_length(s.samples - 1));
        assert!(s.hit_length(s.samples - 1) < s.miss_length());
        assert!(
            (s.time_of_step(0) - s.windup_s).abs() < 1e-12,
            "the arc starts after the wind-up"
        );
        assert!(s.hit_stop_s > 0.0 && s.recover_miss_s > s.recover_hit_s);
        // Never sooner than the server's wait.
        assert!(s.miss_length() >= axe().swing_seconds);
        assert!(
            s.stamina_cost > 0.0,
            "a swing names its stamina cost (unused until stamina)"
        );
    }

    #[test]
    fn the_tree_sways_more_toward_the_fall_and_the_hits_sound_deeper() {
        let log = wood("dwell:log").unwrap();
        assert_eq!(log.sway_deg(0.0), 0.0);
        assert_eq!(
            log.sway_deg(log.sway.from_fraction),
            0.0,
            "still until ~40% through"
        );
        let mid = log.sway_deg((log.sway.from_fraction + log.fell_fraction) / 2.0);
        let end = log.sway_deg(log.fell_fraction);
        assert!(
            mid > 0.0 && end > 2.0 * mid,
            "more toward the fall: {mid} then {end}"
        );
        assert!((end - log.sway.max_deg).abs() < 1e-9);
        assert_eq!(log.sound_at(0.0), Some("chop_bark"));
        assert_eq!(log.sound_at(0.3), Some("chop_wood"));
        assert_eq!(log.sound_at(0.5), Some("chop_creak"));
    }

    /// Swings at a standing (grain y) 1 m trunk's −x side, `band_of(i)` for swing i, with
    /// `share` each, until the cut passes the log's fell fraction; the count.
    fn swings_to_fell(s: f64, band_of: impl Fn(usize) -> usize) -> usize {
        let fell = wood("dwell:log").expect("log").fell_fraction;
        let mut c = Cut::default();
        for i in 0..1000 {
            c.swing(axe(), Y, 0, band_of(i), s);
            if c.cross(Y) >= fell {
                return i + 1;
            }
        }
        panic!("never fell");
    }

    #[test]
    fn a_horizontal_swing_crosses_a_standing_trunk_and_a_vertical_one_splits_it() {
        let h = share(axe(), swing_normal(false, [1.0, 0.0, 0.0]), UP);
        let v = share(axe(), swing_normal(true, [1.0, 0.0, 0.0]), UP);
        assert_eq!(h, axe().across);
        assert_eq!(v, axe().along);
        // On a log lying along x, looked at from the side (+z): vertical crosses, horizontal splits.
        let x = [1.0, 0.0, 0.0];
        assert_eq!(
            share(axe(), swing_normal(true, [0.0, -0.5, 1.0]), x),
            axe().across
        );
        assert_eq!(
            share(axe(), swing_normal(false, [0.0, -0.5, 1.0]), x),
            axe().along
        );
        // Looked at along its length, a vertical swing splits it too.
        assert!(share(axe(), swing_normal(true, [1.0, -0.3, 0.0]), x) < 0.2);
        // Leaning at 45°: between.
        let lean = [
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ];
        let m = share(axe(), swing_normal(false, [1.0, 0.0, 0.0]), lean);
        assert!(m > axe().along && m < axe().across, "{m}");
    }

    #[test]
    fn notching_fells_in_eight_to_twelve_swings_and_hacking_takes_three_to_four_times_more() {
        let notch = swings_to_fell(1.0, |i| i % 2);
        let hack = swings_to_fell(1.0, |_| 0);
        assert!((8..=12).contains(&notch), "notching: {notch} swings");
        let ratio = hack as f64 / notch as f64;
        assert!(
            (3.0..=4.0).contains(&ratio),
            "hacking: {hack} swings, {ratio:.2}× notching"
        );
        // Vertical swings at a standing trunk barely cut it.
        let split = swings_to_fell(axe().along, |i| i % 2);
        assert!(split >= 5 * notch, "vertical: {split} swings");
    }

    #[test]
    fn a_chunk_comes_out_when_both_bands_are_cut() {
        let mut c = Cut::default();
        c.swing(axe(), Y, 0, 0, 1.0);
        assert_eq!(c.notch[0], 0.0);
        assert!(c.kerf[0][0] > 0.1);
        c.swing(axe(), Y, 0, 1, 1.0);
        assert!((c.notch[0] - 0.12).abs() < 1e-6, "{c:?}");
        assert_eq!(c.kerf[0], [0.0, 0.0]);
        assert_eq!(c.deepest(Y), Some(0));
        // A cut never goes past the far side.
        for i in 0..400 {
            c.swing(axe(), Y, 0, i % 2, 1.0);
        }
        assert!(c.depth(0) <= 1.0 + 1e-6);
        assert_eq!(c.cross(Y), 1.0);
    }

    #[test]
    fn sides_are_the_faces_across_the_grain() {
        assert_eq!(
            side_of(1, 1, true),
            None,
            "a standing trunk's top is end grain"
        );
        assert_eq!(side_of(1, 0, false), Some(0));
        assert_eq!(side_normal(1, 3), [0, 0, 1]);
        assert_eq!(side_of(0, 1, true), Some(1), "a log along x: its top");
        assert_eq!(side_axis(0, 1), 1);
        for g in 0..3 {
            for s in 0..SIDES {
                let n = side_normal(g, s);
                let a = n.iter().position(|&v| v != 0).unwrap();
                assert_eq!(side_of(g, a, n[a] > 0), Some(s));
            }
        }
    }

    #[test]
    fn the_new_messages_round_trip_and_reject_bad_ones() {
        use crate::{ChopAction, CutsMessage, Message};
        let mut c = Cut::default();
        c.swing(axe(), Y, 1, 0, 1.0);
        let all = [
            Message::Chop(ChopAction::Hello),
            Message::Chop(ChopAction::Swing {
                tool: "dwell:stone_axe".into(),
                cell: [4, -80, 12],
                face: 1,
                point: [5.0, -39.8, 12.5],
                vertical: true,
            }),
            Message::Cuts(CutsMessage {
                cuts: vec![([4, -80, 12], c)],
                gone: vec![[1, 2, 3]],
            }),
            // The swing's path and the held tool (CHOP_SWEEP).
            Message::Chop(ChopAction::Sweep {
                tool: "dwell:stone_axe".into(),
                look: [0.6, -0.8, 0.0],
                vertical: false,
            }),
            Message::Chop(ChopAction::Held {
                tool: "dwell:stone_axe".into(),
                vertical: true,
            }),
            Message::HeldTools(vec![
                crate::HeldTool {
                    player_id: 3,
                    tool: "dwell:stone_axe".into(),
                    vertical: false,
                },
                crate::HeldTool {
                    player_id: 9,
                    tool: String::new(),
                    vertical: false,
                },
            ]),
        ];
        for m in all {
            let b = m.encode();
            assert_eq!(Message::decode(&b), Some(m.clone()));
            assert_eq!(Message::decode(&b[..b.len() - 1]), None, "{m:?}");
        }
        let mut bad = Message::Chop(ChopAction::Hello).encode();
        *bad.last_mut().unwrap() = 9;
        assert_eq!(Message::decode(&bad), None);
    }

    #[test]
    fn cuts_round_trip_and_bad_records_are_refused() {
        let mut cuts = Cuts::default();
        let mut c = Cut::default();
        c.swing(axe(), Y, 2, 1, 1.0);
        cuts.cells.insert([3, -40, 7], c);
        cuts.cells.insert([-1, 2, 0], Cut::default());
        let b = cuts.to_bytes();
        assert_eq!(Cuts::from_bytes(&b), Some(cuts));
        assert_eq!(Cuts::from_bytes(&b[..b.len() - 1]), None);
        let mut v = b.clone();
        v[0] = 9;
        assert_eq!(Cuts::from_bytes(&v), None);
    }
}
