use super::*;
use crate::hash::HashMap;
use crate::{FLAT_TOP_Y, FaceFrame, M};

/// A fixture world written face-local and placed through `f`: air where nothing is set.
struct Fixture {
    f: FaceFrame,
    cells: HashMap<[i32; 3], StateId>,
}

const G: i32 = FLAT_TOP_Y; // the ground's row, face-local

fn state(name: &str) -> StateId {
    find_block(name).expect(name).first_state
}

impl Fixture {
    /// A grass field `size` cells square, its top at row `G`.
    fn field(f: FaceFrame, size: i32) -> Self {
        let mut w = Self {
            f,
            cells: HashMap::default(),
        };
        for x in 0..size {
            for z in 0..size {
                w.set([x, G, z], M.grass);
                w.set([x, G - 1, z], M.dirt);
            }
        }
        w
    }
    fn set(&mut self, local: [i32; 3], m: StateId) {
        self.cells.insert(self.f.cell(local), self.f.state(m));
    }
    fn get(&self) -> impl FnMut([i32; 3]) -> Option<StateId> + '_ {
        |c| Some(self.cells.get(&c).copied().unwrap_or(M.air))
    }
    /// Every item over the field (the rows just above the ground), face-local cells.
    fn scatter(&self, rules: &Rules<'_>, seed: u64, size: i32) -> Vec<([i32; 3], Placed)> {
        let mut get = self.get();
        let mut out = Vec::new();
        for x in 0..size {
            for z in 0..size {
                for y in G + 1..G + 3 {
                    if let Some(p) = rules.item_at(&mut get, seed, self.f.cell([x, y, z])) {
                        out.push(([x, y, z], p));
                    }
                }
            }
        }
        out
    }
}

fn kind_named(name: &str) -> usize {
    KINDS.iter().position(|k| k.name == name).expect(name)
}

crate::both_faces!(
    /// The same seed scatters the same items; another seed others.
    fn the_scatter_is_deterministic_from_the_seed_and_the_cell(f: FaceFrame) {
        let mut w = Fixture::field(f, 64);
        for x in 0..64 {
            for z in 0..64 {
                w.set([x, G + 8, z], state("dwell:leaves"));
            }
        }
        let rules = Rules::shipped();
        let a = w.scatter(&rules, 7, 64);
        assert!(!a.is_empty());
        assert_eq!(a, w.scatter(&rules, 7, 64));
        assert_ne!(a, w.scatter(&rules, 8, 64));
    }
);

crate::both_faces!(
    /// Sticks lie only under the canopy (leaves within the rule's rows above), at about its density.
    fn sticks_lie_only_under_leaves(f: FaceFrame) {
        let mut w = Fixture::field(f, 64);
        for x in 16..48 {
            for z in 16..48 {
                w.set([x, G + 10, z], state("dwell:leaves"));
            }
        }
        let stick = kind_named("dwell:ground_stick");
        let items = w.scatter(&Rules::shipped(), 3, 64);
        let sticks: Vec<_> = items.iter().filter(|(_, p)| p.kind == stick).collect();
        assert!(
            sticks
                .iter()
                .all(|(c, _)| (16..48).contains(&c[0]) && (16..48).contains(&c[2])),
            "a stick outside the canopy"
        );
        assert!(sticks.iter().all(|(c, _)| c[1] == G + 1), "on the ground");
        let share = sticks.len() as f64 / (32.0 * 32.0);
        assert!((0.04..0.13).contains(&share), "share {share}");
    }
);

crate::both_faces!(
    /// Rocks gather at a cliff's foot: far more cells next to a tall face get one than open grass.
    fn rocks_lie_at_cliff_feet_more_than_on_open_grass(f: FaceFrame) {
        let mut w = Fixture::field(f, 128);
        // A cliff 8 rows (4 m) tall along x = 64.. : the field's west half is its foot.
        for x in 64..128 {
            for z in 0..128 {
                for y in G + 1..G + 9 {
                    w.set([x, y, z], M.stone);
                }
            }
        }
        let rock = kind_named("dwell:ground_rock");
        let items = w.scatter(&Rules::shipped(), 11, 128);
        let rocks = |xs: std::ops::Range<i32>| {
            items
                .iter()
                .filter(|(c, p)| p.kind == rock && xs.contains(&c[0]) && c[1] == G + 1)
                .count() as f64
        };
        let foot = rocks(62..64) / (2.0 * 128.0);
        let open = rocks(0..60) / (60.0 * 128.0);
        assert!(foot > 0.07, "at the foot {foot}");
        assert!(open < 0.01, "on open grass {open}");
        assert!(open > 0.0, "some on open grass");
    }
);

