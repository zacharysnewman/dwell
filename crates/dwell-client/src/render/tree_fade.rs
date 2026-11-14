//! The far trees' hand-over in time, not distance (BUGS.md #154, owner 2026-11-13: "it shouldn't
//! ever be partial transitions over distance, but over time"). Each tree about the eye is in one
//! state, its real leaves or its impostor, switched by its crown's distance with hysteresis
//! (`impostors.json` `far.tiers` `inM`/`outM`, `Far::near_after`); a switch cross-fades over
//! `fadeS` seconds, then the tree is fully the one or the other, so a still eye sees no half-faded
//! tree at any distance.
//!
//! The GPU reads the states from one map: a square of [`window`] metres about the eye (face-local
//! x and z, one byte a metre, the near tree's share 0–255), each texel the share of the tree whose
//! crown covers it (the nearest foot within its bounding radius). The near leaves look their
//! fragment's texel up (`cutout_fs`, `tree_near_share`) and the impostor its tree's foot
//! (`imp_vs`), so a tree's leaves and its impostor fade as one and keep complementary pixels.
//! Leaves no listed tree owns (a player's) are the real ones within `inM` of the eye.

use std::collections::HashMap;

use dwell_world::FaceFrame;
use dwell_worldgen::impostor::{Config, Far};

/// The frame group's binding.
pub const BINDING: u32 = 21;
/// The farthest (m) a near tree's leaf lies from its crown's centre: the leaves are dropped
/// whole past `outM` + this (`card_dropped`), and the map reaches this far past `outM`.
pub const CROWN_M: f32 = 10.0;
/// The map recentres on the eye in steps of this many metres.
const SNAP_M: i32 = 8;
/// No tree owns the texel.
const NONE: u32 = u32::MAX;

/// The map's side (texels, one a metre): every tier's `outM` + [`CROWN_M`] each way from the eye,
/// with a snap step's slack each side, rounded up to a multiple of 4 (a word holds 4 texels).
pub fn window() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| window_for(Config::get()))
}

fn window_for(config: &Config) -> u32 {
    let out = config
        .far
        .iter()
        .filter(|(_, f)| f.impostor)
        .map(|(_, f)| f.out_m)
        .fold(0.0f32, f32::max);
    let side = (2.0 * (out + CROWN_M)).ceil() as u32 + 2 * SNAP_M as u32;
    side.div_ceil(4) * 4
}

/// How far (m) a near tree's leaf can be from the eye: past it every leaf is the impostors'.
pub fn leaves_reach(far: &Far) -> f32 {
    far.out_m + CROWN_M
}

/// One tree's state: near (its real leaves) or its impostor, switched at `since` (s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub near: bool,
    pub since: f64,
}

impl State {
    /// A tree first seen: settled (no fade) in the state its distance gives.
    pub fn settled(near: bool) -> Self {
        Self {
            near,
            since: f64::NEG_INFINITY,
        }
    }

    /// The near tree's share at `now` (0–1): rising from 0 to 1 over `fade_s` after a switch to
    /// near, falling after a switch to the impostor, and 0 or 1 once the fade is done.
    pub fn near_share(&self, now: f64, fade_s: f32) -> f32 {
        let t = if fade_s <= 0.0 {
            1.0
        } else {
            ((now - self.since) / f64::from(fade_s)).clamp(0.0, 1.0) as f32
        };
        if self.near { t } else { 1.0 - t }
    }

    /// Switches to `near` at `now`; a switch back mid-fade carries on from the share it has.
    pub fn switch(&mut self, near: bool, now: f64, fade_s: f32) {
        if near == self.near {
            return;
        }
        let share = self.near_share(now, fade_s);
        let t = if near { share } else { 1.0 - share };
        self.near = near;
        self.since = now - f64::from(t * fade_s);
    }
}

/// A tree the map places: its key (face-local foot, rows), its foot (face-local m), its crown's
/// centre height over the foot and its bounding radius (m), and whether its impostor is baked (an
/// unbaked impostor draws nothing, so its tree stays the real one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tree {
    pub key: [i32; 3],
    pub foot: [f32; 3],
    pub centre_y: f32,
    pub radius: f32,
    pub baked: bool,
}

