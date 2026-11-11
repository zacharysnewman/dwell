//! Per-tree leaf tints (ADR 0079, docs/TREE_GROWTH.md §10–11): each species tree carries a tint
//! drawn from its seed by its species' `variety` (`lightness`, `hue`), and each leaf cell takes
//! the tint of the tree that owns it.
//!
//! The generator lists a chunk's (or an LOD section's) trees as [`TreeTint`]s — anchor, placed
//! bounds, tint — and the meshers ask [`owner`] for a cell's. The rule is a pure function of the
//! cell and the trees whose bounds hold it, so two chunks meeting at a border (each listing every
//! tree that reaches its columns) and the LOD's sections give a cell the same tree.
//!
//! Face-local: anchors and bounds in the frame the cells are meshed in (+y up).

/// A tint: the hue shift (turns of the HSL hue wheel) and the lightness factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    pub hue: f32,
    pub lightness: f32,
}

impl Tint {
    /// No change: a placed leaf's, or a species whose `variety` draws none.
    pub const NONE: Tint = Tint {
        hue: 0.0,
        lightness: 1.0,
    };

    pub fn is_none(&self) -> bool {
        self.hue == 0.0 && self.lightness == 1.0
    }

    /// `rgb` (0..1 channels) tinted as the Tree Growth prototype does: its HSL hue turned by
    /// `hue`, then scaled by `lightness`. [`Tint::NONE`] returns `rgb` exactly.
    pub fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        if self.is_none() {
            return rgb;
        }
        let (h, s, l) = to_hsl(rgb);
        let h = (h + f64::from(self.hue)).rem_euclid(1.0);
        from_hsl(h, s, l).map(|c| c * f64::from(self.lightness))
    }

    /// [`Tint::apply`] on a packed `0xRRGGBB` colour (the meshers' look and LOD colours), each
    /// channel rounded and held to 0..=255: the chunk and LOD meshers tint alike.
    pub fn apply_rgb8(&self, rgb: u32) -> u32 {
        if self.is_none() {
            return rgb;
        }
        let c = [16, 8, 0].map(|s| f64::from((rgb >> s) & 0xff) / 255.0);
        let [r, g, b] = self
            .apply(c)
            .map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u32);
        (rgb & 0xff00_0000) | (r << 16) | (g << 8) | b
    }
}

/// A placed tree with a tint: its anchor (the trunk's foot) and its placed cells' bounds
/// (inclusive), face-local world cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeTint {
    pub anchor: [i32; 3],
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub tint: Tint,
}

impl TreeTint {
    pub fn holds(&self, c: [i32; 3]) -> bool {
        (0..3).all(|a| (self.min[a]..=self.max[a]).contains(&c[a]))
    }
}

/// The tree owning cell `c`: of those whose bounds hold it, the one whose anchor is nearest
/// across (x, z), then along y, then the least anchor — independent of the list's order.
pub fn owner(trees: &[TreeTint], c: [i32; 3]) -> Option<&TreeTint> {
    trees.iter().filter(|t| t.holds(c)).min_by_key(|t| {
        let d = |a: usize| i64::from(c[a] - t.anchor[a]);
        (d(0) * d(0) + d(2) * d(2), d(1).abs(), t.anchor)
    })
}

/// The tint of cell `c`: its owning tree's, else [`Tint::NONE`].
pub fn tint_at(trees: &[TreeTint], c: [i32; 3]) -> Tint {
    owner(trees, c).map_or(Tint::NONE, |t| t.tint)
}

fn to_hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(anchor: [i32; 3], r: i32, hue: f32) -> TreeTint {
        TreeTint {
            anchor,
            min: [anchor[0] - r, anchor[1], anchor[2] - r],
            max: [anchor[0] + r, anchor[1] + 12, anchor[2] + r],
            tint: Tint {
                hue,
                lightness: 1.0,
            },
        }
    }

    #[test]
    fn no_tint_leaves_a_colour_exactly() {
        // face-a-only: colours have no up.
        let c = [0.31, 0.52, 0.18];
        assert_eq!(Tint::NONE.apply(c), c);
        let back = from_hsl(to_hsl(c).0, to_hsl(c).1, to_hsl(c).2);
        for a in 0..3 {
            assert!((back[a] - c[a]).abs() < 1e-12, "{back:?} vs {c:?}");
        }
    }

    #[test]
    fn a_tint_turns_the_hue_and_scales_the_lightness() {
        // face-a-only: colours have no up.
        let c = [0.3, 0.5, 0.2];
        let dark = Tint {
            hue: 0.0,
            lightness: 0.8,
        }
        .apply(c);
        for a in 0..3 {
            assert!((dark[a] - c[a] * 0.8).abs() < 1e-6);
        }
        let turned = Tint {
            hue: 0.04,
            lightness: 1.0,
        }
        .apply(c);
        assert!(to_hsl(turned).0 > to_hsl(c).0 + 0.039, "{turned:?}");
    }

    #[test]
    fn the_owner_is_the_nearest_tree_holding_the_cell_whatever_the_order() {
        // face-a-only: the rule is face-local; the meshers' callers pass each face's frame.
        let a = tree([0, 10, 0], 3, 0.01);
        let b = tree([5, 10, 0], 3, 0.02);
        for list in [[a, b], [b, a]] {
            assert_eq!(owner(&list, [2, 12, 0]).map(|t| t.anchor), Some(a.anchor));
            assert_eq!(owner(&list, [3, 12, 0]).map(|t| t.anchor), Some(b.anchor));
            assert_eq!(owner(&list, [-4, 12, 0]), None, "outside every tree");
            assert_eq!(tint_at(&list, [9, 12, 0]), Tint::NONE);
        }
        // Equally near: the least anchor, either way round.
        let c = tree([4, 10, 0], 3, 0.03);
        let d = tree([0, 10, 0], 3, 0.04);
        assert_eq!(owner(&[c, d], [2, 12, 0]).unwrap().anchor, [0, 10, 0]);
        assert_eq!(owner(&[d, c], [2, 12, 0]).unwrap().anchor, [0, 10, 0]);
    }
}
