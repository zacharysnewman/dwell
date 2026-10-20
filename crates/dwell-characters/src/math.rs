//! Rigid transforms (f32): Euler XYZ in degrees as three.js applies them (rig.mjs `rotate`).

/// A rotation (columns) and a translation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// Columns: where x, y and z go.
    pub m: [[f32; 3]; 3],
    pub t: [f32; 3],
}

pub const IDENTITY: Affine = Affine {
    m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    t: [0.0; 3],
};

impl Default for Affine {
    fn default() -> Self {
        IDENTITY
    }
}

impl Affine {
    pub fn rotate(&self, v: [f32; 3]) -> [f32; 3] {
        let m = &self.m;
        std::array::from_fn(|k| m[0][k] * v[0] + m[1][k] * v[1] + m[2][k] * v[2])
    }
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let r = self.rotate(v);
        std::array::from_fn(|k| r[k] + self.t[k])
    }
    /// `self ∘ other`: other first.
    pub fn then(&self, other: &Affine) -> Affine {
        Affine {
            m: [
                self.rotate(other.m[0]),
                self.rotate(other.m[1]),
                self.rotate(other.m[2]),
            ],
            t: self.apply(other.t),
        }
    }
    pub fn translation(t: [f32; 3]) -> Affine {
        Affine { t, ..IDENTITY }
    }
    pub fn euler(r: [f32; 3]) -> Affine {
        Affine {
            m: euler(r),
            t: [0.0; 3],
        }
    }
}

/// The rotation Rx·Ry·Rz of an XYZ Euler in degrees (three.js's 'XYZ'), as columns.
pub fn euler(r: [f32; 3]) -> [[f32; 3]; 3] {
    let [a, b, c] = r.map(f32::to_radians);
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = b.sin_cos();
    let (sc, cc) = c.sin_cos();
    // three's makeRotationFromEuler, 'XYZ', row-major te → columns.
    let rows = [
        [cb * cc, -cb * sc, sb],
        [ca * sc + sa * sb * cc, ca * cc - sa * sb * sc, -sa * cb],
        [sa * sc - ca * sb * cc, sa * cc + ca * sb * sc, ca * cb],
    ];
    std::array::from_fn(|col| std::array::from_fn(|row| rows[row][col]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rig.mjs `rotate`: z first, then y, then x.
    fn rig_rotate(v: [f32; 3], r: [f32; 3]) -> [f32; 3] {
        let [a, b, c] = r.map(f32::to_radians);
        let [mut x, mut y, mut z] = v;
        (x, y) = (x * c.cos() - y * c.sin(), x * c.sin() + y * c.cos());
        (x, z) = (x * b.cos() + z * b.sin(), -x * b.sin() + z * b.cos());
        (y, z) = (y * a.cos() - z * a.sin(), y * a.sin() + z * a.cos());
        [x, y, z]
    }

    #[test]
    fn euler_matches_the_rigs_rotation() {
        for r in [
            [30.0, 0.0, 0.0],
            [0.0, -50.0, 0.0],
            [10.0, 20.0, 70.0],
            [-90.0, 45.0, 5.0],
        ] {
            for v in [[1.0, 0.0, 0.0], [0.3, -0.7, 0.2]] {
                let a = Affine::euler(r).rotate(v);
                let b = rig_rotate(v, r);
                for k in 0..3 {
                    assert!((a[k] - b[k]).abs() < 1e-5, "{r:?} {v:?}: {a:?} {b:?}");
                }
            }
        }
    }
}
