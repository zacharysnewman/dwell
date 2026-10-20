//! Procedural line geometry the renderer draws itself: the block outline's box edges. (Players are
//! drawn as characters: characters.rs.)

/// The twelve edges of the unit box [0, 1]³ (line-list positions), slightly enlarged as the
/// outline was (1.004 about the cell).
pub fn box_edges() -> Vec<[f32; 3]> {
    box_edges_about([0.0; 3], [1.004; 3])
}

/// The twelve edges of the box [lo, hi] (line-list positions).
pub fn box_edges_about(lo: [f32; 3], hi: [f32; 3]) -> Vec<[f32; 3]> {
    let c = |i: u32| {
        [
            if i & 1 != 0 { hi[0] } else { lo[0] },
            if i & 2 != 0 { hi[1] } else { lo[1] },
            if i & 4 != 0 { hi[2] } else { lo[2] },
        ]
    };
    let mut out = Vec::new();
    for a in 0..8u32 {
        for bit in [1, 2, 4] {
            if a & bit == 0 {
                out.push(c(a));
                out.push(c(a | bit));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_has_twelve_edges() {
        assert_eq!(box_edges().len(), 24);
    }
}
