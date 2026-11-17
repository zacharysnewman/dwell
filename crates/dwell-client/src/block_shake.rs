//! A shaking block (docs/BLOCK_DAMAGE.md §17; owner, 2026-11-17: "Door, ladder, dynamite should
//! shake, not break"): a hit short of breaking a block whose `damage.hit` is `shake` scales it out on
//! x and z in turn for `breaking.shake.seconds`, never in (owner: "as long as we don't get edge
//! holes": a face that only grows can't open a gap beside a neighbour whose face it hid). The box is
//! the struck cell and the cells of the same block straight above and below it (a door's two).
use dwell_blocks::StateId;
use dwell_protocol::chopping::Shake;

use crate::render::BlockShake;

/// The most cells a shaking block spans up and down from the struck one.
const MAX_REACH: i32 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShakeState {
    /// The box's corners (world m) and when the hit landed (ms).
    on: Option<([f64; 3], [f64; 3], f64)>,
}

impl ShakeState {
    /// A hit on `cell` (holding `block`) at `now_ms`: it shakes if the data says so. `voxel` reads
    /// the world's cells; `row_m` is a row's bottom (m).
    pub fn hit(
        &mut self,
        cell: [i32; 3],
        block: StateId,
        now_ms: f64,
        mut voxel: impl FnMut(i32, i32, i32) -> StateId,
        row_m: impl Fn(i32) -> f64,
    ) {
        if dwell_blocks::try_info(block).is_none() {
            return;
        }
        let name = dwell_blocks::block_of(block).name;
        if !dwell_protocol::damage::shakes(name) {
            return;
        }
        let [x, y, z] = cell;
        let mut same = |yy: i32| dwell_blocks::block_of(voxel(x, yy, z)).name == name;
        let (mut lo, mut hi) = (y, y);
        while lo > y - MAX_REACH && same(lo - 1) {
            lo -= 1;
        }
        while hi < y + MAX_REACH && same(hi + 1) {
            hi += 1;
        }
        self.on = Some((
            [f64::from(x), row_m(lo), f64::from(z)],
            [f64::from(x) + 1.0, row_m(hi + 1), f64::from(z) + 1.0],
            now_ms,
        ));
    }

    /// The shake to draw at `now_ms` under `shake`'s timing; none once it is over.
    pub fn frame(&mut self, shake: &Shake, now_ms: f64) -> Option<BlockShake> {
        let (lo, hi, t0) = self.on?;
        let Some(o) = shake.offset_at((now_ms - t0) / 1000.0) else {
            self.on = None;
            return None;
        };
        let half = |k: usize| (0.5 * (hi[k] - lo[k])).max(1e-3);
        Some(BlockShake {
            lo,
            hi,
            // Out on x while the swing is positive, on z while negative: never below 1.
            scale: [1.0 + o.max(0.0) / half(0), 1.0 + (-o).max(0.0) / half(2)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::chopping::SHAKE;

    fn state(name: &str) -> StateId {
        dwell_blocks::parse_state(name).expect("a block of the build")
    }

    #[test]
    fn a_door_hit_shakes_both_its_cells_outward_only_and_stops() {
        // face-a-only: the box is world cells and rows; the shader's test is face-free.
        let door = state("dwell:door");
        let air = state("dwell:air");
        let voxel = |_x: i32, y: i32, _z: i32| if (10..=11).contains(&y) { door } else { air };
        let row = |y: i32| f64::from(y) * 0.5;
        let mut s = ShakeState::default();
        s.hit([3, 10, -2], door, 1000.0, voxel, row);
        let mut seen_x = false;
        let mut seen_z = false;
        for i in 0..40 {
            let t = 1000.0 + f64::from(i) * 5.0;
            let b = s.frame(&SHAKE, t).expect("shaking");
            assert_eq!(b.lo, [3.0, 5.0, -2.0]);
            assert_eq!(b.hi, [4.0, 6.0, -1.0], "both of the door's cells");
            assert!(
                b.scale.iter().all(|&k| k >= 1.0),
                "never inward: {:?}",
                b.scale
            );
            seen_x |= b.scale[0] > 1.0;
            seen_z |= b.scale[1] > 1.0;
        }
        assert!(seen_x && seen_z, "out on x and on z in turn");
        assert!(
            s.frame(&SHAKE, 1000.0 + SHAKE.seconds * 1000.0 + 1.0)
                .is_none(),
            "over"
        );
        assert!(s.frame(&SHAKE, 5000.0).is_none());
    }

    #[test]
    fn a_block_that_takes_marks_does_not_shake() {
        let stone = state("dwell:stone");
        let mut s = ShakeState::default();
        s.hit(
            [0, 0, 0],
            stone,
            0.0,
            |_, _, _| stone,
            |y| f64::from(y) * 0.5,
        );
        assert!(s.frame(&SHAKE, 10.0).is_none());
        for b in ["dwell:ladder", "dwell:dynamite"] {
            let id = state(b);
            let mut s = ShakeState::default();
            s.hit(
                [0, 0, 0],
                id,
                0.0,
                |_, _, _| state("dwell:air"),
                |y| f64::from(y) * 0.5,
            );
            assert!(s.frame(&SHAKE, 10.0).is_some(), "{b}");
        }
    }
}
