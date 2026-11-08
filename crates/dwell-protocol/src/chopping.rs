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
