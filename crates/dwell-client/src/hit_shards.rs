//! A hit's flying pieces (docs/BLOCK_DAMAGE.md §6.4, §10; WORK_QUEUE item 18): `chips.shards` real
//! 3D pieces a hit flings out of the struck face (stone's elongated shards, soil's clods, the axe's
//! wood chips), each its `chips.piece` item's look scaled so that together they are the hit's bite
//! (the bite rule: 0.5 × the block's volume ÷ its hits to break), tumbling under gravity and gone
//! after [`LIFE_S`]. Draw-only, through the pieces' renderer (no physics, no item): the one
//! picked-up shard a hit drops is the server's loose item. None on a CPU adapter; half on a phone.
//!
//! Face-local: they fall toward the face's ground (`FaceFrame`).

use dwell_protocol::chopping::{BLOCK_DAMAGE, Chips, WOODS};
use dwell_world::FaceFrame;

/// The fragments' ids in the pieces' renderer: under the loose items' (0xC000_0000), over bodies'.
pub const ID_BASE: u32 = 0xA000_0000;
/// The most fragments in flight (a ring of ids).
pub const MAX: u32 = 48;
/// How long a fragment flies (s).
pub const LIFE_S: f64 = 1.0;
/// Gravity on a fragment (m/s²), toward the face's ground.
const GRAVITY: f64 = 9.0;

/// One fragment in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fragment {
    pub id: u32,
    pub at: [f64; 3],
    /// World m/s.
    pub vel: [f64; 3],
    /// Its tumble axis (unit, world) and rate (rad/s).
    pub axis: [f64; 3],
    pub spin: f64,
    pub born_s: f64,
}

#[derive(Clone, Debug, Default)]
pub struct HitShards {
    pub list: Vec<Fragment>,
    next: u32,
    /// A CPU adapter: none.
    pub cpu: bool,
    /// A phone: half.
    pub phone: bool,
    /// Graphics → Hit shards: the share of them flung (crate::effect_share).
    pub share: crate::effect_share::Share,
}

fn hash(seed: u32, k: u32) -> f64 {
    let mut h = seed ^ k.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h) / f64::from(u32::MAX)
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-9 { v } else { v.map(|c| c / l) }
}

impl HitShards {
    /// How many fragments a hit on `block` (by name) flings: its data's, none on a CPU adapter,
    /// half (at least one) on a phone, then the Hit shards setting's share of that.
    pub fn count_of(&self, block: &str) -> usize {
        let n = chips_of(block).map_or(0, |c| c.shards as usize);
        let n = if self.cpu || n == 0 {
            0
        } else if self.phone {
            n.div_ceil(2)
        } else {
            n
        };
        self.share.of(n)
    }

    /// A hit's fragments out of the face with outward `normal` (world) at `point` (world): the
    /// ids that are new (their meshes to set) and those reused from the ring (already meshed).
    pub fn burst(
        &mut self,
        block: &str,
        normal: [f64; 3],
        point: [f64; 3],
        now_s: f64,
    ) -> Vec<u32> {
        let n = self.count_of(block);
        let f = FaceFrame::at_height(point[1]);
        let nl = f.vector(normal);
        let speed = chips_of(block).map_or(2.5, |c| c.speed);
        let seed = (point[0] * 977.0) as i32 as u32
            ^ ((point[1] * 613.0) as i32 as u32).wrapping_mul(0x85eb_ca6b)
            ^ ((point[2] * 331.0) as i32 as u32).wrapping_mul(0xc2b2_ae35)
            ^ (now_s * 1000.0) as u32;
        let mut ids = Vec::new();
        for k in 0..n as u32 {
            let r: [f64; 3] = std::array::from_fn(|j| hash(seed, k * 3 + j as u32) * 2.0 - 1.0);
            // Out of the face, scattered, a little up (face-local), then into the world.
            let d = norm(std::array::from_fn(|j| {
                nl[j] + 0.7 * r[j] + if j == 1 { 0.5 } else { 0.0 }
            }));
            let s = speed * (0.6 + 0.6 * hash(seed, 50 + k));
            let a: [f64; 3] = std::array::from_fn(|j| hash(seed, 80 + k * 3 + j as u32) - 0.5);
            let id = ID_BASE | (self.next % MAX);
            self.next = self.next.wrapping_add(1);
            self.list.retain(|x| x.id != id);
            self.list.push(Fragment {
                id,
                at: point,
                vel: f.vector(d.map(|c| c * s)),
                axis: norm(a),
                spin: 8.0 + 10.0 * hash(seed, 120 + k),
                born_s: now_s,
            });
            ids.push(id);
        }
        ids
    }