/// The states of the eye's face's trees within the map, the map's ownership and its texels.
pub struct TreeFades {
    n: u32,
    face: Option<FaceFrame>,
    /// The map's corner (face-local x and z, m).
    pub origin: [i32; 2],
    trees: Vec<Tree>,
    states: HashMap<[i32; 3], State>,
    owner: Vec<u32>,
    /// One byte a texel, row z then x: the near tree's share.
    pub texels: Vec<u8>,
    rebuild: bool,
}

impl Default for TreeFades {
    fn default() -> Self {
        Self::new(window())
    }
}

impl TreeFades {
    pub fn new(n: u32) -> Self {
        let cells = (n * n) as usize;
        Self {
            n,
            face: None,
            origin: [i32::MIN, i32::MIN],
            trees: Vec::new(),
            states: HashMap::new(),
            owner: vec![NONE; cells],
            texels: vec![0; cells],
            rebuild: true,
        }
    }

    /// The trees changed (a tile came or went, a layer was baked): the next update re-reads them.
    pub fn touch(&mut self) {
        self.rebuild = true;
    }

    /// A tree's state, by key.
    pub fn state(&self, key: [i32; 3]) -> Option<State> {
        self.states.get(&key).copied()
    }

    /// The near tree's share at face-local `x`, `z` from the map (0 outside it).
    pub fn share_at(&self, x: f64, z: f64) -> f32 {
        let i = (x.floor() as i64) - i64::from(self.origin[0]);
        let k = (z.floor() as i64) - i64::from(self.origin[1]);
        let n = i64::from(self.n);
        if !(0..n).contains(&i) || !(0..n).contains(&k) {
            return 0.0;
        }
        f32::from(self.texels[(k * n + i) as usize]) / 255.0
    }

    /// The frame uniform's `far_trees` (shaders.rs `tree_near_share`): the eye's face-local x and
    /// z from the map's corner, the leaves' reach, 1 on.
    pub fn uniform(&self, far: &Far, face: FaceFrame, eye: [f64; 3]) -> [f32; 4] {
        let e = face.point(eye);
        [
            (e[0] - f64::from(self.origin[0])) as f32,
            (e[2] - f64::from(self.origin[1])) as f32,
            leaves_reach(far),
            1.0,
        ]
    }