crate::both_faces!(
    /// Face B's items are face A's mirrored: the same face-local cells, kinds, turns and offsets.
    fn face_b_scatters_face_a_mirrored(f: FaceFrame) {
        let build = |f: FaceFrame| {
            let mut w = Fixture::field(f, 48);
            for x in 8..24 {
                for z in 8..24 {
                    w.set([x, G + 7, z], state("dwell:leaves"));
                }
            }
            for z in 0..48 {
                for y in G + 1..G + 9 {
                    w.set([40, y, z], M.stone);
                }
            }
            w
        };
        let strip = |v: Vec<([i32; 3], Placed)>| -> Vec<_> {
            v.into_iter()
                .map(|(c, p)| (c, p.kind, p.yaw, p.scale, p.offset))
                .collect()
        };
        let rules = Rules::shipped();
        let mine = strip(build(f).scatter(&rules, 5, 48));
        let a = strip(build(FaceFrame::A).scatter(&rules, 5, 48));
        assert!(!a.is_empty());
        assert_eq!(mine, a);
    }
);

crate::both_faces!(
    /// An item rests on its ground's top, face-local: the point is the cell's bottom on its face.
    fn an_item_rests_on_the_ground_top(f: FaceFrame) {
        let w = Fixture::field(f, 4);
        let kinds = [GroundKind {
            name: "test:pebble",
            item: "dwell:rock",
            shape: GroundShape::Lump,
            size: [0.1, 0.05, 0.1],
            texture: "dwell:stone",
            color: [255, 255, 255],
            scale: [1.0, 1.0],
        }];
        let rules = [GroundRule {
            kind: 0,
            on: &["dwell:grass"],
            under: None,
            near_wall: None,
            density: 1.0,
        }];
        let r = Rules::new(&kinds, &rules);
        let p = r
            .item_at(&mut w.get(), 1, f.cell([1, G + 1, 1]))
            .expect("one on every grass cell");
        let local = f.point(p.point());
        assert!((local[1] - m_of_row(G + 1)).abs() < 1e-9, "{local:?}");
        assert!((local[0] - 1.5).abs() <= 0.45 && (local[2] - 1.5).abs() <= 0.45);
    }
);

crate::both_faces!(
    /// A kind and a rule added by data alone scatter: a pebble on every sand cell, nowhere else.
    fn a_fixture_rule_by_data_alone_scatters(f: FaceFrame) {
        let mut w = Fixture::field(f, 8);
        for x in 0..4 {
            for z in 0..8 {
                w.set([x, G, z], M.sand);
            }
        }
        let kinds = [GroundKind {
            name: "test:pebble",
            item: "dwell:rock",
            shape: GroundShape::Lump,
            size: [0.05, 0.03, 0.05],
            texture: "dwell:sand",
            color: [255, 255, 255],
            scale: [1.0, 1.0],
        }];
        let rules = [GroundRule {
            kind: 0,
            on: &["dwell:sand"],
            under: None,
            near_wall: None,
            density: 1.0,
        }];
        let items = w.scatter(&Rules::new(&kinds, &rules), 9, 8);
        assert_eq!(items.len(), 32);
        assert!(items.iter().all(|(c, _)| c[0] < 4));
    }
);

crate::both_faces!(
    /// Nothing lies on a cell that isn't open, under water, or on a slope or unknown ground.
    fn nothing_lies_in_water_on_unknown_or_unflat_ground(f: FaceFrame) {
        let mut w = Fixture::field(f, 4);
        w.set([1, G + 1, 1], M.water);
        w.set([2, G, 2], state("dwell:grass_ramp"));
        let kinds = [KINDS[0]];
        let rules = [GroundRule {
            kind: 0,
            on: &["dwell:grass", "dwell:grass_ramp"],
            under: None,
            near_wall: None,
            density: 1.0,
        }];
        let r = Rules::new(&kinds, &rules);
        assert!(r.item_at(&mut w.get(), 1, f.cell([1, G + 1, 1])).is_none());
        assert!(r.item_at(&mut w.get(), 1, f.cell([2, G + 1, 2])).is_none());
        assert!(r.item_at(&mut w.get(), 1, f.cell([3, G + 1, 3])).is_some());
        let mut unknown = |_c: [i32; 3]| None;
        assert!(r.item_at(&mut unknown, 1, f.cell([3, G + 1, 3])).is_none());
    }
);

