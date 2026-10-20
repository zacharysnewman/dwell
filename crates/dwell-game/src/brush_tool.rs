//! The brush tool (docs/BRUSHES.md; VoxelSniper-style painting and sculpting in worlds whose type
//! allows brushes): a preset from `shared/brushes/brushes.json`, changed by keys (brush, performer,
//! size), aimed along the view ray up to the world's brush reach, previewed at the target as a
//! see-through ghost of the exact cells the stroke would change, and applied as one undo step of
//! `KEEP` pastes through the region tools. Like every edit, a stroke is predicted: it shows at once
//! and is reconciled with the server's acks (`edit_prediction.rs`).
//!
//! Left click is VoxelSniper's arrow (the brush centred on the aimed-at block), right click its
//! gunpowder (centred on the cell in front of it, where a block would be placed); a middle or Alt
//! click picks the aimed-at block as the replace target.

use dwell_blocks::{StateId, info};
use dwell_world::brush::{
    Aim, BRUSH_KINDS, BrushKind, BrushSpec, Change, Grid, MAX_SIZE, MIN_SIZE, Materials,
    PERFORMER_KINDS, PRESETS, PerformerKind, bounds, brush_cells, paste_edits, perform,
    read_bounds,
};
use dwell_world::edit::FACE_DIRS;
use std::collections::HashMap;

use crate::block_interaction::BlockTarget;
use crate::region_tools::{BoxSource, RegionTools, ToolContext};

/// What a key asks of the brush tool ([`brush_key`]; the controls table in
/// `client/src/ui/controls.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushAction {
    /// The brush tool on or off.
    Toggle,
    NextPreset,
    PreviousPreset,
    NextBrush,
    PreviousBrush,
    NextPerformer,
    PreviousPerformer,
    Bigger,
    Smaller,
    /// The preset at this index of `PRESETS` (its data `key`).
    Preset(usize),
    /// The exact-cell preview on or off (`\`).
    Preview,
}

/// The brush key `code` (`KeyboardEvent.code`) is, with Ctrl/⌘ up: V always (where brushes are
/// allowed), the others only while the tool is on.
pub fn brush_key(code: &str, shift: bool, active: bool) -> Option<BrushAction> {
    use BrushAction as B;
    if code == "KeyV" {
        return Some(B::Toggle);
    }
    if !active {
        return None;
    }
    Some(match code {
        "KeyE" => B::NextPreset,
        "KeyQ" => B::PreviousPreset,
        "KeyZ" if shift => B::PreviousBrush,
        "KeyZ" => B::NextBrush,
        "KeyP" if shift => B::PreviousPerformer,
        "KeyP" => B::NextPerformer,
        "BracketRight" => B::Bigger,
        "BracketLeft" => B::Smaller,
        "Backslash" => B::Preview,
        _ => {
            return PRESETS
                .iter()
                .position(|p| p.key == Some(code))
                .map(B::Preset);
        }
    })
}

/// The most ghost faces a preview draws; a stroke whose ghost would have more shows the box of
/// its cells instead (a size-16 ball's surface is about 7,000 faces, a dense splatter's 30,000).
pub const GHOST_FACE_CAP: usize = 16_384;

/// How often a preview whose aim and brush are unchanged is rebuilt for the world under it (ms).
pub const PREVIEW_REFRESH_MS: f64 = 250.0;

/// What a ghost cell becomes (its colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GhostKind {
    /// A block where there was air or water.
    Put,
    /// Air or water where there was a block.
    Remove,
    /// Another block where there was one.
    Paint,
}

/// One face of the ghost: its corners relative to the preview's `origin` (m), anticlockwise seen
/// from outside, and what its cell becomes.
pub type GhostFace = ([[f32; 3]; 4], GhostKind);

/// The brush's preview at its target, for the renderer (BRUSHES.md §3): the ghost of the exact
/// cells a stroke would change — their outer faces, a little proud of the cells — and lines (a
/// line's or triangle's clicks; the box of the brush's cells when there is no ghost), relative to
/// `origin` (m).
#[derive(Clone, Debug, PartialEq)]
pub struct BrushOutline {
    pub origin: [f64; 3],
    pub segments: Vec<([f32; 3], [f32; 3])>,
    pub faces: Vec<GhostFace>,
    /// Cells the stroke would change.
    pub cells: usize,
    /// The ghost would have had more than `GHOST_FACE_CAP` faces.
    pub over_cap: bool,
    /// New with each different picture: the renderer uploads only a new one.
    pub version: u64,
}

/// How far a ghost face stands out from its cell (m), so it shows over the terrain's own face.
const GHOST_INSET: f32 = 0.02;

/// A box's twelve edges.
fn cuboid(segments: &mut Vec<([f32; 3], [f32; 3])>, lo: [f32; 3], hi: [f32; 3]) {
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        for (sb, sc) in [
            (lo[b], lo[c]),
            (hi[b], lo[c]),
            (lo[b], hi[c]),
            (hi[b], hi[c]),
        ] {
            let (mut p, mut q) = ([0.0; 3], [0.0; 3]);
            (p[a], q[a]) = (lo[a], hi[a]);
            (p[b], q[b], p[c], q[c]) = (sb, sb, sc, sc);
            segments.push((p, q));
        }
    }
}

