//! The brush tool (docs/BRUSHES.md; VoxelSniper-style painting and sculpting in worlds whose type
//! allows brushes): a preset from `shared/brushes/brushes.json`, changed by keys (brush, performer,
//! size), aimed along the view ray up to the world's brush reach, outlined at the target, and
//! applied as one undo step of `KEEP` pastes through the region tools. Like every edit, nothing is
//! predicted: the change shows when the server's VoxelModification arrives.
//!
//! Left click is VoxelSniper's arrow (the brush centred on the aimed-at block), right click its
//! gunpowder (centred on the cell in front of it, where a block would be placed); a middle or Alt
//! click picks the aimed-at block as the replace target.

use dwell_blocks::{StateId, info};
use dwell_world::brush::{
    Aim, BRUSH_KINDS, BrushKind, BrushSpec, Grid, MAX_SIZE, MIN_SIZE, Materials, PERFORMER_KINDS,
    PRESETS, PerformerKind, bounds, brush_cells, paste_edits, perform, read_bounds,
};
use dwell_world::edit::FACE_DIRS;

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
        _ => {
            return PRESETS
                .iter()
                .position(|p| p.key == Some(code))
                .map(B::Preset);
        }
    })
}

/// A wireframe of the brush at its target, for the renderer: segments relative to `origin` (m).
#[derive(Clone, Debug, PartialEq)]
pub struct BrushOutline {
    pub origin: [f64; 3],
    pub segments: Vec<([f32; 3], [f32; 3])>,
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

const CIRCLE_SEGMENTS: usize = 48;

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
        let aim = self.aim_at(center);
        let cells = brush_cells(&self.spec, aim);
        let (min, size) = read_bounds(&self.spec, &cells)?;
        let grid = Grid::new(min, size, world.read_box(min, size)?);
        Some(perform(&self.spec, aim, &cells, &grid, materials))
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
        let target = match self.replace {
            Some(m) => m,
            None => self.target.map_or(0, |_| ctx.target_state),
        };
        let materials = Materials {
            material: ctx.material,
            target,
        };
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

    /// The outline of the brush where it would go (arrow), or none.
    pub fn outline(&self) -> Option<BrushOutline> {
        if !self.active() {
            return None;
        }
        let c = self.center(false)?;
        let origin = c.map(|v| f64::from(v) + 0.5);
        let r = f32::from(self.spec.size) + 0.5;
        let mut segments = Vec::new();
        let circle =
            |segments: &mut Vec<_>, axes: (usize, usize), radii: (f32, f32), at: [f32; 3]| {
                for i in 0..CIRCLE_SEGMENTS {
                    let p = |k: usize| {
                        let a = k as f32 / CIRCLE_SEGMENTS as f32 * std::f32::consts::TAU;
                        let mut v = at;
                        v[axes.0] += radii.0 * a.cos();
                        v[axes.1] += radii.1 * a.sin();
                        v
                    };
                    segments.push((p(i), p(i + 1)));
                }
            };
        let cuboid = |segments: &mut Vec<_>, lo: [f32; 3], hi: [f32; 3]| {
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
        };
        match self.spec.brush {
            BrushKind::Ball | BrushKind::Splatter | BrushKind::Ellipsoid => {
                let s = if self.spec.brush == BrushKind::Ellipsoid {
                    self.spec.params.stretch
                } else {
                    [1.0; 3]
                };
                let k = s.map(|v| v * r);
                circle(&mut segments, (0, 2), (k[0], k[2]), [0.0; 3]);
                circle(&mut segments, (0, 1), (k[0], k[1]), [0.0; 3]);
                circle(&mut segments, (2, 1), (k[2], k[1]), [0.0; 3]);
            }
            BrushKind::Disc | BrushKind::Cylinder => {
                let h = if self.spec.brush == BrushKind::Disc {
                    1.0
                } else {
                    f32::from(self.spec.params.height)
                };
                let (lo, hi) = (-0.5, h - 0.5);
                circle(&mut segments, (0, 2), (r, r), [0.0, lo, 0.0]);
                circle(&mut segments, (0, 2), (r, r), [0.0, hi, 0.0]);
                for (x, z) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                    segments.push(([x, lo, z], [x, hi, z]));
                }
            }
            BrushKind::Cube => {
                let e = r;
                cuboid(&mut segments, [-e; 3], [e; 3]);
            }
            BrushKind::Line | BrushKind::JaggedLine | BrushKind::Triangle => {
                let t = r;
                // The clicks so far and this one, joined.
                let mut corners: Vec<[f32; 3]> = self
                    .points
                    .iter()
                    .map(|f| std::array::from_fn(|i| (f[i] - c[i]) as f32))
                    .collect();
                corners.push([0.0; 3]);
                for w in corners.windows(2) {
                    segments.push((w[0], w[1]));
                }
                if self.spec.brush == BrushKind::Triangle && corners.len() == 3 {
                    segments.push((corners[2], corners[0]));
                }
                for k in corners {
                    cuboid(&mut segments, k.map(|v| v - t), k.map(|v| v + t));
                }
            }
            _ => {
                // The box of the cells it would cover.
                let cells = brush_cells(&self.spec, self.aim_at(c));
                if let Some((min, size)) = bounds(&cells) {
                    let lo: [f32; 3] = std::array::from_fn(|i| (min[i] - c[i]) as f32 - 0.5);
                    let hi: [f32; 3] = std::array::from_fn(|i| lo[i] + size[i] as f32);
                    cuboid(&mut segments, lo, hi);
                }
            }
        }
        Some(BrushOutline { origin, segments })
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
        // The outline joins the corners so far to the aim.
        assert!(t.outline().is_some());
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
        // The outline follows the target; nothing outlines while the tool is off.
        let o = t.outline().expect("outline");
        assert_eq!(o.origin, [0.5, 0.5, 0.5]);
        assert!(!o.segments.is_empty());
        t.key(BrushAction::Toggle);
        assert_eq!(t.outline(), None);
        // KEEP never reaches the world.
        assert!(!snapshot(&mut w).contains(&KEEP));
        assert_eq!(w.get_voxel(0, 1, 0), M.air);
    }
}
