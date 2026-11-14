//! The client's side of inventories and loose items (docs/adr/0066-inventory-and-drops.md): the
//! inventory the server last sent, which palette slot a hotbar slot places, and the loose items
//! drawn as small spinning copies of their block (an item with a shaped `look` as that look, a
//! stick as a rod and a rock as a lump; any other item as a small block in its colour) through the pieces' renderer.

use dwell_blocks::{StateId, find_block};
use dwell_mesh::ChunkMeshes;
use dwell_protocol::inventory::{self as inv, Inventory};
use dwell_protocol::{ItemDropNet, ItemDrops};
use dwell_world::FaceFrame;
use dwell_world::hash::HashMap;

/// Loose items' ids in the pieces' renderer: above any body's.
pub const ID_BASE: u32 = 0xC000_0000;
/// A block item's size as a loose item (m across).
const BLOCK_SIZE_M: f64 = 0.25;
/// Turns a second, and the bob's height (m) and period (s).
const SPIN_PER_S: f64 = 0.5;
const BOB_M: f64 = 0.06;
const BOB_S: f64 = 2.0;
/// The block a non-block item's look is drawn with, tinted its colour.
const STAND_IN: &str = "dwell:stone";

/// The palette slot (index) that places what inventory slot `slot` holds: the palette item of the
/// same name (`dwell:stone`); none for an empty slot or an item that places nothing.
pub fn palette_slot_for(
    inventory: &Inventory,
    slot: usize,
    palette_items: &[&'static str],
) -> Option<usize> {
    let s = inventory.slots.get(slot)?.as_ref()?;
    palette_items.iter().position(|&n| n == s.item)
}

/// The block state an item is drawn with, and the colour it is tinted (non-block items).
fn look_of(item: &str) -> Option<(StateId, Option<[f32; 3]>)> {
    if let Some(b) = find_block(item) {
        let m = (b.first_state..b.first_state + b.state_count)
            .find(|&m| dwell_world::edit::placeable(m))
            .unwrap_or(b.first_state);
        return Some((m, None));
    }
    let def = inv::item(item)?;
    let stand_in = find_block(STAND_IN)?.first_state;
    Some((stand_in, Some(def.color.map(|c| f32::from(c) / 255.0))))
}

/// The meshes of a loose item: its block's one-cell meshes scaled to its size about their centre,
/// the centre at half its size above its point (face-local; mirrored on face B).
pub fn item_meshes(item: &str, face: FaceFrame) -> Option<(Vec<ChunkMeshes>, f64)> {
    // An item with a shaped look (a stick, a rock) is that look, at its own size: the same shape,
    // tile and tint as one lying on the ground (shared/inventory `look`).
    if let Some(look) = inv::item(item).and_then(|d| d.look) {
        let mut m = dwell_mesh::look_mesh(&look);
        if face.is_b() {
            mirror_y(&mut m);
        }
        let size = look.size.iter().copied().fold(0.0, f64::max);
        let meshes = vec![ChunkMeshes {
            opaque: m,
            ..ChunkMeshes::default()
        }];
        return Some((meshes, size));
    }
    let (m, tint) = look_of(item)?;
    let size = inv::item(item).map_or(BLOCK_SIZE_M, |d| d.size);
    let (mut meshes, _) = crate::pieces::piece_meshes_mirrored(&[([0, 0, 0], m)], face.is_b());
    // Its bounds, to scale about its centre.
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for ms in &meshes {
        for a in [&ms.opaque, &ms.cutout] {
            for p in a.positions.chunks_exact(3) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
    }
    if lo[0] > hi[0] {
        return None;
    }
    let centre: [f32; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let extent = (0..3)
        .map(|k| hi[k] - lo[k])
        .fold(0.0f32, f32::max)
        .max(1e-3);
    let scale = size as f32 / extent;
    let half_h = (hi[1] - lo[1]) * scale / 2.0;
    let up = face.sign_f32();
    for ms in &mut meshes {
        for a in [&mut ms.opaque, &mut ms.cutout] {
            for p in a.positions.chunks_exact_mut(3) {
                for k in 0..3 {
                    p[k] = (p[k] - centre[k]) * scale;
                }
                p[1] += up * half_h;
            }
            if let Some(c) = tint {
                for v in a.colors.chunks_exact_mut(3) {
                    v.copy_from_slice(&c);
                }
            }
        }
    }
    Some((meshes, size))
}

/// `m` mirrored through y = 0, face B's copy of a face-local item mesh (as the pieces' meshes are
/// mirrored): positions and normals flipped, each triangle's winding turned so it still faces out.
fn mirror_y(m: &mut dwell_mesh::MeshArrays) {
    for p in m.positions.chunks_exact_mut(3) {
        p[1] = -p[1];
    }
    for n in m.normals.chunks_exact_mut(3) {
        n[1] = -n[1];
    }
    for t in m.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
}

/// The inventory and loose items as the server last sent them.
#[derive(Default)]
pub struct LooseItems {
    /// The inventory and whether the world's mode uses items (adventure); none from a server that
    /// keeps no inventories.
    pub inventory: Option<(bool, Inventory)>,
    pub items: HashMap<u32, ItemDropNet>,
}

/// What an `ItemDrops` changed for the renderer: items to (re)mesh, ids to remove.
pub struct Changed {
    pub added: Vec<(u32, String, FaceFrame)>,
    pub removed: Vec<u32>,
}

impl LooseItems {
    pub fn uses_items(&self) -> bool {
        self.inventory.as_ref().is_some_and(|(u, _)| *u)
    }

    pub fn apply(&mut self, d: ItemDrops) -> Changed {
        let mut out = Changed {
            added: Vec::new(),
            removed: Vec::new(),
        };
        for id in d.gone {
            if self.items.remove(&id).is_some() {
                out.removed.push(id);
            }
        }
        for it in d.items {
            let face = FaceFrame::at_height(f64::from(it.position[1]));
            let fresh = self.items.get(&it.id).is_none_or(|o| {
                o.item != it.item || FaceFrame::at_height(f64::from(o.position[1])) != face
            });
            if fresh {
                out.added.push((it.id, it.item.clone(), face));
            }
            self.items.insert(it.id, it);
        }
        out
    }

    /// Each item's pose now: its point raised by the bob along its face's up, turning about up.
    pub fn poses(&self, now_ms: f64) -> Vec<(u32, [f64; 3], [f32; 4])> {
        let t = now_ms / 1000.0;
        self.items
            .values()
            .map(|it| {
                let p = it.position.map(f64::from);
                let f = FaceFrame::at_height(p[1]);
                let phase = f64::from(it.id % 97) * 0.37;
                let bob = BOB_M * (0.5 + 0.5 * ((t / BOB_S + phase) * std::f64::consts::TAU).sin());
                let angle = (t * SPIN_PER_S + phase) * std::f64::consts::TAU;
                let (s, c) = ((angle / 2.0).sin() as f32, (angle / 2.0).cos() as f32);
                (
                    ID_BASE | (it.id & !ID_BASE),
                    [p[0], p[1] + f.sign_f64() * bob, p[2]],
                    [0.0, s, 0.0, c],
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::inventory::Stack;

    #[test]
    fn a_hotbar_slot_places_the_palette_item_of_its_name() {
        // face-a-only: names, no geometry.
        let mut i = Inventory::default();
        i.slots[0] = Some(Stack::new("dwell:dirt", 3));
        i.slots[1] = Some(Stack::new("dwell:rock", 1));
        let palette = ["dwell:stone", "dwell:dirt"];
        assert_eq!(palette_slot_for(&i, 0, &palette), Some(1));
        assert_eq!(
            palette_slot_for(&i, 1, &palette),
            None,
            "a rock places nothing"
        );
        assert_eq!(palette_slot_for(&i, 2, &palette), None, "empty");
    }

    dwell_world::both_faces!(
        fn a_stick_and_a_rock_are_drawn_loose_as_their_look_not_a_cube(f: FaceFrame) {
            // Owner, 2026-11-09: "Why don't sticks and rocks in your hand and hotbar look like
            // sticks and rocks on the ground?" A loose one is its look's shape: a rod long along
            // x, a lump of 14 faces; not the stand-in block's cube.
            for (item, triangles) in [("dwell:stick", 12), ("dwell:rock", 32)] {
                let look = inv::item(item).and_then(|d| d.look).expect(item);
                let (meshes, size) = item_meshes(item, f).expect(item);
                let m = &meshes[0].opaque;
                assert_eq!(m.indices.len() / 3, triangles, "{item}");
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in m.positions.chunks_exact(3) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                let ext: Vec<f64> = (0..3).map(|k| f64::from(hi[k] - lo[k])).collect();
                for k in 0..3 {
                    assert!(
                        (ext[k] - look.size[k]).abs() < 0.02,
                        "{item} axis {k}: {ext:?}"
                    );
                }
                assert!((size - look.size[0].max(look.size[1]).max(look.size[2])).abs() < 1e-9);
                // From its point up its face's up.
                let (top, bottom) = if f.is_b() {
                    (-lo[1], -hi[1])
                } else {
                    (hi[1], lo[1])
                };
                assert!(bottom.abs() < 1e-5 && top > 0.0, "{item}: {bottom} {top}");
                // Outward: the top face's normal is up its face's up.
                let up = m
                    .normals
                    .chunks_exact(3)
                    .map(|n| n[1] * f.sign_f32())
                    .fold(f32::MIN, f32::max);
                assert!(up > 0.99, "{item}");
            }
        }
    );

    dwell_world::both_faces!(
        fn a_loose_item_is_drawn_small_and_above_its_point(f: FaceFrame) {
            for item in ["dwell:dirt", "dwell:gem"] {
                let (meshes, size) = item_meshes(item, f).expect(item);
                let ys: Vec<f32> = meshes
                    .iter()
                    .flat_map(|m| m.opaque.positions.chunks_exact(3).map(|p| p[1]))
                    .collect();
                assert!(!ys.is_empty(), "{item}");
                let (lo, hi) = ys
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
                // Face-local: from its point (0) up.
                let (lo, hi) = if f.is_b() { (-hi, -lo) } else { (lo, hi) };
                assert!(
                    lo.abs() < 1e-4 && hi > 0.0 && f64::from(hi) <= size + 1e-4,
                    "{item}: {lo} {hi}"
                );
            }
        }
    );

    #[test]
    fn drops_are_added_moved_and_removed() {
        // face-a-only: bookkeeping.
        let mut l = LooseItems::default();
        let net = |id, y: f32| ItemDropNet {
            id,
            item: "dwell:dirt".into(),
            count: 1,
            position: [0.5, y, 0.5],
        };
        let c = l.apply(ItemDrops {
            items: vec![net(1, 40.0), net(2, 40.0)],
            gone: vec![],
        });
        assert_eq!(c.added.len(), 2);
        let c = l.apply(ItemDrops {
            items: vec![net(1, 39.0)],
            gone: vec![2],
        });
        assert!(c.added.is_empty(), "a move needs no new mesh");
        assert_eq!(c.removed, [2]);
        assert_eq!(l.poses(0.0).len(), 1);
    }
}