    /// Each fragment's pose now (id, world point, rotation quaternion x, y, z, w), and the ids
    /// whose time is up (removed from the list).
    #[allow(clippy::type_complexity)]
    pub fn step(&mut self, now_s: f64) -> (Vec<(u32, [f64; 3], [f32; 4])>, Vec<u32>) {
        let mut gone = Vec::new();
        self.list.retain(|x| {
            let keep = now_s - x.born_s < LIFE_S;
            if !keep {
                gone.push(x.id);
            }
            keep
        });
        let poses = self
            .list
            .iter()
            .map(|x| {
                let t = (now_s - x.born_s).max(0.0);
                let f = FaceFrame::at_height(x.at[1]);
                let g = f.vector([0.0, -0.5 * GRAVITY * t * t, 0.0]);
                let p: [f64; 3] = std::array::from_fn(|k| x.at[k] + x.vel[k] * t + g[k]);
                let half = 0.5 * x.spin * t;
                let (s, c) = half.sin_cos();
                (
                    x.id,
                    p,
                    [
                        (x.axis[0] * s) as f32,
                        (x.axis[1] * s) as f32,
                        (x.axis[2] * s) as f32,
                        c as f32,
                    ],
                )
            })
            .collect();
        (poses, gone)
    }
}

/// The chips data of a block hit or a wood chopped (its `damage.chips`, or the wood's `chips`).
pub fn chips_of(block: &str) -> Option<Chips> {
    BLOCK_DAMAGE
        .iter()
        .find(|d| d.block == block)
        .and_then(|d| d.chips)
        .or_else(|| {
            WOODS
                .iter()
                .find(|w| w.block == block || w.fell_wedge == Some(block))
                .map(|w| w.chips)
        })
}