fn solid(m: StateId) -> bool {
    dwell_blocks::try_info(m).is_some_and(|s| s.solid)
}

/// The ghost of `changes` (what they do to `grid`'s cells), relative to the centre of cell `c`:
/// each changed cell's faces that don't touch another changed cell of the same kind, or `None` if
/// that is more than `cap` faces.
pub fn ghost_faces(
    changes: &[Change],
    grid: &Grid,
    c: [i32; 3],
    cap: usize,
) -> Option<Vec<GhostFace>> {
    let kinds: HashMap<[i32; 3], GhostKind> = changes
        .iter()
        .map(|&(p, new)| {
            let old = grid.get(p);
            let k = match (solid(old), solid(new)) {
                (false, true) => GhostKind::Put,
                (true, false) => GhostKind::Remove,
                _ => GhostKind::Paint,
            };
            (p, k)
        })
        .collect();
    let mut out = Vec::new();
    for &(p, _) in changes {
        let k = kinds[&p];
        for (f, d) in FACE_DIRS.iter().enumerate() {
            let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
            if kinds.get(&n) == Some(&k) {
                continue;
            }
            if out.len() == cap {
                return None;
            }
            let a = f / 2;
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            let lo: [f32; 3] = std::array::from_fn(|i| (p[i] - c[i]) as f32 - 0.5 - GHOST_INSET);
            let hi: [f32; 3] = std::array::from_fn(|i| (p[i] - c[i]) as f32 + 0.5 + GHOST_INSET);
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
    Some(out)
}

/// What the HUD's brush panel shows (`BrushTool::panel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrushPanel {
    pub preset: &'static str,
    pub brush: &'static str,
    pub performer: &'static str,
    pub size: u8,
    /// The hotbar's block, by name.
    pub material: String,
    /// What a replace (or a target mask) replaces, when the performer uses it.
    pub replace: Option<String>,
    /// The last action's message.
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct BrushTool {
    /// The server takes brushes in this world (`welcome_flags::BRUSHES`).
    pub available: bool,
    /// How far along the view the tool aims (m): the world type's brush reach.
    pub reach: f32,
    active: bool,
    preset: usize,
    spec: BrushSpec,
    target: Option<BlockTarget>,
    replace: Option<StateId>,
    /// The earlier clicks of a line or triangle (oldest first).
    points: Vec<[i32; 3]>,
    note: String,
    /// The exact-cell preview is on (`\`); off, the box of the brush's cells shows.
    ghost: bool,
    preview: Option<BrushOutline>,
    /// What the preview was built from, and when.
    preview_key: Option<(Aim, BrushSpec, Materials, bool)>,
    preview_ms: f64,
    version: u64,
}

impl Default for BrushTool {
    fn default() -> Self {
        Self::new()
    }
}

fn step<T: Copy + PartialEq>(all: &[T], now: T, by: i32) -> T {
    let i = all.iter().position(|&k| k == now).unwrap_or(0) as i32;
    all[(i + by).rem_euclid(all.len() as i32) as usize]
}

fn block_name(m: StateId) -> String {
    let name = info(m).name;
    let base = name.split('[').next().unwrap_or(name);
    base.split(':').nth(1).unwrap_or(base).replace('_', " ")
}

impl BrushTool {
    pub fn new() -> Self {
        Self {
            available: false,
            reach: 0.0,
            active: false,
            preset: 0,
            spec: PRESETS[0].spec(),
            target: None,
            replace: None,
            points: Vec::new(),
            note: String::new(),
            ghost: true,
            preview: None,
            preview_key: None,
            preview_ms: f64::NEG_INFINITY,
            version: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.active && self.available
    }

    pub fn spec(&self) -> BrushSpec {
        self.spec
    }

    pub fn preset(&self) -> usize {
        self.preset
    }

    pub fn replace_target(&self) -> Option<StateId> {
        self.replace
    }

    pub fn target(&self) -> Option<BlockTarget> {
        self.target
    }

    /// Runs a key's action; a status note says what changed.
    pub fn key(&mut self, action: BrushAction) {
        use BrushAction as B;
        self.note.clear();
        if !self.available {
            self.active = false;
            if action == B::Toggle {
                self.note = "Brushes are for sandbox worlds".into();
            }
            return;
        }
        let set_preset = |t: &mut Self, i: usize| {
            t.preset = i % PRESETS.len();
            t.spec = PRESETS[t.preset].spec();
            t.points.clear();
        };
        match action {
            B::Toggle => {
                self.active = !self.active;
                self.points.clear();
            }
            B::NextPreset => set_preset(self, self.preset + 1),
            B::PreviousPreset => set_preset(self, self.preset + PRESETS.len() - 1),
            B::Preset(i) => set_preset(self, i),
            B::NextBrush | B::PreviousBrush => {
                // Stencils and trees take their blocks from their preset's data: the key keeps to
                // the shapes, and leaves a stencil or tree as it is.
                if !self.spec.brush.own_blocks() {
                    let by = if action == B::NextBrush { 1 } else { -1 };
                    let shapes: Vec<BrushKind> = BRUSH_KINDS
                        .into_iter()
                        .filter(|b| !b.own_blocks())
                        .collect();
                    self.spec.brush = step(&shapes, self.spec.brush, by);
                    if self.spec.performer == PerformerKind::Stamp {
                        self.spec.performer = PerformerKind::Set;
                    }
                }
                self.points.clear();
            }
            B::NextPerformer | B::PreviousPerformer => {
                // Only a stencil or tree stamps.
                let by = if action == B::NextPerformer { 1 } else { -1 };
                let own = self.spec.brush.own_blocks();
                let performers: Vec<PerformerKind> = PERFORMER_KINDS
                    .into_iter()
                    .filter(|p| own || *p != PerformerKind::Stamp)
                    .collect();
                self.spec.performer = step(&performers, self.spec.performer, by);
            }
            B::Bigger | B::Smaller => self.resize(if action == B::Bigger { 1 } else { -1 }),
            B::Preview => {
                self.ghost = !self.ghost;
                self.note = if self.ghost {
                    "Preview: the exact cells"
                } else {
                    "Preview: the brush's box"
                }
                .into();
            }
        }
    }

    /// Size up (+) or down (−), within `MIN_SIZE..=MAX_SIZE`.
    pub fn resize(&mut self, by: i32) {
        self.spec.size =
            (i32::from(self.spec.size) + by).clamp(i32::from(MIN_SIZE), i32::from(MAX_SIZE)) as u8;
    }

    /// The block the view ray hits within the brush reach (each frame).
    pub fn aim(&mut self, target: Option<BlockTarget>) {
        self.target = target;
    }

    /// The cell a stroke is centred on: the aimed-at block (arrow), or the cell in front of the
    /// face aimed at (gunpowder).
    pub fn center(&self, gunpowder: bool) -> Option<[i32; 3]> {
        let t = self.target?;
        if !gunpowder {
            return Some(t.cell);
        }
        let d = FACE_DIRS
            .get(usize::from(t.face))
            .copied()
            .unwrap_or([0; 3]);
        Some(std::array::from_fn(|i| t.cell[i] + d[i]))
    }

    fn aim_at(&self, center: [i32; 3]) -> Aim {
        let n = self.points.len();
        Aim {
            center,
            face: self.target.map_or(2, |t| t.face),
            from: n.checked_sub(1).map(|i| self.points[i]),
            before: n.checked_sub(2).map(|i| self.points[i]),
        }
    }

    /// Picks the replace target: the aimed-at block's state.
    pub fn pick(&mut self, state: Option<StateId>) {
        self.note.clear();
        match state {
            Some(m) => {
                self.replace = Some(m);
                self.note = format!("Replace target: {}", block_name(m));
            }
            None => self.note = "Aim at a block to pick".into(),
        }
    }

    /// The cells and changes of a stroke at `center` on `world` (what [`apply`](Self::apply)
    /// sends): `None` where the world isn't loaded.
    pub fn changes(
        &self,
        center: [i32; 3],
        materials: Materials,
        world: &mut impl BoxSource,
    ) -> Option<Vec<([i32; 3], StateId)>> {
        self.stroke_at(center, materials, world).map(|(c, _)| c)
    }

    /// A stroke's changes at `center` and the grid of the world they were read from.
    fn stroke_at(
        &self,
        center: [i32; 3],
        materials: Materials,
        world: &mut impl BoxSource,
    ) -> Option<(Vec<Change>, Grid)> {
        let aim = self.aim_at(center);
        let cells = brush_cells(&self.spec, aim);
        let (min, size) = read_bounds(&self.spec, &cells)?;
        let grid = Grid::new(min, size, world.read_box(min, size)?);
        Some((perform(&self.spec, aim, &cells, &grid, materials), grid))
    }

    /// Applies the brush (left click: arrow; right click: gunpowder) with the hotbar's block,
    /// through the region tools as one undo step. True if it queued edits.
    pub fn apply(
        &mut self,
        gunpowder: bool,
        ctx: &ToolContext,
        region: &mut RegionTools,
        world: &mut impl BoxSource,
    ) -> bool {
        self.note.clear();
        region.set_note("");
        let Some(center) = self.center(gunpowder) else {
            self.note = "Aim at a block".into();
            return false;
        };
        let needs = self.spec.brush.points();
        if self.points.len() < needs {
            self.points.push(center);
            self.note = match (self.spec.brush, self.points.len()) {
                (BrushKind::Triangle, 1) => "Triangle's first corner: click the second",
                (BrushKind::Triangle, _) => "Second corner: click the third",
                _ => "Line starts here: click its other end",
            }
            .into();
            return false;
        }
        let materials = self.materials(ctx.material, ctx.target_state);
        let Some(changes) = self.changes(center, materials, world) else {
            self.note = "Not loaded yet".into();
            return false;
        };
        let n = changes.len();
        let sent = region.stroke(paste_edits(&changes), ctx, world);
        if sent {
            self.note = format!("{n} blocks");
            // A line chains from its end; a triangle starts again.
            match self.spec.brush {
                BrushKind::Line | BrushKind::JaggedLine => self.points = vec![center],
                BrushKind::Triangle => self.points.clear(),
                _ => {}
            }
        } else {
            self.note = region.note().to_owned();
        }
        sent
    }

    /// What a stroke's replace uses: the picked block, else the aimed-at block's state.
    pub fn materials(&self, material: StateId, target_state: StateId) -> Materials {
        Materials {
            material,
            target: self
                .replace
                .unwrap_or(self.target.map_or(0, |_| target_state)),
        }
    }

    /// The exact-cell preview on or off (`\`).
    pub fn ghost(&self) -> bool {
        self.ghost
    }

    /// Rebuilds the preview where the stroke would go (arrow) when what it depends on changed — the
    /// aim, the brush, the blocks — or, for the world under it, at most every
    /// [`PREVIEW_REFRESH_MS`] (each frame; `materials` as for a stroke).
    pub fn update_preview(
        &mut self,
        materials: Materials,
        world: &mut impl BoxSource,
        now_ms: f64,
    ) {
        let Some(c) = self.center(false).filter(|_| self.active()) else {
            self.preview = None;
            self.preview_key = None;
            return;
        };
        let key = (self.aim_at(c), self.spec, materials, self.ghost);
        if self.preview_key == Some(key) && now_ms - self.preview_ms < PREVIEW_REFRESH_MS {
            return;
        }
        let rebuilt = self.build_preview(c, materials, world, GHOST_FACE_CAP);
        // The same picture (the world under it unchanged) keeps its version: nothing re-uploads.
        let same = self.preview.as_ref().is_some_and(|p| {
            p.origin == rebuilt.origin && p.segments == rebuilt.segments && p.faces == rebuilt.faces
        });
        if !same {
            self.version += 1;
            self.preview = Some(BrushOutline {
                version: self.version,
                ..rebuilt
            });
        }
        self.preview_key = Some(key);
        self.preview_ms = now_ms;
    }

    /// The preview at `c`: the ghost of the cells the stroke would change (at most `cap` faces),
    /// the clicks a line or triangle has so far, and the box of the brush's cells when there is no
    /// ghost (it is off, too large, or the world isn't loaded).
    pub fn build_preview(
        &self,
        c: [i32; 3],
        materials: Materials,
        world: &mut impl BoxSource,
        cap: usize,
    ) -> BrushOutline {
        let origin = dwell_world::centre_of_cell(c);
        let rel = |p: [i32; 3]| -> [f32; 3] { std::array::from_fn(|i| (p[i] - c[i]) as f32) };
        let mut segments = Vec::new();
        let mut faces = Vec::new();
        let mut cells = 0;
        let mut over = false;
        let ready = self.points.len() >= self.spec.brush.points();
        if self.spec.brush.points() > 0 {
            // The clicks so far joined to the aim, with a box at each.
            let t = f32::from(self.spec.size) - 0.5;
            let mut corners: Vec<[f32; 3]> = self.points.iter().map(|&p| rel(p)).collect();
            corners.push([0.0; 3]);
            for w in corners.windows(2) {
                segments.push((w[0], w[1]));
            }
            if self.spec.brush == BrushKind::Triangle && corners.len() == 3 {
                segments.push((corners[2], corners[0]));
            }
            for k in corners {
                cuboid(
                    &mut segments,
                    k.map(|v| v - t - 0.5),
                    k.map(|v| v + t + 0.5),
                );
            }
        }
        let mut ghosted = false;
        if self.ghost
            && ready
            && let Some((changes, grid)) = self.stroke_at(c, materials, world)
        {
            cells = changes.len();
            match ghost_faces(&changes, &grid, c, cap) {
                Some(f) => {
                    faces = f;
                    ghosted = true;
                }
                None => over = true,
            }
        }
        if !ghosted && ready {
            let covered = brush_cells(&self.spec, self.aim_at(c));
            if let Some((min, size)) = bounds(&covered) {
                let lo: [f32; 3] = std::array::from_fn(|i| (min[i] - c[i]) as f32 - 0.5);
                let hi: [f32; 3] = std::array::from_fn(|i| lo[i] + size[i] as f32);
                cuboid(&mut segments, lo, hi);
            }
        }
        // Built in cells; the preview is in metres (HALF_HEIGHT_GRID.md §2).
        let h = dwell_world::CELL_H as f32;
        let m = |p: [f32; 3]| [p[0], p[1] * h, p[2]];
        let segments = segments.into_iter().map(|(p, q)| (m(p), m(q))).collect();
        let faces = faces.into_iter().map(|(q, k)| (q.map(m), k)).collect();
        BrushOutline {
            origin,
            segments,
            faces,
            cells,
            over_cap: over,
            version: 0,
        }
    }

    /// The preview where the stroke would go (arrow), as last built, or none.
    pub fn outline(&self) -> Option<&BrushOutline> {
        self.preview.as_ref().filter(|_| self.active())
    }

    /// The HUD panel's fields while the tool is on.
    pub fn panel(&self, material: StateId) -> Option<BrushPanel> {
        self.active().then(|| {
            let s = &self.spec;
            BrushPanel {
                preset: PRESETS[self.preset].label,
                brush: s.brush.label(),
                performer: s.performer.label(),
                size: s.size,
                material: block_name(material),
                replace: s
                    .performer
                    .uses_target(s.params.mask)
                    .then(|| self.replace.map_or("aimed-at block".into(), block_name)),
                note: self.note.clone(),
            }
        })
    }

    pub fn note(&self) -> &str {
        &self.note
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::require;
    use dwell_protocol::Message;
    use dwell_world::brush::{KEEP, PerformerKind, preset};
    use dwell_world::edit::EditCapsule;
    use dwell_world::region::{RegionCheck, check_region_edit, read_box};
    use dwell_world::{M, VoxelWorld, empty_generator};

    fn world() -> VoxelWorld {
        VoxelWorld::new(empty_generator(), None)
    }

    fn tool() -> BrushTool {
        let mut t = BrushTool {
            available: true,
            reach: 128.0,
            ..BrushTool::new()
        };
        t.key(BrushAction::Toggle);
        t
    }

    fn region() -> RegionTools {
        let mut r = RegionTools::new();
        (r.enabled, r.keep, r.reach) = (true, true, 128.0);
        r
    }

    fn ctx(material: StateId, now_ms: f64) -> ToolContext {
        ToolContext {
            target: None,
            target_state: 0,
            placement: None,
            material,
            eye: Some([0.5, 10.0, 0.5]),
            now_ms,
        }
    }

    /// The server's side: validates and applies what the tools send.
    fn apply_all(w: &mut VoxelWorld, msgs: Vec<Message>) {
        for m in msgs {
            let Message::RegionEdit(r) = m else {
                panic!("{m:?}")
            };
            let out = check_region_edit(w, &r, [0.5, 10.0, 0.5], &[] as &[EditCapsule], 140.0);
            assert_eq!(out.check, RegionCheck::Ok);
            for (c, s) in out.changes {
                w.set_voxel(c[0], c[1], c[2], s);
            }
        }
    }

    fn snapshot(w: &mut VoxelWorld) -> Vec<StateId> {
        read_box(w, [-20, -4, -20], [41, 30, 41]).expect("reads")
    }

    fn select(t: &mut BrushTool, name: &str) {
        let i = PRESETS.iter().position(|p| p.name == name).expect(name);
        t.key(BrushAction::Preset(i));
    }

    #[test]
    fn keys_switch_brush_performer_size_and_presets() {
        assert_eq!(brush_key("KeyV", false, false), Some(BrushAction::Toggle));
        assert_eq!(
            brush_key("KeyE", false, false),
            None,
            "only while the tool is on"
        );
        assert_eq!(
            brush_key("KeyE", false, true),
            Some(BrushAction::NextPreset)
        );
        assert_eq!(
            brush_key("KeyZ", true, true),
            Some(BrushAction::PreviousBrush)
        );
        assert_eq!(
            brush_key("BracketRight", false, true),
            Some(BrushAction::Bigger)
        );
        // A preset's key is its data's.
        let with_key = PRESETS.iter().position(|p| p.key.is_some()).unwrap();
        let key = PRESETS[with_key].key.unwrap();
        assert_eq!(
            brush_key(key, false, true),
            Some(BrushAction::Preset(with_key))
        );
        // Movement and the region tools' keys stay theirs.
        for k in [
            "KeyW", "KeyA", "KeyS", "KeyD", "KeyC", "Space", "KeyB", "KeyG", "KeyR", "KeyX",
        ] {
            assert_eq!(brush_key(k, false, true), None, "{k}");
        }

        let mut t = tool();
        assert!(t.active());
        t.key(BrushAction::NextBrush);
        assert_eq!(t.spec().brush, BrushKind::Disc);
        t.key(BrushAction::PreviousBrush);
        t.key(BrushAction::PreviousBrush);
        assert_eq!(
            t.spec().brush,
            BrushKind::Snipe,
            "wraps, past the stencil and tree"
        );
        t.key(BrushAction::NextPerformer);
        assert_eq!(t.spec().performer, PerformerKind::Replace);
        for _ in 0..40 {
            t.key(BrushAction::Bigger);
        }
        assert_eq!(t.spec().size, MAX_SIZE);
        for _ in 0..40 {
            t.resize(-1);
        }
        assert_eq!(t.spec().size, MIN_SIZE);
        select(&mut t, "smooth");
        assert_eq!(t.spec(), preset("smooth").unwrap().spec());
        t.key(BrushAction::NextPreset);
        assert_eq!(
            t.preset(),
            PRESETS.iter().position(|p| p.name == "smooth").unwrap() + 1
        );
        // Not in a world without brushes.
        let mut off = BrushTool::new();
        off.key(BrushAction::Toggle);
        assert!(!off.active());
        assert!(off.note().contains("sandbox"));
    }

    #[test]
    fn a_stroke_is_one_undo_step_and_round_trips() {
        let (stone, floor) = (require("dwell:stone"), require("dwell:sandbox_floor"));
        let mut w = world();
        for z in -20..=20 {
            for x in -20..=20 {
                w.set_voxel(x, 0, z, floor);
            }
        }
        let original = snapshot(&mut w);
        let mut t = tool();
        let mut r = region();
        select(&mut t, "ball");
        for _ in 0..20 {
            t.key(BrushAction::Bigger); // the largest: 33 a side, several edits
        }
        t.aim(Some(BlockTarget {
            cell: [0, 0, 0],
            face: 2,
        }));
        assert!(t.apply(true, &ctx(stone, 0.0), &mut r, &mut w));
        let sent = r.pump(0.0);
        assert!(sent.len() > 1, "{}", sent.len());
        assert_eq!(r.progress(), None);
        apply_all(&mut w, sent);
        assert_eq!(w.get_voxel(0, 1, 0), stone);
        assert_eq!(w.get_voxel(0, 0, 0), floor);
        let painted = snapshot(&mut w);
        assert_eq!(r.undo_depth(), (1, 0));
        // Undo (several edits, queued) puts everything back; redo paints it again.
        let none = r.act(crate::region_tools::ToolAction::Undo, &ctx(0, 1.0), &mut w);
        assert_eq!(none, None);
        // The bucket has to refill first: progress shows while it waits.
        let first = r.pump(1.0);
        if r.progress().is_some() {
            assert!(r.status().contains("Sending"), "{}", r.status());
        }
        apply_all(&mut w, first);
        apply_all(&mut w, r.pump(5000.0));
        assert_eq!(snapshot(&mut w), original);
        assert_eq!(r.undo_depth(), (0, 1));
        r.act(
            crate::region_tools::ToolAction::Redo,
            &ctx(0, 6000.0),
            &mut w,
        );
        apply_all(&mut w, r.pump(10_000.0));
        assert_eq!(snapshot(&mut w), painted);
    }

    #[test]
    fn strokes_wait_for_the_rate_and_show_progress() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = tool();
        let mut r = region();
        select(&mut t, "cube");
        for _ in 0..20 {
            t.key(BrushAction::Bigger);
        }
        // Five full strokes far apart: more than the burst, so the last ones wait.
        let mut sent = 0;
        let mut waited = false;
        for i in 0..5 {
            t.aim(Some(BlockTarget {
                cell: [i * 40 - 80, 10, 0],
                face: 2,
            }));
            let now = f64::from(i) * 10.0;
            if !t.apply(false, &ctx(stone, now), &mut r, &mut w) {
                assert!(t.note().contains("Still sending"), "{}", t.note());
                waited = true;
                let mut later = now;
                while r.progress().is_some() {
                    later += 100.0;
                    sent += r.pump(later).len();
                }
                continue;
            }
            sent += r.pump(now).len();
        }
        assert!(waited);
        assert!(sent >= 8);
    }

    #[test]
    fn out_of_reach_older_servers_and_lines() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = tool();
        let mut r = region();
        t.aim(Some(BlockTarget {
            cell: [200, 0, 0],
            face: 2,
        }));
        assert!(!t.apply(false, &ctx(stone, 0.0), &mut r, &mut w));
        assert!(t.note().contains("Too far"), "{}", t.note());
        // An older server (no KEEP cells): nothing is sent.
        let mut old = RegionTools::new();
        old.enabled = true;
        t.aim(Some(BlockTarget {
            cell: [5, 0, 0],
            face: 2,
        }));
        assert!(!t.apply(false, &ctx(stone, 0.0), &mut old, &mut w));
        assert!(t.note().contains("doesn't take brushes"), "{}", t.note());
        // A line: the first click sets its start, the second draws to it.
        select(&mut t, "line");
        assert!(!t.apply(false, &ctx(stone, 0.0), &mut r, &mut w));
        assert!(t.note().contains("Line starts"));
        t.aim(Some(BlockTarget {
            cell: [5, 0, 10],
            face: 2,
        }));
        assert!(t.apply(false, &ctx(stone, 0.0), &mut r, &mut w));
        apply_all(&mut w, r.pump(0.0));
        for z in 0..=10 {
            assert_eq!(w.get_voxel(5, 0, z), stone, "{z}");
        }
    }

    #[test]
    fn stencils_keep_their_shape_and_only_they_stamp() {
        let mut t = tool();
        select(&mut t, "tree_snipe");
        assert_eq!(
            (t.spec().brush, t.spec().performer),
            (BrushKind::Stencil, PerformerKind::Stamp)
        );
        t.key(BrushAction::NextBrush);
        assert_eq!(t.spec().brush, BrushKind::Stencil, "Z keeps a stencil");
        for _ in 0..PERFORMER_KINDS.len() {
            t.key(BrushAction::NextPerformer);
        }
        assert_eq!(
            t.spec().performer,
            PerformerKind::Stamp,
            "it cycles through stamp"
        );
        select(&mut t, "ball");
        for _ in 0..PERFORMER_KINDS.len() * 2 {
            t.key(BrushAction::NextPerformer);
            assert_ne!(
                t.spec().performer,
                PerformerKind::Stamp,
                "a ball has no blocks to stamp"
            );
        }
    }

    #[test]
    fn a_triangle_takes_three_clicks() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = tool();
        let mut r = region();
        select(&mut t, "triangle");
        let click = |t: &mut BrushTool, r: &mut RegionTools, w: &mut VoxelWorld, c: [i32; 3]| {
            t.aim(Some(BlockTarget { cell: c, face: 2 }));
            t.apply(false, &ctx(stone, 0.0), r, w)
        };
        assert!(!click(&mut t, &mut r, &mut w, [0, 0, 0]));
        assert!(t.note().contains("first corner"), "{}", t.note());
        assert!(!click(&mut t, &mut r, &mut w, [8, 0, 0]));
        assert!(t.note().contains("third"), "{}", t.note());
        // The preview joins the corners so far to the aim, with the ghost of the triangle the
        // third click would make.
        t.update_preview(t.materials(stone, 0), &mut w, 0.0);
        let o = t.outline().expect("preview");
        assert!(!o.segments.is_empty() && !o.faces.is_empty());
        assert!(click(&mut t, &mut r, &mut w, [0, 0, 8]));
        apply_all(&mut w, r.pump(0.0));
        for c in [[0, 0, 0], [8, 0, 0], [0, 0, 8], [2, 0, 2]] {
            assert_eq!(w.get_voxel(c[0], c[1], c[2]), stone, "{c:?}");
        }
        // It starts again.
        assert!(!click(&mut t, &mut r, &mut w, [3, 0, 3]));
    }

    #[test]
    fn replace_uses_the_picked_block_and_overlay_paints() {
        let (stone, sand, grass) = (
            require("dwell:stone"),
            require("dwell:sand"),
            require("dwell:grass"),
        );
        let mut w = world();
        for z in -6..=6 {
            for x in -6..=6 {
                w.set_voxel(x, 0, z, stone);
                w.set_voxel(x, -1, z, stone);
            }
        }
        w.set_voxel(1, 0, 1, sand);
        let mut t = tool();
        let mut r = region();
        select(&mut t, "replace");
        t.pick(Some(sand));
        t.aim(Some(BlockTarget {
            cell: [0, 0, 0],
            face: 2,
        }));
        assert!(t.apply(false, &ctx(grass, 0.0), &mut r, &mut w));
        apply_all(&mut w, r.pump(0.0));
        assert_eq!(w.get_voxel(1, 0, 1), grass);
        assert_eq!(w.get_voxel(0, 0, 0), stone);
        select(&mut t, "overlay");
        assert!(t.apply(false, &ctx(sand, 0.0), &mut r, &mut w));
        apply_all(&mut w, r.pump(0.0));
        assert_eq!((w.get_voxel(2, 0, 2), w.get_voxel(2, -1, 2)), (sand, stone));
        // The preview follows the target; nothing shows while the tool is off.
        t.update_preview(t.materials(grass, 0), &mut w, 0.0);
        let o = t.outline().expect("preview");
        assert_eq!(o.origin, dwell_world::centre_of_cell([0, 0, 0]));
        assert!(!o.faces.is_empty());
        t.key(BrushAction::Toggle);
        assert_eq!(t.outline(), None);
        t.update_preview(t.materials(grass, 0), &mut w, 0.0);
        assert_eq!(t.outline(), None);
        // KEEP never reaches the world.
        assert!(!snapshot(&mut w).contains(&KEEP));
        assert_eq!(w.get_voxel(0, 1, 0), M.air);
    }

    /// The cells a ghost's faces belong to (each face's centre moved half a cell inward).
    fn ghost_cells(o: &BrushOutline) -> Vec<[i32; 3]> {
        // The preview is in metres; its cells are found in cell space (HALF_HEIGHT_GRID.md §2).
        let c = dwell_world::cell_of_pos(o.origin);
        let h = dwell_world::CELL_H as f32;
        let mut out: Vec<[i32; 3]> = o
            .faces
            .iter()
            .map(|(q, _)| {
                let q = q.map(|p| [p[0], p[1] / h, p[2]]);
                let centre: [f32; 3] = std::array::from_fn(|i| (q[0][i] + q[2][i]) / 2.0);
                let e1: [f32; 3] = std::array::from_fn(|i| q[1][i] - q[0][i]);
                let e2: [f32; 3] = std::array::from_fn(|i| q[3][i] - q[0][i]);
                let n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let len = n.iter().map(|v| v * v).sum::<f32>().sqrt();
                std::array::from_fn(|i| (centre[i] - 0.5 * n[i] / len).round() as i32 + c[i])
            })
            .collect();
        out.sort_by_key(|p| [p[1], p[2], p[0]]);
        out.dedup();
        out
    }

    fn aim_at(t: &mut BrushTool, cell: [i32; 3]) {
        t.aim(Some(BlockTarget { cell, face: 2 }));
    }

    #[test]
    fn the_preview_is_a_ghost_of_the_exact_cells() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = tool();
        // A splatter's ghost is its real cells (faces point outward: each lands in its cell).
        select(&mut t, "splatter");
        aim_at(&mut t, [5, 10, -3]);
        let m = t.materials(stone, 0);
        t.update_preview(m, &mut w, 0.0);
        let o = t.outline().expect("preview").clone();
        let changed: Vec<[i32; 3]> = t
            .changes([5, 10, -3], m, &mut w)
            .unwrap()
            .iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(o.cells, changed.len());
        assert_eq!(ghost_cells(&o), changed);
        assert!(o.faces.iter().all(|(_, k)| *k == GhostKind::Put));
        assert!(o.segments.is_empty(), "no box when the ghost shows");
        // Only the outer faces: a size-1 cube is 27 cells and 54 faces.
        select(&mut t, "cube");
        t.resize(-10);
        t.update_preview(m, &mut w, 0.0);
        assert_eq!(t.outline().unwrap().faces.len(), 54);
        // What a cell becomes picks its colour: an erase of stone is a removal.
        for y in 0..3 {
            w.set_voxel(5, 10 + y, -3, stone);
        }
        select(&mut t, "erase");
        t.update_preview(m, &mut w, 0.0);
        let o = t.outline().unwrap();
        assert_eq!(o.cells, 3, "the stone cells");
        assert!(o.faces.iter().all(|(_, k)| *k == GhostKind::Remove));
        select(&mut t, "replace");
        t.pick(Some(stone));
        t.update_preview(t.materials(require("dwell:sand"), 0), &mut w, 0.0);
        assert!(
            t.outline()
                .unwrap()
                .faces
                .iter()
                .all(|(_, k)| *k == GhostKind::Paint)
        );
    }

    #[test]
    fn the_preview_is_bounded_toggled_and_rebuilt_only_when_needed() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = tool();
        select(&mut t, "splatter");
        aim_at(&mut t, [0, 20, 0]);
        let m = t.materials(stone, 0);
        // Past the cap, the box of the brush's cells shows instead.
        let big = t.build_preview([0, 20, 0], m, &mut w, 10);
        assert!(big.over_cap && big.faces.is_empty());
        assert_eq!(big.segments.len(), 12);
        let full = t.build_preview([0, 20, 0], m, &mut w, GHOST_FACE_CAP);
        assert!(!full.over_cap && full.faces.len() > 10);
        // The largest, densest splatter stays within the cap or falls back to the box.
        t.resize(20);
        t.update_preview(m, &mut w, 0.0);
        assert!(t.outline().unwrap().faces.len() <= GHOST_FACE_CAP);
        t.resize(-20);
        // \ turns the ghost off: the box shows.
        assert_eq!(
            brush_key("Backslash", false, true),
            Some(BrushAction::Preview)
        );
        t.key(BrushAction::Preview);
        assert!(!t.ghost());
        t.update_preview(m, &mut w, 1000.0);
        let o = t.outline().unwrap();
        assert!(o.faces.is_empty() && o.segments.len() == 12);
        t.key(BrushAction::Preview);
        t.update_preview(m, &mut w, 2000.0);
        let v = t.outline().unwrap().version;
        // Unchanged, it keeps its version (nothing re-uploads), even when refreshed.
        t.update_preview(m, &mut w, 2010.0);
        t.update_preview(m, &mut w, 5000.0);
        assert_eq!(t.outline().unwrap().version, v);
        // A change under it shows within the refresh interval, not before.
        let first = t.changes([0, 20, 0], m, &mut w).unwrap()[0].0;
        w.set_voxel(first[0], first[1], first[2], stone);
        t.update_preview(m, &mut w, 5100.0);
        assert_eq!(t.outline().unwrap().version, v);
        t.update_preview(m, &mut w, 5000.0 + PREVIEW_REFRESH_MS);
        assert!(t.outline().unwrap().version > v);
        // Moving the aim rebuilds at once.
        let v = t.outline().unwrap().version;
        aim_at(&mut t, [3, 20, 0]);
        t.update_preview(m, &mut w, 5300.0);
        assert!(t.outline().unwrap().version > v);
    }
}
