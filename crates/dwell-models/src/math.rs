//! The little math skinning needs: column-major 4 × 4 matrices and unit quaternions (x, y, z, w),
//! as glTF stores them.

pub type Mat4 = [[f32; 4]; 4];
pub type Quat = [f32; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a × b` (apply `b` first).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [[0.0; 4]; 4];
    for (c, col) in o.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    o
}

/// A point through `m`.
pub fn point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

/// The matrix of translation `t`, rotation `q` and scale `s` (applied scale, rotate, translate).
pub fn trs(t: [f32; 3], q: Quat, s: [f32; 3]) -> Mat4 {
    let [x, y, z, w] = q;
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    [
        [r[0][0] * s[0], r[0][1] * s[0], r[0][2] * s[0], 0.0],
        [r[1][0] * s[1], r[1][1] * s[1], r[1][2] * s[1], 0.0],
        [r[2][0] * s[2], r[2][1] * s[2], r[2][2] * s[2], 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

/// The normalised blend of two rotations along the shorter arc (nlerp: close to slerp for the small
/// steps between keys and between clips' poses, and cheaper).
pub fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let d: f32 = (0..4).map(|k| a[k] * b[k]).sum();
    let s = if d < 0.0 { -1.0 } else { 1.0 };
    let q: Quat = std::array::from_fn(|k| a[k] * (1.0 - t) + b[k] * s * t);
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if n < 1e-8 { a } else { q.map(|v| v / n) }
}

/// The rotation by `deg` degrees about the unit `axis`.
pub fn axis_angle(axis: [f32; 3], deg: f32) -> Quat {
    let (s, c) = (deg.to_radians() / 2.0).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}
