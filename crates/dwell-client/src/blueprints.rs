//! The world's blueprints on the client (WORK_QUEUE item 55, docs/BLUEPRINTS.md): kept from the
//! server's `Blueprints` (`blueprints.world`), the one the view aims into, its ghost (the cells left,
//! in the build colour where the player has the block, red where not) for the brush preview's
//! renderer, the panel's have/need rows, and the held Place's pace.

use std::collections::HashMap;

use dwell_blocks::StateId;
use dwell_game::brush_tool::{BrushOutline, GhostFace, GhostKind};
use dwell_protocol::blueprints::BlueprintsMessage;
use dwell_world::blueprint::{Ghost, GhostCell, Need, Rules, SHIPPED, cost_item};
use dwell_world::{CELL_H, pos_of_cell};

/// How far a ghost face stands out from its cell (m), so it shows over a block beside it.
const INSET: f32 = 0.02;
/// Ghost outlines carry versions in their own space (the brush's count up from 0).
const VERSION_BASE: u64 = 1 << 62;

/// What the HUD's blueprint panel shows (`ClientBlueprints::panel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub name: String,
    pub rows: Vec<Need>,
    /// Rows past the panel's most, folded into one.
    pub more: usize,
    /// Cells left to build.
    pub left: usize,
}

/// The blueprints the server sent, and the held Place's state.
pub struct ClientBlueprints {
    pub rules: Rules,
    pub ghosts: Vec<Ghost>,
    /// The aimed one's outline, its key (id, cells left, what the player has) and version.
    outline: Option<BrushOutline>,
    key: Option<(u32, usize, u64)>,
    version: u64,
    /// Place held into a blueprint, and when its next piece may go.
    pub held: bool,
    pub next_ms: f64,
}

impl Default for ClientBlueprints {
    fn default() -> Self {
        Self {
            rules: SHIPPED,
            ghosts: Vec::new(),
            outline: None,
            key: None,
            version: VERSION_BASE,
            held: false,
            next_ms: 0.0,
        }
    }
}

impl ClientBlueprints {
    /// The server's news.
    pub fn apply(&mut self, m: BlueprintsMessage) {
        match m {
            BlueprintsMessage::All(list) => {
                self.ghosts = list.iter().map(Ghost::of).collect();
            }
            BlueprintsMessage::Added(b) => {
                self.ghosts.retain(|g| g.id != b.id);
                self.ghosts.push(Ghost::of(&b));
            }
            BlueprintsMessage::Removed(id) => self.ghosts.retain(|g| g.id != id),
            _ => {}
        }
        self.key = None;
    }

    /// The blueprint the view aims into within `reach` m: the one whose box the ray enters first.
    pub fn aimed(&self, eye: [f64; 3], dir: [f64; 3], reach: f64) -> Option<usize> {
        let step = 0.25 * CELL_H;
        let mut t = 0.0;
        while t <= reach {
            let c = dwell_world::cell_of_pos(std::array::from_fn(|i| eye[i] + dir[i] * t));
            if let Some(i) = self.ghosts.iter().position(|g| g.contains(c)) {
                return Some(i);
            }
            t += step;
        }
        None
    }

    /// The ghost of blueprint `i`'s cells left (none aimed: no ghost), each cell's kind by whether
    /// the player `has` its block: `Put` (the build colour) or `Remove` (red). Rebuilt only when
    /// the blueprint, its cells left or what the player has changed (`have_key`).
    pub fn update_outline(
        &mut self,
        i: Option<usize>,
        get: &dyn Fn([i32; 3]) -> Option<StateId>,
        has: &dyn Fn(StateId) -> bool,
        have_key: u64,
    ) {
        let Some(g) = i.and_then(|i| self.ghosts.get(i)) else {
            self.outline = None;
            self.key = None;
            return;
        };
        let left: Vec<&GhostCell> = g.remaining(get).collect();
        let key = (g.id, left.len(), have_key);
        if self.key == Some(key) && self.outline.is_some() {
            return;
        }
        self.key = Some(key);
        let kinds: HashMap<[i32; 3], GhostKind> = left
            .iter()
            .map(|c| {
                let k = if has(c.state) {
                    GhostKind::Put
                } else {
                    GhostKind::Remove
                };
                (c.world, k)
            })
            .collect();
        let faces = ghost_faces(&kinds, g.min, self.rules.ghost_face_cap);
        self.version += 1;
        self.outline = Some(BrushOutline {
            origin: pos_of_cell(g.min),
            segments: Vec::new(),
            over_cap: faces.len() >= self.rules.ghost_face_cap,
            faces,
            cells: left.len(),
            version: self.version,
        });
    }

    /// The aimed blueprint's ghost, for the renderer.
    pub fn outline(&self) -> Option<&BrushOutline> {
        self.outline.as_ref()
    }

    /// The panel for blueprint `i`: its name, each item still needed with what the player has
    /// (`have`, by item name), the cells left.
    pub fn panel(
        &self,
        i: usize,
        get: &dyn Fn([i32; 3]) -> Option<StateId>,
        have: &dyn Fn(&str) -> u32,
    ) -> Option<Panel> {
        let g = self.ghosts.get(i)?;
        let (mut rows, left) = g.needs(get, have);
        let more = rows.len().saturating_sub(self.rules.panel_rows);
        rows.truncate(self.rules.panel_rows);
        Some(Panel {
            name: g.name.clone(),
            rows,
            more,
            left,
        })
    }

    /// Whether the player has a cell's item (`count` by item name; unlimited blocks: always).
    pub fn has_with(state: StateId, count: &dyn Fn(&str) -> u32) -> bool {
        count(cost_item(state)) > 0
    }
}

