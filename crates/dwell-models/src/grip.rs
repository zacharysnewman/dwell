//! The hand closed on what it holds (models.json `grips`, docs/CHARACTERS.md §11.10): a grip's
//! turns put on its joints' local rotations, after the clips or the retarget.

use crate::anim::Trs;
use crate::glb::Model;
use crate::{Grip, math};

fn qmul(a: math::Quat, b: math::Quat) -> math::Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Euler XYZ degrees as a quaternion: Rx·Ry·Rz, as characters.json (`dwell_characters::math::euler`).
fn euler(r: [f32; 3]) -> math::Quat {
    let x = math::axis_angle([1.0, 0.0, 0.0], r[0]);
    let y = math::axis_angle([0.0, 1.0, 0.0], r[1]);
    let z = math::axis_angle([0.0, 0.0, 1.0], r[2]);
    qmul(x, qmul(y, z))
}

/// `turns` (an idle pose's, models.json `poses`) on `pose` at weight `w` (0 none, 1 all): each
/// turn's angles scaled by `w`, so a pose blends in and out.
pub fn apply_weighted(model: &Model, turns: &Grip, pose: &mut [Trs], w: f32) {
    let w = w.clamp(0.0, 1.0);
    if w <= 0.0 {
        return;
    }
    for &(joint, rot) in turns.turns {
        if let Some(t) = model.skin.get(joint).and_then(|&n| pose.get_mut(n)) {
            t.r = qmul(t.r, euler(rot.map(|a| a * w)));
        }
    }
}

/// `grip`'s turns on `pose` (every node's local transform).
pub fn apply(model: &Model, grip: &Grip, pose: &mut [Trs]) {
    for &(joint, rot) in grip.turns {
        if let Some(t) = model.skin.get(joint).and_then(|&n| pose.get_mut(n)) {
            t.r = qmul(t.r, euler(rot));
        }
    }
}