crate::both_faces!(
    /// The crosshair finds an item it points at, not one behind a wall, nor a gone one.
    fn the_crosshair_targets_an_item_before_a_wall(f: FaceFrame) {
        let mut w = Fixture::field(f, 8);
        let kinds = [KINDS[1]];
        let rules = [GroundRule {
            kind: 0,
            on: &["dwell:grass"],
            under: None,
            near_wall: None,
            density: 1.0,
        }];
        let r = Rules::new(&kinds, &rules);
        let cell = f.cell([4, G + 1, 4]);
        let it = r.item_at(&mut w.get(), 2, cell).expect("an item");
        let p = it.point();
        let eye_local = [f.point(p)[0] - 2.0, f.point(p)[1] + 1.6, f.point(p)[2]];
        let eye = f.point(eye_local);
        let to: [f64; 3] = std::array::from_fn(|k| p[k] - eye[k]);
        let len = to.iter().map(|v| v * v).sum::<f64>().sqrt();
        let dir = to.map(|v| v / len);
        let none = HashSet::default();
        let got = r.target(&mut w.get(), 2, eye, dir, 4.0, &none);
        assert_eq!(got.map(|g| g.cell), Some(cell));
        let gone: HashSet<[i32; 3]> = [cell].into_iter().collect();
        let behind = r.target(&mut w.get(), 2, eye, dir, 4.0, &gone);
        assert_ne!(behind.map(|g| g.cell), Some(cell));
        // A wall between: nothing.
        let mid = f.point([eye_local[0] + 1.0, f.point(p)[1] + 0.8, eye_local[2]]);
        let mc = crate::cell_of_pos(mid);
        let ml = f.cell(mc);
        for y in G + 1..G + 6 {
            w.set([ml[0], y, ml[2]], M.stone);
        }
        assert!(r.target(&mut w.get(), 2, eye, dir, 4.0, &none).is_none());
    }
);

crate::both_faces!(
    /// The break action aimed at an item hits it, not the ground under it behind it (ADR 0071,
    /// amended 2026-11-09); a block in front of it along the ray is hit instead.
    fn the_break_action_hits_a_nearer_item_before_the_block_behind_it(f: FaceFrame) {
        let w = Fixture::field(f, 8);
        let kinds = [KINDS[1]];
        let rules = [GroundRule {
            kind: 0,
            on: &["dwell:grass"],
            under: None,
            near_wall: None,
            density: 1.0,
        }];
        let r = Rules::new(&kinds, &rules);
        let cell = f.cell([4, G + 1, 4]);
        let it = r.item_at(&mut w.get(), 2, cell).expect("an item");
        let p = it.point();
        let eye_local = [f.point(p)[0] - 2.0, f.point(p)[1] + 1.6, f.point(p)[2]];
        let eye = f.point(eye_local);
        let to: [f64; 3] = std::array::from_fn(|k| p[k] - eye[k]);
        let len = to.iter().map(|v| v * v).sum::<f64>().sqrt();
        let dir = to.map(|v| v / len);
        // The grass the item lies on, which the ray reaches after it.
        let under = f.cell([4, G, 4]);
        assert!(r.hit_before(&it, eye, dir, Some(under)), "the item first");
        assert!(r.hit_before(&it, eye, dir, None));
        // A block halfway along the ray, in front of it.
        let mid: [f64; 3] = std::array::from_fn(|k| eye[k] + dir[k] * len * 0.4);
        let front = crate::cell_of_pos(mid);
        assert!(!r.hit_before(&it, eye, dir, Some(front)), "the block first");
    }
);

#[test]
fn the_gone_set_round_trips_and_a_newer_record_is_not_read() {
    // both-faces: a byte format, no face.
    let mut g = GroundGone::default();
    g.cells.insert([1, -2, 3]);
    g.cells.insert([40, 500, -7]);
    let b = g.to_bytes();
    assert_eq!(GroundGone::from_bytes(&b), Some(g.clone()));
    let mut newer = b.clone();
    newer[0] = 2;
    assert_eq!(GroundGone::from_bytes(&newer), None);
    assert_eq!(GroundGone::from_bytes(&b[..b.len() - 1]), None);
    assert_eq!(g.in_chunk([1, 15, -1]), vec![[8, 20, 25]]);
}

crate::both_faces!(
    /// The rules read the terrain as generated (owner, 2026-11-09: "keep sticks under cleared
    /// leaves"): the leaves cleared over a stick leave it; its ground broken takes it.
    fn a_stick_stays_under_cleared_leaves(f: FaceFrame) {
        let mut generated = Fixture::field(f, 8);
        for x in 0..8 {
            for z in 0..8 {
                generated.set([x, G + 6, z], state("dwell:leaves"));
            }
        }
        let rules = [GroundRule {
            density: 1.0,
            ..RULES[0]
        }];
        let r = Rules::new(&KINDS, &rules);
        let cell = f.cell([3, G + 1, 3]);
        let mut now = Fixture {
            f,
            cells: generated.cells.clone(),
        };
        for x in 0..8 {
            for z in 0..8 {
                now.set([x, G + 6, z], M.air);
            }
        }
        let mut get = |c: [i32; 3], gen_: bool| {
            let w = if gen_ { &generated } else { &now };
            Some(w.cells.get(&c).copied().unwrap_or(M.air))
        };
        let it = r.item_over(&mut get, 4, cell).expect("the stick stays");
        assert_eq!(KINDS[it.kind].name, "dwell:ground_stick");
        // Its ground broken (as now): gone.
        now.set([3, G, 3], M.air);
        let mut get = |c: [i32; 3], gen_: bool| {
            let w = if gen_ { &generated } else { &now };
            Some(w.cells.get(&c).copied().unwrap_or(M.air))
        };
        assert!(r.item_over(&mut get, 4, cell).is_none());
    }
);
