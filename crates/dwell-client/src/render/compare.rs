//! Compare mode's state (docs/RAY_MARCHED_RENDERER.md §5): how the two sides of a frame are
//! shown, and what each side draws. Platform free; the renderer turns it into scissored passes.
//!
//! - **Split**: the left renderer left of a draggable divider, the right one right of it; each
//!   draws only its part (scissor), so the frame costs about one of each.
//! - **Blink**: one side fills the screen; a key swaps them.
//! - **Difference**: both draw everything and a heatmap shows where they disagree (the
//!   `difference_fs` shader): coverage (one hits, the other shows sky: red), view depth (relative
//!   difference over [`DEPTH_TOLERANCE`]: orange), colour (CIE76 ΔE over [`DELTA_E`]: yellow);
//!   elsewhere the left side, darkened. Cell id and face need the renderers to write ids (A3): until
//!   then coverage stands in for them. The pixels counted in each class are read back
//!   ([`DiffCounts`]).

/// Relative view-depth difference over which a pixel counts as different.
pub const DEPTH_TOLERANCE: f32 = 1e-3;
/// Colour difference (CIE76 ΔE, of the displayed sRGB colours) over which a pixel counts as
/// different (about just noticeable; the bench's default too).
pub const DELTA_E: f32 = 2.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareView {
    Split,
    Blink,
    Difference,
}

impl CompareView {
    pub fn name(self) -> &'static str {
        match self {
            CompareView::Split => "split",
            CompareView::Blink => "blink",
            CompareView::Difference => "difference",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "split" => Some(CompareView::Split),
            "blink" => Some(CompareView::Blink),
            "difference" | "diff" => Some(CompareView::Difference),
            _ => None,
        }
    }
}

/// A rectangle of the drawing buffer, in pixels: x, y, width, height.
pub type Rect = [u32; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compare {
    pub view: CompareView,
    /// The split's divider, a share of the width from the left (0–1).
    pub divider: f64,
    /// Blink shows the right side (else the left).
    pub blink_right: bool,
}

impl Default for Compare {
    fn default() -> Self {
        Self {
            view: CompareView::Split,
            divider: 0.5,
            blink_right: false,
        }
    }
}

/// How the composite assembles the frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assemble {
    /// Each side's composite in its rectangle (side 0 left, 1 right).
    Regions(Vec<(usize, Rect)>),
    /// The difference heatmap over the whole screen.
    Difference,
}

impl Compare {
    /// The next view (F7): split → blink → difference → split.
    pub fn cycle(&mut self) {
        self.view = match self.view {
            CompareView::Split => CompareView::Blink,
            CompareView::Blink => CompareView::Difference,
            CompareView::Difference => CompareView::Split,
        };
    }

    /// Swaps the sides shown (F8): in blink, the other side; from another view, into blink on the
    /// right side.
    pub fn blink(&mut self) {
        if self.view == CompareView::Blink {
            self.blink_right = !self.blink_right;
        } else {
            self.view = CompareView::Blink;
            self.blink_right = true;
        }
    }

    /// Moves the divider (a share of the width), kept on screen.
    pub fn set_divider(&mut self, share: f64) {
        if share.is_finite() {
            self.divider = share.clamp(0.0, 1.0);
        }
    }

    /// The divider's column in a drawing buffer `width` wide.
    pub fn divider_px(&self, width: u32) -> u32 {
        ((self.divider * f64::from(width)).round() as u32).min(width)
    }

    /// The part of the screen each side draws (`None`: not drawn this frame).
    pub fn draws(&self, (w, h): (u32, u32)) -> [Option<Rect>; 2] {
        let full = Some([0, 0, w, h]);
        match self.view {
            CompareView::Split => {
                let d = self.divider_px(w);
                [
                    (d > 0).then_some([0, 0, d, h]),
                    (d < w).then_some([d, 0, w - d, h]),
                ]
            }
            CompareView::Blink if self.blink_right => [None, full],
            CompareView::Blink => [full, None],
            CompareView::Difference => [full, full],
        }
    }

    /// How the composite puts the sides together.
    pub fn assemble(&self, size: (u32, u32)) -> Assemble {
        if self.view == CompareView::Difference {
            return Assemble::Difference;
        }
        Assemble::Regions(
            self.draws(size)
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.map(|r| (i, r)))
                .collect(),
        )
    }
}

/// Pixels of a difference frame, and how many disagree in each way (each pixel in its worst
/// class: coverage, then depth, then colour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffCounts {
    pub pixels: u32,
    pub coverage: u32,
    pub depth: u32,
    pub colour: u32,
}

impl DiffCounts {
    pub fn from_raw(raw: [u32; 4]) -> Self {
        Self {
            pixels: raw[0],
            coverage: raw[1],
            depth: raw[2],
            colour: raw[3],
        }
    }
    /// Pixels that differ in any way.
    pub fn differing(&self) -> u32 {
        self.coverage + self.depth + self.colour
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_draws_each_side_only_on_its_side_of_the_divider() {
        let mut c = Compare::default();
        assert_eq!(
            c.draws((100, 50)),
            [Some([0, 0, 50, 50]), Some([50, 0, 50, 50])]
        );
        c.set_divider(0.3);
        assert_eq!(
            c.draws((100, 50)),
            [Some([0, 0, 30, 50]), Some([30, 0, 70, 50])]
        );
        // At an edge one side has nothing to draw (a zero-width scissor is invalid).
        c.set_divider(-2.0);
        assert_eq!(c.draws((100, 50)), [None, Some([0, 0, 100, 50])]);
        c.set_divider(1.0);
        assert_eq!(c.draws((100, 50)), [Some([0, 0, 100, 50]), None]);
        c.set_divider(f64::NAN);
        assert_eq!(c.divider, 1.0, "ignored");
        assert_eq!(
            c.assemble((100, 50)),
            Assemble::Regions(vec![(0, [0, 0, 100, 50])])
        );
    }

    #[test]
    fn blink_shows_one_side_and_swaps() {
        let mut c = Compare::default();
        c.blink();
        assert_eq!(c.view, CompareView::Blink);
        assert_eq!(
            c.draws((8, 8)),
            [None, Some([0, 0, 8, 8])],
            "the other side first"
        );
        c.blink();
        assert_eq!(c.draws((8, 8)), [Some([0, 0, 8, 8]), None]);
        assert_eq!(
            c.assemble((8, 8)),
            Assemble::Regions(vec![(0, [0, 0, 8, 8])])
        );
    }

    #[test]
    fn difference_draws_both_everywhere_and_the_views_cycle() {
        let mut c = Compare::default();
        c.cycle();
        assert_eq!(c.view, CompareView::Blink);
        c.cycle();
        assert_eq!(c.view, CompareView::Difference);
        assert_eq!(c.draws((4, 2)), [Some([0, 0, 4, 2]), Some([0, 0, 4, 2])]);
        assert_eq!(c.assemble((4, 2)), Assemble::Difference);
        c.cycle();
        assert_eq!(c.view, CompareView::Split);
        for v in [
            CompareView::Split,
            CompareView::Blink,
            CompareView::Difference,
        ] {
            assert_eq!(CompareView::parse(v.name()), Some(v));
        }
    }

    #[test]
    fn difference_counts_add_up_their_classes() {
        let d = DiffCounts::from_raw([100, 1, 2, 3]);
        assert_eq!(d.pixels, 100);
        assert_eq!(d.differing(), 6);
        assert_eq!(DiffCounts::default().differing(), 0);
    }
}