    /// This frame's states and map at `now` (s) for the eye at `eye` (world) on `face`; `trees`
    /// (face-local) read only when [`TreeFades::touch`]ed or the map moved. Returns whether the
    /// texels changed (the GPU's copy needs writing).
    pub fn update<'a>(
        &mut self,
        now: f64,
        face: FaceFrame,
        eye: [f64; 3],
        far: &Far,
        trees: impl FnOnce() -> Box<dyn Iterator<Item = Tree> + 'a>,
    ) -> bool {
        let e = face.point(eye);
        let half = f64::from(self.n / 2);
        let snap = |c: f64| ((c - half) / f64::from(SNAP_M)).floor() as i32 * SNAP_M;
        let origin = [snap(e[0]), snap(e[2])];
        if origin != self.origin || self.face != Some(face) || self.rebuild {
            if self.face != Some(face) {
                self.states.clear();
            }
            self.origin = origin;
            self.face = Some(face);
            self.rebuild = false;
            let (o, n) = (origin, self.n as i32);
            self.trees = trees()
                .filter(|t| {
                    let (x, z) = (
                        t.foot[0].floor() as i32 - o[0],
                        t.foot[2].floor() as i32 - o[1],
                    );
                    (0..n).contains(&x) && (0..n).contains(&z)
                })
                .collect();
            self.raster();
            let keep: std::collections::HashSet<[i32; 3]> =
                self.trees.iter().map(|t| t.key).collect();
            self.states.retain(|k, _| keep.contains(k));
        }
        // Each tree's state: first seen, settled where its distance puts it; then switched with
        // hysteresis, a timed fade each switch.
        let mut shares = Vec::with_capacity(self.trees.len());
        for t in &self.trees {
            let c = [t.foot[0], t.foot[1] + t.centre_y, t.foot[2]];
            let d = ((f64::from(c[0]) - e[0]).powi(2)
                + (f64::from(c[1]) - e[1]).powi(2)
                + (f64::from(c[2]) - e[2]).powi(2))
            .sqrt() as f32;
            let s = self
                .states
                .entry(t.key)
                .or_insert_with(|| State::settled(!t.baked || d <= (far.in_m + far.out_m) * 0.5));
            let want = !t.baked || far.near_after(s.near, d);
            s.switch(want, now, far.fade_s);
            shares.push((s.near_share(now, far.fade_s) * 255.0).round() as u8);
        }
        let n = self.n as usize;
        let mut changed = false;
        for k in 0..n {
            for i in 0..n {
                let at = k * n + i;
                let v = match self.owner[at] {
                    NONE => {
                        let dx = f64::from(self.origin[0]) + i as f64 + 0.5 - e[0];
                        let dz = f64::from(self.origin[1]) + k as f64 + 0.5 - e[2];
                        if (dx * dx + dz * dz).sqrt() < f64::from(far.in_m) {
                            255
                        } else {
                            0
                        }
                    }
                    o => shares[o as usize],
                };
                if self.texels[at] != v {
                    self.texels[at] = v;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Each texel's tree: the nearest foot whose bounding radius covers the texel's centre.
    fn raster(&mut self) {
        let n = self.n as i32;
        self.owner.fill(NONE);
        let mut best = vec![f32::MAX; self.owner.len()];
        for (ti, t) in self.trees.iter().enumerate() {
            let (fx, fz) = (
                t.foot[0] - self.origin[0] as f32,
                t.foot[2] - self.origin[1] as f32,
            );
            let r = t.radius.max(1.0);
            let lo = |c: f32| ((c - r).floor() as i32).clamp(0, n - 1);
            let hi = |c: f32| ((c + r).ceil() as i32).clamp(0, n - 1);
            for k in lo(fz)..=hi(fz) {
                for i in lo(fx)..=hi(fx) {
                    let (dx, dz) = (i as f32 + 0.5 - fx, k as f32 + 0.5 - fz);
                    let d2 = dx * dx + dz * dz;
                    let at = (k * n + i) as usize;
                    if d2 <= r * r && d2 < best[at] {
                        best[at] = d2;
                        self.owner[at] = ti as u32;
                    }
                }
            }
        }
    }
}

pub fn layout_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: BINDING,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The map on the GPU: [`window`]² bytes as words.
pub struct TreeFadeGpu {
    pub buffer: wgpu::Buffer,
}

impl TreeFadeGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tree fades"),
                size: Self::bytes(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    pub fn group_entry(&self) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding: BINDING,
            resource: self.buffer.as_entire_binding(),
        }
    }

    /// Writes the whole map (`texels` [`window`]² bytes).
    pub fn write(&self, queue: &wgpu::Queue, texels: &[u8]) {
        if texels.len() as u64 == Self::bytes() {
            queue.write_buffer(&self.buffer, 0, texels);
        }
    }

    /// GPU bytes held.
    pub fn bytes() -> u64 {
        u64::from(window()).pow(2)
    }
}

/// The shader's half (joined to the scene shaders): `tree_near_at`, a map texel's near share, and
/// `tree_near_share`, a leaf fragment's.
pub fn wgsl() -> String {
    format!(
        "@group(0) @binding({BINDING}) var<storage, read> tree_fades: array<u32>;
const TREE_FADE_N: i32 = {n};

// The near tree's share (0–1) at `p` m (face-local x, z) from the map's corner
// (render/tree_fade.rs); 0 outside it.
fn tree_near_at(p: vec2f) -> f32 {{
    let t = vec2i(floor(p));
    if (t.x < 0 || t.y < 0 || t.x >= TREE_FADE_N || t.y >= TREE_FADE_N) {{
        return 0.0;
    }}
    let i = u32(t.y * TREE_FADE_N + t.x);
    return f32((tree_fades[i / 4u] >> ((i % 4u) * 8u)) & 0xffu) / 255.0;
}}

// The near trees' share of a leaf fragment `rel` from the eye (`TreeFades`): its tree's state, 1
// when no impostors draw. A leaf on the other face (no impostors there) is the real tree's.
fn tree_near_share(rel: vec3f) -> f32 {{
    if (frame.far_trees.w < 0.5) {{
        return 1.0;
    }}
    let face = frame.eye.y;
    if (face_at(frame.eye.x + rel.y) != face) {{
        return 1.0;
    }}
    return tree_near_at(frame.far_trees.xy + face_vector(face, rel).xz);
}}
",
        n = window(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn far() -> Far {
        Config::get().far("on").expect("the on tier")
    }

    /// A row of trees along +x from the eye's column, 2 m apart.
    fn row(ground: f32) -> Vec<Tree> {
        (0..60)
            .map(|k| {
                let x = 4.0 + 2.0 * k as f32;
                Tree {
                    key: [x as i32, 0, 0],
                    foot: [x + 0.5, ground, 0.5],
                    centre_y: 6.0,
                    radius: 1.2,
                    baked: true,
                }
            })
            .collect()
    }

    fn eye_at(face: FaceFrame, local: [f64; 3]) -> [f64; 3] {
        face.point(local)
    }

    fn step(f: &mut TreeFades, now: f64, face: FaceFrame, eye: [f64; 3], trees: &[Tree]) -> bool {
        let far = far();
        let list = trees.to_vec();
        f.update(now, face, eye, &far, move || Box::new(list.into_iter()))
    }

    dwell_world::both_faces! {
        /// BUGS.md #154: a still eye at any distance sees every tree fully near or fully its
        /// impostor once the fade time is past, the old fade band included (48–64 m, where both
        /// drew half each before).
        fn a_still_eye_sees_no_half_faded_tree_at_any_distance(face: FaceFrame) {
            let trees = row(10.0);
            let far = far();
            for eye_x in [0.0, 6.0, 13.0, 21.0] {
                let mut f = TreeFades::new(window());
                let eye = eye_at(face, [eye_x, 16.0, 0.5]);
                for k in 0..30 {
                    step(&mut f, f64::from(k) * 0.05, face, eye, &trees);
                }
                let mut near = 0;
                for t in &trees {
                    let s = f.share_at(f64::from(t.foot[0]), f64::from(t.foot[2]));
                    assert!(s == 0.0 || s == 1.0, "a tree at {:?} drawn at {s}", t.foot);
                    let d = ((f64::from(t.foot[0]) - eye_x).powi(2)
                        + (f64::from(t.foot[1] + t.centre_y) - 16.0).powi(2))
                    .sqrt() as f32;
                    if d < far.in_m {
                        assert_eq!(s, 1.0, "a tree {d} m off is the real one");
                    }
                    if d > far.out_m {
                        assert_eq!(s, 0.0, "a tree {d} m off is its impostor");
                    }
                    near += usize::from(s == 1.0);
                }
                assert!(near > 0 && near < trees.len(), "the row spans the switch");
            }
        }
    }

    dwell_world::both_faces! {
        /// Crossing the switch distance starts one fade, done in `fadeS`; then nothing moves.
        fn crossing_the_switch_fades_once_over_time(face: FaceFrame) {
            let far = far();
            let tree = Tree {
                key: [0, 0, 0],
                foot: [0.5, 10.0, 0.5],
                centre_y: 5.0,
                radius: 2.0,
                baked: true,
            };
            let trees = [tree];
            let mut f = TreeFades::new(window());
            let at = |d: f64| eye_at(face, [0.5 - d, 15.0, 0.5]);
            step(&mut f, 0.0, face, at(f64::from(far.in_m) - 5.0), &trees);
            assert_eq!(f.share_at(0.5, 0.5), 1.0, "near: the real tree");
            // Back past outM: one switch, fading over fadeS, the share falling with time alone.
            let mut now = 1.0;
            let mut last = 1.0;
            let out = at(f64::from(far.out_m) + 1.0);
            step(&mut f, now, face, out, &trees);
            let since = f.state([0, 0, 0]).unwrap().since;
            assert_eq!(since, now, "switched once, now");
            while now < 1.0 + f64::from(far.fade_s) {
                now += 0.02;
                step(&mut f, now, face, out, &trees);
                let s = f.share_at(0.5, 0.5);
                assert!(s <= last, "fading out: {s} after {last}");
                last = s;
            }
            assert_eq!(f.share_at(0.5, 0.5), 0.0, "the impostor alone after fadeS");
            assert_eq!(f.state([0, 0, 0]).unwrap().since, since, "no second switch");
        }
    }

    dwell_world::both_faces! {
        /// Standing at the boundary (between inM and outM) and wobbling there switches nothing.
        fn the_gap_between_in_and_out_keeps_a_tree_from_flickering(face: FaceFrame) {
            let far = far();
            let tree = Tree {
                key: [0, 0, 0],
                foot: [0.5, 10.0, 0.5],
                centre_y: 5.0,
                radius: 2.0,
                baked: true,
            };
            let trees = [tree];
            let mut f = TreeFades::new(window());
            let at = |d: f64| eye_at(face, [0.5 - d, 15.0, 0.5]);
            step(&mut f, 0.0, face, at(f64::from(far.out_m) + 0.5), &trees);
            step(&mut f, 0.01, face, at(f64::from(far.in_m) - 0.5), &trees);
            let since = f.state([0, 0, 0]).unwrap().since;
            let gap = f64::from(far.out_m - far.in_m);
            for k in 0..200 {
                let wobble = f64::from(far.in_m) + 0.2 + (gap - 0.4) * ((k % 7) as f64 / 6.0);
                step(&mut f, 0.02 + f64::from(k) * 0.016, face, at(wobble), &trees);
            }
            let s = f.state([0, 0, 0]).unwrap();
            assert!(s.near && s.since == since, "no switch inside the gap: {s:?}");
            assert_eq!(f.share_at(0.5, 0.5), 1.0);
        }
    }

    dwell_world::both_faces! {
        /// A tree's leaves away from its foot read its own state (its crown's texels), and an
        /// unbaked impostor keeps its tree the real one.
        fn a_crown_reads_its_trees_state_and_an_unbaked_tree_stays_real(face: FaceFrame) {
            let far = far();
            let d = f64::from(far.out_m) + 4.0;
            let trees = [
                Tree { key: [0, 0, 0], foot: [0.5, 10.0, 0.5], centre_y: 4.0, radius: 3.0, baked: true },
                Tree { key: [0, 0, 8], foot: [0.5, 10.0, 8.5], centre_y: 4.0, radius: 3.0, baked: false },
            ];
            let mut f = TreeFades::new(window());
            step(&mut f, 0.0, face, eye_at(face, [0.5 - d, 14.0, 4.5]), &trees);
            assert_eq!(f.share_at(-1.5, 1.5), 0.0, "the far tree's crown edge: its impostor");
            assert_eq!(f.share_at(1.5, 8.0), 1.0, "the unbaked one's: the real leaves");
        }
    }

    #[test]
    fn a_reversal_mid_fade_carries_on_from_its_share() {
        // face-a-only: time only.
        let mut s = State::settled(true);
        s.switch(false, 10.0, 0.4);
        let mid = s.near_share(10.1, 0.4);
        assert!((mid - 0.75).abs() < 1e-4, "{mid}");
        s.switch(true, 10.1, 0.4);
        assert!((s.near_share(10.1, 0.4) - mid).abs() < 1e-4, "no jump");
        assert_eq!(s.near_share(10.3, 0.4), 1.0);
    }

    #[test]
    fn the_window_covers_the_leaves_reach_about_the_eye() {
        // face-a-only: sizes only.
        let far = far();
        let n = window();
        assert_eq!(n % 4, 0);
        assert!(n as f32 / 2.0 - SNAP_M as f32 >= leaves_reach(&far), "{n}");
        assert!(wgsl().contains(&format!("TREE_FADE_N: i32 = {n};")));
    }
}