/// The faces of `kinds`' cells that don't touch another of the same kind, relative to the corner
/// of cell `origin` (m: a cell is 1 m across and `CELL_H` tall), at most `cap` of them.
pub fn ghost_faces(
    kinds: &HashMap<[i32; 3], GhostKind>,
    origin: [i32; 3],
    cap: usize,
) -> Vec<GhostFace> {
    let size = [1.0f32, CELL_H as f32, 1.0];
    let mut cells: Vec<(&[i32; 3], &GhostKind)> = kinds.iter().collect();
    cells.sort_by_key(|(c, _)| (c[1], c[2], c[0]));
    let mut out = Vec::new();
    for (p, &k) in cells {
        for (f, d) in dwell_world::edit::FACE_DIRS.iter().enumerate() {
            let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
            if kinds.get(&n) == Some(&k) {
                continue;
            }
            if out.len() >= cap {
                return out;
            }
            let a = f / 2;
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let lo: [f32; 3] = std::array::from_fn(|i| (p[i] - origin[i]) as f32 * size[i] - INSET);
            let hi: [f32; 3] =
                std::array::from_fn(|i| (p[i] - origin[i] + 1) as f32 * size[i] + INSET);
            let mut base = lo;
            if d[a] > 0 {
                base[a] = hi[a];
            }
            let at = |du: bool, dv: bool| {
                let mut q = base;
                if du {
                    q[u] = hi[u];
                }
                if dv {
                    q[v] = hi[v];
                }
                q
            };
            // e_u × e_v = e_a: (0, u, u+v, v) turns anticlockwise about +a.
            let quad = if d[a] > 0 {
                [
                    at(false, false),
                    at(true, false),
                    at(true, true),
                    at(false, true),
                ]
            } else {
                [
                    at(false, false),
                    at(false, true),
                    at(true, true),
                    at(true, false),
                ]
            };
            out.push((quad, k));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_protocol::blueprints::Blueprint;
    use dwell_world::blueprint::record_of;
    use dwell_world::{FaceFrame, M, both_faces};

    fn wall(f: FaceFrame, id: u32) -> Blueprint {
        let s = dwell_world::structure::load("dwell:bp_wall").unwrap();
        Blueprint {
            id,
            ..record_of(&s, f, [0, 64, 0], 0).unwrap()
        }
    }

    both_faces!(
        fn the_ghost_shows_cells_left_red_where_the_player_lacks_the_block(f: FaceFrame) {
            let mut bp = ClientBlueprints::default();
            bp.apply(BlueprintsMessage::All(vec![wall(f, 3)]));
            assert_eq!(bp.ghosts.len(), 1);
            let get = |c: [i32; 3]| Some(if f.cell(c)[1] < 64 { M.stone } else { M.air });
            // Aimed from in front of the wall, level with its middle (row 66: 33 m).
            let eye = f.point([0.5, 33.25, 6.0]);
            let dir = f.vector([0.0, 0.0, -1.0]);
            assert_eq!(bp.aimed(eye, dir, 8.0), Some(0));
            assert_eq!(bp.aimed(eye, f.vector([0.0, 0.0, 1.0]), 8.0), None);
            let none = |_: StateId| false;
            bp.update_outline(Some(0), &get, &none, 0);
            let o = bp.outline().unwrap();
            assert!(!o.faces.is_empty());
            assert!(
                o.faces.iter().all(|(_, k)| *k == GhostKind::Remove),
                "all red"
            );
            let all = |_: StateId| true;
            bp.update_outline(Some(0), &get, &all, 1);
            let o = bp.outline().unwrap();
            assert!(o.faces.iter().all(|(_, k)| *k == GhostKind::Put));
            // A solid slab of cells: only its outside faces, ½ m tall rows.
            let g = &bp.ghosts[0];
            let cells = g.cells.len();
            assert_eq!(o.cells, cells);
            let tops = o
                .faces
                .iter()
                .flat_map(|(q, _)| q.iter().map(|v| v[1]))
                .fold(0.0f32, f32::max);
            assert!(
                (tops - (7.0 * CELL_H as f32 + INSET)).abs() < 1e-4,
                "{tops}"
            );
            // Removed: no ghost.
            bp.apply(BlueprintsMessage::Removed(3));
            assert!(bp.ghosts.is_empty());
            bp.update_outline(bp.aimed(eye, dir, 8.0), &get, &all, 1);
            assert!(bp.outline().is_none());
        }
    );

    #[test]
    fn the_panel_shows_have_and_need_and_folds_rows_past_its_most() {
        // face-a-only: the panel's counts, which the face doesn't change (dwell-world tests both).
        let f = FaceFrame::A;
        let mut bp = ClientBlueprints::default();
        let s = dwell_world::structure::load("dwell:bp_hut").unwrap();
        bp.apply(BlueprintsMessage::Added(Blueprint {
            id: 1,
            ..record_of(&s, f, [0, 64, 0], 0).unwrap()
        }));
        let get = |c: [i32; 3]| Some(if c[1] < 64 { M.stone } else { M.air });
        let have = |n: &str| if n == "dwell:log" { 12 } else { 3 };
        let p = bp.panel(0, &get, &have).unwrap();
        assert_eq!(p.name, "dwell:bp_hut");
        assert_eq!(p.left, 111);
        assert_eq!(
            p.rows,
            vec![
                Need {
                    block: "dwell:log",
                    have: 12,
                    need: 61
                },
                Need {
                    block: "dwell:stone",
                    have: 3,
                    need: 50
                }
            ]
        );
        assert_eq!(p.more, 0);
        bp.rules.panel_rows = 1;
        let p = bp.panel(0, &get, &have).unwrap();
        assert_eq!((p.rows.len(), p.more), (1, 1));
    }
}
