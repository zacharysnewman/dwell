//! The client's side of chopping (docs/adr/0067-stone-axe-and-chopping.md): the held axe's swing
//! direction (Use toggles it), the aimed hit on a wood cell sent as `ChopAction::Swing`, and the
//! server's cuts kept for drawing.

use dwell_protocol::chopping::{Cut, ToolDef};
use dwell_protocol::{ChopAction, CutsMessage};
use dwell_world::{CELL_H, m_of_row};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Chopping {
    /// The server keeps cuts (Welcome's `CHOPPING`).
    pub on: bool,
    /// The swing's plane: vertical, else level (the default: across a standing trunk).
    pub vertical: bool,
    /// When the last swing went (ms).
    last_ms: f64,
    /// The cuts the server sent, by cell.
    pub cuts: HashMap<[i32; 3], Cut>,
}

/// Where a ray from `eye` along `dir` enters cell `cell`'s box: the face (`axis × 2 + positive`,
/// world) and the point; none if it misses or starts inside.
pub fn ray_cell_hit(eye: [f64; 3], dir: [f64; 3], cell: [i32; 3]) -> Option<(u8, [f64; 3])> {
    let lo = [
        f64::from(cell[0]),
        m_of_row(cell[1]),
        f64::from(cell[2]),
    ];
    let hi = [lo[0] + 1.0, lo[1] + CELL_H, lo[2] + 1.0];
    let (mut t_in, mut t_out, mut face) = (f64::NEG_INFINITY, f64::INFINITY, None);
    for a in 0..3 {
        if dir[a].abs() < 1e-12 {
            if eye[a] < lo[a] || eye[a] > hi[a] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo[a] - eye[a]) / dir[a], (hi[a] - eye[a]) / dir[a]);
        let (near, far) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        if near > t_in {
            t_in = near;
            // Entering through the low side means the face whose normal points back: negative.
            face = Some((a * 2) as u8 + u8::from(dir[a] < 0.0));
        }
        t_out = t_out.min(far);
    }
    if t_in > t_out || t_in < 0.0 {
        return None;
    }
    Some((face?, std::array::from_fn(|k| eye[k] + dir[k] * t_in)))
}

impl Chopping {
    /// Use with the axe held: the swing turns between level and vertical.
    pub fn toggle(&mut self) -> bool {
        self.vertical = !self.vertical;
        self.vertical
    }

    /// A swing of `tool` at `cell` from `eye` along `dir`, if the axe is ready again; the message.
    pub fn swing(
        &mut self,
        tool: &ToolDef,
        cell: [i32; 3],
        eye: [f64; 3],
        dir: [f64; 3],
        now_ms: f64,
    ) -> Option<ChopAction> {
        if !self.on || now_ms - self.last_ms < tool.swing_seconds * 1000.0 {
            return None;
        }
        let (face, point) = ray_cell_hit(eye, dir, cell)?;
        self.last_ms = now_ms;
        Some(ChopAction::Swing {
            tool: tool.name.to_owned(),
            cell,
            face,
            point: point.map(|v| v as f32),
            vertical: self.vertical,
        })
    }

    pub fn apply(&mut self, m: CutsMessage) {
        for c in m.gone {
            self.cuts.remove(&c);
        }
        self.cuts.extend(m.cuts);
    }

    /// The hint shown while the axe is held.
    pub fn hint(&self) -> &'static str {
        if self.vertical {
            "Axe: vertical swing (right click: level)"
        } else {
            "Axe: level swing (right click: vertical)"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_enters_a_cell_through_the_face_it_meets() {
        // From −x, level: the −x face (0), at x = 3.
        let (f, p) = ray_cell_hit([0.5, 0.2, 0.5], [1.0, 0.0, 0.0], [3, 0, 0]).unwrap();
        assert_eq!(f, 0);
        assert!((p[0] - 3.0).abs() < 1e-9);
        // From above: the +y face (3), at the row's top.
        let (f, p) = ray_cell_hit([3.5, 5.0, 0.5], [0.0, -1.0, 0.0], [3, 0, 0]).unwrap();
        assert_eq!(f, 3);
        assert!((p[1] - CELL_H).abs() < 1e-9);
        // From below (face B's up): the −y face (2).
        assert_eq!(ray_cell_hit([3.5, -5.0, 0.5], [0.0, 1.0, 0.0], [3, 0, 0]).unwrap().0, 2);
        assert_eq!(ray_cell_hit([0.5, 0.2, 0.5], [-1.0, 0.0, 0.0], [3, 0, 0]), None);
    }

    #[test]
    fn swings_wait_for_the_axe_and_carry_the_direction() {
        let axe = dwell_protocol::chopping::tool("dwell:stone_axe").unwrap();
        let mut c = Chopping { on: true, ..Default::default() };
        let eye = [0.5, 0.2, 0.5];
        let s = c.swing(axe, [3, 0, 0], eye, [1.0, 0.0, 0.0], 1000.0);
        assert!(matches!(s, Some(ChopAction::Swing { vertical: false, face: 0, .. })));
        assert!(c.swing(axe, [3, 0, 0], eye, [1.0, 0.0, 0.0], 1100.0).is_none());
        assert!(c.toggle());
        let s = c.swing(axe, [3, 0, 0], eye, [1.0, 0.0, 0.0], 1500.0);
        assert!(matches!(s, Some(ChopAction::Swing { vertical: true, .. })));
        let off = &mut Chopping::default();
        assert!(off.swing(axe, [3, 0, 0], eye, [1.0, 0.0, 0.0], 0.0).is_none(), "no CHOPPING");
    }
}