/// A closed mesh's volume (m³; the divergence theorem over its opaque triangles).
pub fn mesh_volume(meshes: &[dwell_mesh::ChunkMeshes]) -> f64 {
    let mut v = 0.0;
    for m in meshes {
        let p = |i: u32| {
            let i = i as usize * 3;
            let q = &m.opaque.positions;
            [f64::from(q[i]), f64::from(q[i + 1]), f64::from(q[i + 2])]
        };
        for t in m.opaque.indices.chunks_exact(3) {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            v += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
    }
    v.abs()
}

/// The linear scale on `block`'s piece look that makes its data's `shards` pieces total
/// `bite_m3` (the bite rule; BLOCK_DAMAGE.md §10): ∛(bite ÷ (count × the look's volume)).
pub fn piece_scale(block: &str, bite_m3: f64, unit_m3: f64) -> f64 {
    let n = chips_of(block).map_or(0, |c| c.shards);
    if n == 0 || unit_m3 <= 0.0 {
        return 0.0;
    }
    (bite_m3 / (f64::from(n) * unit_m3)).cbrt()
}

/// A hit's pieces' meshes: `block`'s piece item's look scaled so its data's `shards` of them total
/// `bite_m3`, face-mirrored as the loose item is; none for a block without pieces.
pub fn fragment_meshes(
    block: &str,
    bite_m3: f64,
    face: FaceFrame,
) -> Option<(Vec<dwell_mesh::ChunkMeshes>, f64)> {
    let piece = chips_of(block)?.piece;
    let (mut meshes, size) = crate::loose_items::item_meshes(piece, face)?;
    let k = piece_scale(block, bite_m3, mesh_volume(&meshes));
    for m in &mut meshes {
        for p in m.opaque.positions.iter_mut() {
            *p *= k as f32;
        }
    }
    Some((meshes, size * k))
}

#[cfg(test)]
mod tests {
    use super::*;

    dwell_world::both_faces!(
        /// A pickaxe hit on stone flings its data's 3D fragments out of the struck face, which
        /// tumble, fall toward the face's ground and are gone after a second (owner: "elongated
        /// low poly shards"; WORK_QUEUE item 18).
        fn a_stone_hit_flings_tumbling_fragments_that_fall_and_go(f: FaceFrame) {
            let mut h = HitShards::default();
            let n = h.count_of("dwell:stone");
            assert!((3..=5).contains(&n), "3–5 a hit: {n}");
            let point = f.point([0.5, 10.0, 0.5]);
            let up = f.vector([0.0, 1.0, 0.0]);
            let ids = h.burst("dwell:stone", up, point, 0.0);
            assert_eq!(ids.len(), n);
            let (p0, gone) = h.step(0.0);
            assert!(gone.is_empty() && p0.len() == n);
            let (p1, _) = h.step(0.6);
            let along = |p: [f64; 3]| (0..3).map(|k| (p[k] - point[k]) * up[k]).sum::<f64>();
            let mut turned = 0;
            for (a, b) in p0.iter().zip(&p1) {
                // Out of the face, then falling back toward the ground by 0.6 s.
                assert!(b.1 != a.1, "it flies");
                if (b.2[3] - a.2[3]).abs() > 1e-3 {
                    turned += 1;
                }
            }
            assert_eq!(turned, n, "every fragment tumbles");
            let (p2, _) = h.step(0.95);
            for (b, c) in p1.iter().zip(&p2) {
                assert!(along(c.1) < along(b.1) + 0.5, "gravity bends it down");
            }
            assert!(
                p2.iter().map(|p| along(p.1)).sum::<f64>()
                    < p1.iter().map(|p| along(p.1)).sum::<f64>(),
                "falling by then"
            );
            let (p3, gone) = h.step(1.01);
            assert!(p3.is_empty() && gone.len() == n, "gone after a second");
        }
    );

    /// The bite rule (owner, 2026-11-12): a hit's pieces together are its bite, 0.5 × the block's
    /// volume ÷ its hits to break, for stone's shards, soil's clods and the axe's wood chips; the
    /// metal pickaxe's 3 hits' pieces are 5/3 of the stone one's 5 hits'.
    #[test]
    fn a_hits_pieces_total_its_bite() {
        // face-a-only: volumes, the same mirrored (a mirror keeps a volume).
        use dwell_protocol::damage::bite_m3_of;
        let total = |block: &str, bite: f64| {
            let n = f64::from(chips_of(block).unwrap().shards);
            let (m, _) = fragment_meshes(block, bite, FaceFrame::A).expect("pieces");
            n * mesh_volume(&m)
        };
        for (block, tool) in [
            ("dwell:stone", "dwell:stone_pickaxe"),
            ("dwell:stone", "dwell:metal_pickaxe"),
            ("dwell:dirt", "dwell:wooden_shovel"),
            ("dwell:grass", "dwell:metal_shovel"),
        ] {
            let bite = bite_m3_of(block, Some(tool)).unwrap();
            let got = total(block, bite);
            assert!(
                (got - bite).abs() < 1e-3 * bite,
                "{block} {tool}: {got} vs {bite}"
            );
        }
        let log = dwell_protocol::chopping::wood("dwell:log").unwrap();
        let axe = dwell_protocol::chopping::tool("dwell:stone_axe").unwrap();
        let bite = dwell_protocol::chopping::chop_bite_m3(axe, log);
        assert!(
            (total("dwell:log", bite) - bite).abs() < 1e-3 * bite,
            "wood chips"
        );
        // Bigger, not more: the same count, each ∛(5/3) the size.
        let s = |t| bite_m3_of("dwell:stone", Some(t)).unwrap();
        let (a, b) = (
            fragment_meshes("dwell:stone", s("dwell:metal_pickaxe"), FaceFrame::A)
                .unwrap()
                .1,
            fragment_meshes("dwell:stone", s("dwell:stone_pickaxe"), FaceFrame::A)
                .unwrap()
                .1,
        );
        assert!((a / b - (5.0f64 / 3.0).cbrt()).abs() < 1e-6, "{a} {b}");
    }

    #[test]
    fn none_on_a_cpu_adapter_half_on_a_phone_none_without_data() {
        // face-a-only: counts, before anything face-dependent.
        let n = HitShards::default().count_of("dwell:stone");
        let cpu = HitShards {
            cpu: true,
            ..HitShards::default()
        };
        let phone = HitShards {
            phone: true,
            ..HitShards::default()
        };
        assert_eq!(cpu.count_of("dwell:stone"), 0);
        assert_eq!(phone.count_of("dwell:stone"), n.div_ceil(2));
        assert_eq!(HitShards::default().count_of("dwell:sky"), 0);
    }

    #[test]
    fn the_ring_reuses_ids() {
        // face-a-only: ids only.
        let mut h = HitShards::default();
        for i in 0..40 {
            h.burst(
                "dwell:stone",
                [0.0, 1.0, 0.0],
                [0.5, 10.0, 0.5],
                f64::from(i) * 0.01,
            );
        }
        assert!(h.list.len() <= MAX as usize);
        assert!(
            h.list
                .iter()
                .all(|x| x.id & ID_BASE == ID_BASE && x.id & !ID_BASE < MAX)
        );
    }
}
