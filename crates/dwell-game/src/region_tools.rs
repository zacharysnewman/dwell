//! The region tools (ARCHITECTURE.md §6.5; structure sandbox S3): a two-corner box selection, fill,
//! hollow, replace and clear with the hotbar's block, copy, paste with a ghost box (rotated and
//! mirrored on paste), and undo/redo as a client-side stack of region diffs replayed as region
//! edits. Each edit is a `RegionEdit` the server validates; like single edits, it is predicted
//! (`edit_prediction.rs`): the client shows it at once and reconciles with the server's acks.
//!
//! A diff keeps the box's states read from the client's world just before the edit is sent: undo
//! pastes them back (and keeps what it replaced, for redo). Edits only go to a server that
//! announced `welcome_flags::REGION_EDITS`.
//!
//! A brush stroke (BRUSHES.md, `brush_tool.rs`) is one undo step of several pastes whose untouched
//! cells are `KEEP`; its edits wait in a queue and go out as the server's cell rate allows
//! ([`RegionTools::pump`]). With a server that takes `KEEP` cells (`welcome_flags::BRUSHES`),
//! undo keeps unbreakable cells as `KEEP` too, since no paste may put one down.

use std::collections::VecDeque;

use dwell_blocks::StateId;
use dwell_protocol::{Message, RegionEdit, RegionEditOp, region_edit};
use dwell_world::FaceFrame;
use dwell_world::VoxelWorld;
use dwell_world::edit::unbreakable;
use dwell_world::region::{KEEP, PasteTransform, box_distance_sq, box_in_world_of, read_box};

/// Undo steps kept (each holds its boxes' states twice; a brush stroke's boxes are trimmed to
/// the cells it changes).
pub const UNDO_DEPTH: usize = 64;

/// The most cells a structure loaded from the library may have (a memory guard, 8 MB of states):
/// the library's own limit is data (`shared/structures/library.json`: `tileSize` × `maxTiles`,
/// enforced where it is saved and read, `client/src/local/structureLibrary.ts`); a test checks
/// that limit fits under this one.
pub const MAX_CLIPBOARD_CELLS: usize = 1 << 22;

/// What a key asks of the tools (bindings: [`tool_for_key`], the controls table in
/// `client/src/ui/controls.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    /// The aimed-at block is the selection's first corner (and, until the second is set, its
    /// only cell).
    CornerA,
    CornerB,
    Deselect,
    /// The selection set to the hotbar's block.
    Fill,
    /// The selection's shell set to the hotbar's block, air inside.
    Hollow,
    /// The aimed-at block's state, within the selection, turned into the hotbar's block.
    Replace,
    /// The selection set to air.
    Clear,
    Copy,
    /// Paste mode on or off (the ghost box follows the crosshair).
    PasteMode,
    Rotate,
    Mirror,
    /// Pastes at the ghost box (right click in paste mode).
    Paste,
    Undo,
    Redo,
}

/// The tool a key press asks for: `code` is `KeyboardEvent.code`; `command` is Ctrl or ⌘ held.
pub fn tool_for_key(code: &str, command: bool, shift: bool) -> Option<ToolAction> {
    use ToolAction as T;
    if command {
        return match code {
            "KeyC" => Some(T::Copy),
            "KeyV" => Some(T::PasteMode),
            "KeyZ" if shift => Some(T::Redo),
            "KeyZ" => Some(T::Undo),
            "KeyY" => Some(T::Redo),
            _ => None,
        };
    }
    match code {
        "KeyB" => Some(T::CornerA),
        "KeyN" => Some(T::CornerB),
        "KeyX" => Some(T::Deselect),
        "KeyG" => Some(T::Fill),
        "KeyH" => Some(T::Hollow),
        "KeyT" => Some(T::Replace),
        "Delete" | "Backspace" => Some(T::Clear),
        "KeyR" => Some(T::Rotate),
        "KeyM" => Some(T::Mirror),
        _ => None,
    }
}

/// What the tools read from the client: a box of states, or `None` where its chunks are not loaded.
pub trait BoxSource {
    fn read_box(&mut self, min: [i32; 3], size: [i32; 3]) -> Option<Vec<StateId>>;
}

impl BoxSource for VoxelWorld {
    fn read_box(&mut self, min: [i32; 3], size: [i32; 3]) -> Option<Vec<StateId>> {
        read_box(self, min, size)
    }
}

/// The moment's view of the player for a tool action.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolContext {
    /// The aimed-at block.
    pub target: Option<[i32; 3]>,
    /// Its state.
    pub target_state: StateId,
    /// The cell a block would be placed in (next to the target, through the aimed-at face).
    pub placement: Option<[i32; 3]>,
    /// The hotbar's block.
    pub material: StateId,
    pub eye: Option<[f64; 3]>,
    pub now_ms: f64,
}

/// A box the renderer outlines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolBox {
    pub min: [i32; 3],
    pub size: [i32; 3],
    pub kind: ToolBoxKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolBoxKind {
    Selection,
    /// Where a paste would go.
    Ghost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Clipboard {
    size: [i32; 3],
    cells: Vec<StateId>,
    /// Cells that keep what is in the world where they land (a structure's `null` cells); empty
    /// for a copy, which sets every cell.
    keep: Vec<bool>,
    /// A saved structure from the library (`load_clipboard`), not a copy: its paste is a structure
    /// placement (`RegionEditOp::Structure` to a server that takes it).
    structure: bool,
}

/// One edit of an undo step: the box and its states before `edit` (what undo pastes back; `KEEP`
/// where it leaves a cell alone).
#[derive(Clone, Debug, PartialEq)]
struct Diff {
    min: [i32; 3],
    size: [u8; 3],
    before: Vec<StateId>,
    edit: RegionEdit,
}

#[derive(Clone, Debug)]
pub struct RegionTools {
    /// The server takes region edits (`welcome_flags::REGION_EDITS`).
    pub enabled: bool,
    /// The server takes `KEEP` paste cells and brushes (`welcome_flags::BRUSHES`).
    pub keep: bool,
    /// How far from the eye an edit's nearest cell may be (m): `region_edit::REACH`, or the world
    /// type's brush reach (`dwell_world::region::region_reach`, without its slack).
    pub reach: f32,
    /// The world's mode places saved structures (`modes::Ability::Structures`, creative only for
    /// now): [`load_clipboard`](Self::load_clipboard) refuses without it.
    pub structures: bool,
    /// The server takes structure placements as such (the `structures.place` capability,
    /// ARCHITECTURE.md §8.3): a library structure pastes as `RegionEditOp::Structure`, which the
    /// server refuses outside a mode with `structures`. Without it (an older server), plain
    /// `Paste`s, validated as any paste (STRUCTURES.md §9).
    pub structure_ops: bool,
    /// The world's disc and build volume (its size, `Game::set_world`): a box outside them is not
    /// sent (BUGS.md #109).
    pub bounds: dwell_world::WorldBounds,
    a: Option<[i32; 3]>,
    b: Option<[i32; 3]>,
    clipboard: Option<Clipboard>,
    paste: Option<PasteTransform>,
    ghost: Option<[i32; 3]>,
    /// Steps of one or more edits (a brush stroke has several).
    undo: Vec<Vec<Diff>>,
    redo: Vec<Vec<Diff>>,
    /// Edits waiting for the rate, and how many the last batch had.
    /// With the id edit prediction gave each (`tag`), once it has.
    queue: VecDeque<(RegionEdit, Option<u32>)>,
    queued: usize,
    /// Cells the server's rate allows now (its token bucket, mirrored).
    credit: f64,
    credit_ms: f64,
    note: String,
}

impl Default for RegionTools {
    fn default() -> Self {
        Self::new()
    }
}

fn dims(size: [u8; 3]) -> [i32; 3] {
    size.map(i32::from)
}

fn fits(size: [i32; 3]) -> Option<[u8; 3]> {
    size.iter()
        .all(|&s| s >= 1 && s as usize <= region_edit::MAX_SIDE)
        .then(|| size.map(|s| s as u8))
}

fn size_text(s: [i32; 3]) -> String {
    format!("{}×{}×{}", s[0], s[1], s[2])
}

/// A clipboard's keep mask moved as `PasteTransform::apply` moves its cells.
fn transform_mask(p: PasteTransform, size: [i32; 3], keep: &[bool]) -> Vec<bool> {
    let out_size = p.size(size);
    let mut out = vec![false; keep.len()];
    let mut i = 0;
    for y in 0..size[1] {
        for z in 0..size[2] {
            for x in 0..size[0] {
                let [nx, ny, nz] = p.cell(size, [x, y, z]);
                out[(nx + out_size[0] * (nz + out_size[2] * ny)) as usize] = keep[i];
                i += 1;
            }
        }
    }
    out
}

impl RegionTools {
    pub fn new() -> Self {
        Self {
            enabled: false,
            keep: false,
            reach: region_edit::REACH,
            structures: false,
            structure_ops: false,
            bounds: dwell_world::WorldBounds::DEFAULT,
            a: None,
            b: None,
            clipboard: None,
            paste: None,
            ghost: None,
            undo: Vec::new(),
            redo: Vec::new(),
            queue: VecDeque::new(),
            queued: 0,
            credit: region_edit::BURST_CELLS as f64,
            credit_ms: f64::NEG_INFINITY,
            note: String::new(),
        }
    }

    /// The selected box: its min corner and size (possibly larger than an edit may be).
    pub fn selection(&self) -> Option<([i32; 3], [i32; 3])> {
        let a = self.a?;
        let b = self.b.unwrap_or(a);
        let min = std::array::from_fn(|i| a[i].min(b[i]));
        let size = std::array::from_fn(|i| (a[i] - b[i]).abs() + 1);
        Some((min, size))
    }

    pub fn pasting(&self) -> bool {
        self.paste.is_some()
    }

    pub fn undo_depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// Where a paste goes for the clipboard turned as it is: the box stands on the placement cell
    /// (its bottom centre there).
    fn paste_box(&self, at: [i32; 3]) -> Option<([i32; 3], [i32; 3])> {
        let c = self.clipboard.as_ref()?;
        let size = self.paste?.size(c.size);
        // Face-local: on the placement cell, up from it; in the world through the face frame.
        let f = FaceFrame::at_row(at[1]);
        let local = [at[0] - size[0] / 2, f.row(at[1]), at[2] - size[2] / 2];
        Some((f.box_min(local, size), size))
    }

    /// Follows the crosshair (each frame): the ghost box's cell.
    pub fn aim(&mut self, placement: Option<[i32; 3]>) {
        self.ghost = placement;
    }

    /// The boxes to outline: the selection, and in paste mode the ghost.
    pub fn boxes(&self) -> Vec<ToolBox> {
        let mut out = Vec::new();
        if let Some((min, size)) = self.selection() {
            out.push(ToolBox {
                min,
                size,
                kind: ToolBoxKind::Selection,
            });
        }
        if let Some((min, size)) = self.ghost.and_then(|g| self.paste_box(g)) {
            out.push(ToolBox {
                min,
                size,
                kind: ToolBoxKind::Ghost,
            });
        }
        out
    }

    /// One line for the HUD: the selection or paste box and the last tool's message; empty when
    /// no tool is in use.
    pub fn status(&self) -> String {
        let mut parts = Vec::new();
        if let (Some(p), Some(c)) = (self.paste, &self.clipboard) {
            let turned = p.quarter_turns % 4 * 90;
            let mirrored = if p.mirror_x { ", mirrored" } else { "" };
            parts.push(format!(
                "Paste {} ({turned}°{mirrored}) · R rotate · M mirror · right click paste",
                size_text(p.size(c.size))
            ));
        } else if let Some((_, size)) = self.selection() {
            let n: i64 = size.iter().map(|&s| i64::from(s)).product();
            let over = if fits(size).is_none() {
                format!(" — at most {} a side", region_edit::MAX_SIDE)
            } else {
                String::new()
            };
            parts.push(format!("Selection {} ({n} blocks){over}", size_text(size)));
        }
        if let Some((sent, all)) = self.progress() {
            parts.push(format!("Sending {sent} of {all} edits"));
        } else if !self.note.is_empty() {
            parts.push(self.note.clone());
        }
        parts.join(" · ")
    }

    /// The last tool's message.
    pub fn note(&self) -> &str {
        &self.note
    }

    pub fn set_note(&mut self, note: impl Into<String>) {
        self.note = note.into();
    }

    /// Runs a tool: the request to send, if it makes one. Every action leaves a status note.
    pub fn act(
        &mut self,
        action: ToolAction,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
    ) -> Option<Message> {
        use ToolAction as T;
        self.note.clear();
        match action {
            T::CornerA | T::CornerB => {
                let Some(t) = ctx.target else {
                    self.note = "Aim at a block".into();
                    return None;
                };
                if action == T::CornerA {
                    self.a = Some(t);
                    self.b = None;
                } else if self.a.is_none() {
                    self.a = Some(t);
                } else {
                    self.b = Some(t);
                }
                None
            }
            T::Deselect => {
                self.a = None;
                self.b = None;
                self.paste = None;
                None
            }
            T::Fill | T::Hollow | T::Clear | T::Replace => {
                let (min, size) = self.selection().or_else(|| {
                    self.note = "Select a box first (B, N)".into();
                    None
                })?;
                let op = match action {
                    T::Hollow => RegionEditOp::Hollow,
                    T::Replace => RegionEditOp::Replace,
                    _ => RegionEditOp::Fill,
                };
                let material = if action == T::Clear { 0 } else { ctx.material };
                if action == T::Replace && ctx.target.is_none() {
                    self.note = "Aim at the block to replace".into();
                    return None;
                }
                let size = self.fit(size)?;
                let edit = RegionEdit {
                    op,
                    min,
                    size,
                    material,
                    from: ctx.target_state,
                    cells: Vec::new(),
                };
                self.submit(edit, ctx, world)
            }
            T::Copy => {
                let (min, size) = self.selection().or_else(|| {
                    self.note = "Select a box first (B, N)".into();
                    None
                })?;
                self.fit(size)?;
                let Some(cells) = world.read_box(min, size) else {
                    self.note = "Not loaded yet".into();
                    return None;
                };
                // Kept face-local (BIFACIAL_WORLD.md §11): it pastes upright on either face.
                self.clipboard = Some(Clipboard {
                    size,
                    cells: FaceFrame::of_box(min, size).box_cells(size, &cells),
                    keep: Vec::new(),
                    structure: false,
                });
                self.note = format!("Copied {}", size_text(size));
                None
            }
            T::PasteMode => {
                if self.paste.take().is_none() {
                    if self.clipboard.is_none() {
                        self.note = "Copy a box first (Ctrl+C)".into();
                    } else {
                        self.paste = Some(PasteTransform::default());
                    }
                }
                None
            }
            T::Rotate | T::Mirror => {
                if let Some(p) = &mut self.paste {
                    if action == T::Rotate {
                        p.quarter_turns = (p.quarter_turns + 1) % 4;
                    } else {
                        p.mirror_x = !p.mirror_x;
                    }
                }
                None
            }
            T::Paste => {
                let (Some(p), Some(c)) = (self.paste, self.clipboard.clone()) else {
                    return None;
                };
                let Some((min, _)) = ctx.placement.and_then(|at| self.paste_box(at)) else {
                    self.note = "Aim where to paste".into();
                    return None;
                };
                let (size, local) = p.apply(c.size, &c.cells);
                if fits(size).is_none() {
                    // A library structure over one edit's side: one undo step of tiles.
                    self.paste_tiles(p, &c, min, size, local, ctx, world);
                    return None;
                }
                let size = self.fit(size)?;
                // The clipboard is face-local: into the world through the paste's face frame.
                let f = FaceFrame::of_box(min, dims(size));
                let mut cells = f.box_cells(dims(size), &local);
                if c.keep.iter().any(|&k| k) {
                    // Kept cells take what the world holds there now: a paste sets every cell.
                    let Some(now) = world.read_box(min, dims(size)) else {
                        self.note = "Not loaded yet".into();
                        return None;
                    };
                    let kept = f.box_flags(dims(size), &transform_mask(p, c.size, &c.keep));
                    for (i, kept) in kept.into_iter().enumerate() {
                        if kept {
                            cells[i] = now[i];
                        }
                    }
                }
                let edit = RegionEdit {
                    op: self.paste_op(&c),
                    min,
                    size,
                    material: 0,
                    from: 0,
                    cells,
                };
                self.submit(edit, ctx, world)
            }
            T::Undo => self.replay(true, ctx, world),
            T::Redo => self.replay(false, ctx, world),
        }
    }

    /// The op a paste of `c` goes as: a library structure's is a structure placement where the
    /// server takes one (`structure_ops`), anything else a plain `Paste`.
    fn paste_op(&self, c: &Clipboard) -> RegionEditOp {
        if c.structure && self.structure_ops {
            RegionEditOp::Structure
        } else {
            RegionEditOp::Paste
        }
    }

    /// Puts a structure (STRUCTURES.md §4) on the clipboard and turns paste mode on, as a copy
    /// and Ctrl+V would: `cells` in layer order, `keep` (empty, or one per cell) where it keeps
    /// what is there. Like a copy, it pastes with its bottom centre on the aimed-at cell.
    ///
    /// A structure may be larger than one edit (the library's tiles, STRUCTURES.md §9): its paste
    /// goes as one undo step of edits of at most `region_edit::MAX_SIDE` a side, sent as the rate
    /// allows. Only where the world's mode places structures (`structures`).
    pub fn load_clipboard(
        &mut self,
        size: [i32; 3],
        cells: Vec<StateId>,
        keep: Vec<bool>,
        label: &str,
    ) -> Result<(), String> {
        if !self.structures {
            return Err("Saved structures are placed in creative worlds only".into());
        }
        if size.iter().any(|&s| s < 1) {
            return Err("The cells don't fill the box".into());
        }
        let volume = size.iter().map(|&n| n as usize).product::<usize>();
        if volume > MAX_CLIPBOARD_CELLS {
            return Err(format!("Too big: at most {MAX_CLIPBOARD_CELLS} blocks"));
        }
        if cells.len() != volume || !(keep.is_empty() || keep.len() == volume) {
            return Err("The cells don't fill the box".into());
        }
        self.clipboard = Some(Clipboard {
            size,
            cells,
            keep,
            structure: true,
        });
        self.paste = Some(PasteTransform::default());
        self.note = format!("{label}: aim and right click to paste");
        Ok(())
    }

    /// Pastes a clipboard larger than one edit (turned to `size`, its face-local states `local`) at
    /// `min`: tiles of at most `MAX_SIDE` a side from the box's lowest corner, each a `Paste` the
    /// server validates as any other (the world's rules: its mode, reach, the build volume), queued
    /// as one undo step. False (with a note) if a tile can't go: nothing is sent then.
    #[allow(clippy::too_many_arguments)]
    fn paste_tiles(
        &mut self,
        p: PasteTransform,
        c: &Clipboard,
        min: [i32; 3],
        size: [i32; 3],
        local: Vec<StateId>,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
    ) -> bool {
        if self.busy() {
            return false;
        }
        if !self.enabled {
            self.note = "This server doesn't take region edits (an older build)".into();
            return false;
        }
        let Some(eye) = ctx.eye else {
            return false;
        };
        if !box_in_world_of(&self.bounds, min, size) {
            self.note = "Outside the build volume".into();
            return false;
        }
        // Into the world through the paste's face frame, as a single edit's paste is.
        let f = FaceFrame::of_box(min, size);
        let cells = f.box_cells(size, &local);
        let kept = if c.keep.iter().any(|&k| k) {
            f.box_flags(size, &transform_mask(p, c.size, &c.keep))
        } else {
            Vec::new()
        };
        let side = region_edit::MAX_SIDE as i32;
        let mut edits = Vec::new();
        let mut step = Vec::new();
        for ty in (0..size[1]).step_by(side as usize) {
            for tz in (0..size[2]).step_by(side as usize) {
                for tx in (0..size[0]).step_by(side as usize) {
                    let at = [tx, ty, tz];
                    let tsize: [i32; 3] = std::array::from_fn(|i| side.min(size[i] - at[i]));
                    let tmin: [i32; 3] = std::array::from_fn(|i| min[i] + at[i]);
                    let Some(before) = world.read_box(tmin, tsize) else {
                        self.note = "Not loaded yet".into();
                        return false;
                    };
                    let mut tile = Vec::with_capacity(before.len());
                    let mut all_kept = true;
                    for y in 0..tsize[1] {
                        for z in 0..tsize[2] {
                            for x in 0..tsize[0] {
                                let i = ((at[0] + x)
                                    + size[0] * ((at[2] + z) + size[2] * (at[1] + y)))
                                    as usize;
                                let j = tile.len();
                                if kept.get(i).copied().unwrap_or(false) {
                                    // Kept cells take what the world holds there now.
                                    tile.push(before[j]);
                                } else {
                                    all_kept = false;
                                    tile.push(cells[i]);
                                }
                            }
                        }
                    }
                    if all_kept {
                        continue;
                    }
                    let edit = RegionEdit {
                        op: self.paste_op(c),
                        min: tmin,
                        size: tsize.map(|s| s as u8),
                        material: 0,
                        from: 0,
                        cells: tile,
                    };
                    if !self.in_reach(&edit, eye) {
                        return false;
                    }
                    step.push(Diff {
                        min: edit.min,
                        size: edit.size,
                        before: self.masked(before, None),
                        edit: edit.clone(),
                    });
                    edits.push(edit);
                }
            }
        }
        if edits.is_empty() {
            self.note = "Nothing to change".into();
            return false;
        }
        self.redo.clear();
        self.push_undo(step);
        self.enqueue(edits);
        true
    }

    fn fit(&mut self, size: [i32; 3]) -> Option<[u8; 3]> {
        let f = fits(size);
        if f.is_none() {
            self.note = format!("Too big: at most {} a side", region_edit::MAX_SIDE);
        }
        f
    }
    /// The server's cell bucket: refilled at CELLS_PER_SECOND up to BURST_CELLS.
    fn refill(&mut self, now_ms: f64) {
        if self.credit_ms.is_finite() {
            let refill =
                (now_ms - self.credit_ms).max(0.0) / 1000.0 * region_edit::CELLS_PER_SECOND as f64;
            self.credit = (self.credit + refill).min(region_edit::BURST_CELLS as f64);
        }
        self.credit_ms = now_ms;
    }

    fn in_reach(&mut self, edit: &RegionEdit, eye: [f64; 3]) -> bool {
        let reach = f64::from(self.reach);
        let ok = box_distance_sq(edit.min, dims(edit.size), eye) <= reach * reach;
        if !ok {
            self.note = "Too far away".into();
        }
        ok
    }

    /// A box's states as undo keeps them: with a server that takes `KEEP` cells, unbreakable
    /// cells (which no edit changes, and no paste may put down) and the cells `like` keeps are
    /// `KEEP`.
    fn masked(&self, mut states: Vec<StateId>, like: Option<&[StateId]>) -> Vec<StateId> {
        if self.keep {
            for (i, m) in states.iter_mut().enumerate() {
                if unbreakable(*m) || like.is_some_and(|l| l.get(i) == Some(&KEEP)) {
                    *m = KEEP;
                }
            }
        }
        states
    }

    /// Checks what the server will (support, the build volume, reach, rate), reads the box's states for undo, and
    /// returns the request.
    fn sendable(
        &mut self,
        edit: &RegionEdit,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
        like: Option<&[StateId]>,
    ) -> Option<Vec<StateId>> {
        if !self.enabled {
            self.note = "This server doesn't take region edits (an older build)".into();
            return None;
        }
        // The server refuses a box not wholly inside the build volume (ADR 0025).
        if !box_in_world_of(&self.bounds, edit.min, dims(edit.size)) {
            self.note = "Outside the build volume".into();
            return None;
        }
        let eye = ctx.eye?;
        if !self.in_reach(edit, eye) {
            return None;
        }
        self.refill(ctx.now_ms);
        let cost = edit.volume() as f64;
        if self.credit < cost {
            self.note = "Too fast: wait a moment".into();
            return None;
        }
        let Some(before) = world.read_box(edit.min, dims(edit.size)) else {
            self.note = "Not loaded yet".into();
            return None;
        };
        self.credit -= cost;
        Some(self.masked(before, like))
    }

    fn submit(
        &mut self,
        edit: RegionEdit,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
    ) -> Option<Message> {
        if self.busy() {
            return None;
        }
        let before = self.sendable(&edit, ctx, world, None)?;
        self.redo.clear();
        self.push_undo(vec![Diff {
            min: edit.min,
            size: edit.size,
            before,
            edit: edit.clone(),
        }]);
        Some(Message::RegionEdit(edit))
    }

    fn push_undo(&mut self, step: Vec<Diff>) {
        self.undo.push(step);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    /// Edits still waiting for the server's rate (a brush stroke, or the undo of one): no other
    /// edit starts until they are sent.
    pub fn busy(&mut self) -> bool {
        let busy = !self.queue.is_empty();
        if busy {
            self.note = "Still sending the last stroke".into();
        }
        busy
    }

    /// One brush stroke (BRUSHES.md) — `edits` are its `KEEP` pastes (`brush::paste_edits`) — as
    /// one undo step; its edits are sent by [`pump`](Self::pump) as the rate allows. False (with a
    /// note) if the server can't take it, a box is out of reach or not loaded.
    pub fn stroke(
        &mut self,
        edits: Vec<RegionEdit>,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
    ) -> bool {
        self.note.clear();
        if !self.enabled || !self.keep {
            self.note = "This server doesn't take brushes (an older build or another world)".into();
            return false;
        }
        if edits.is_empty() {
            self.note = "Nothing to change".into();
            return false;
        }
        if self.busy() {
            return false;
        }
        let Some(eye) = ctx.eye else {
            return false;
        };
        let mut step = Vec::with_capacity(edits.len());
        for edit in &edits {
            if !self.in_reach(edit, eye) {
                return false;
            }
            let Some(before) = world.read_box(edit.min, dims(edit.size)) else {
                self.note = "Not loaded yet".into();
                return false;
            };
            step.push(Diff {
                min: edit.min,
                size: edit.size,
                before: self.masked(before, Some(&edit.cells)),
                edit: edit.clone(),
            });
        }
        self.redo.clear();
        self.push_undo(step);
        self.enqueue(edits);
        true
    }

    fn enqueue(&mut self, edits: Vec<RegionEdit>) {
        self.queued = edits.len();
        self.queue.extend(edits.into_iter().map(|e| (e, None)));
    }

    /// The queued edits the server's rate allows now (each frame).
    pub fn pump(&mut self, now_ms: f64) -> Vec<Message> {
        self.pump_tagged(now_ms)
            .into_iter()
            .map(|(m, _)| m)
            .collect()
    }

    /// [`pump`](Self::pump), with each edit's prediction id (`tag`), if it has one.
    pub fn pump_tagged(&mut self, now_ms: f64) -> Vec<(Message, Option<u32>)> {
        let mut out = Vec::new();
        if self.queue.is_empty() {
            return out;
        }
        self.refill(now_ms);
        while let Some((e, _)) = self.queue.front() {
            let cost = e.volume() as f64;
            if self.credit < cost {
                break;
            }
            self.credit -= cost;
            let (e, id) = self.queue.pop_front().expect("front");
            out.push((Message::RegionEdit(e), id));
        }
        out
    }

    /// Queued edits without a prediction id yet, by their place in the queue (edit prediction
    /// shows a stroke's edits at once, before they are sent).
    pub fn untagged(&self) -> Vec<(usize, RegionEdit)> {
        self.queue
            .iter()
            .enumerate()
            .filter(|(_, (_, id))| id.is_none())
            .map(|(i, (e, _))| (i, e.clone()))
            .collect()
    }

    /// Gives the queued edit at `i` its prediction id.
    pub fn tag(&mut self, i: usize, id: u32) {
        if let Some(q) = self.queue.get_mut(i) {
            q.1 = Some(id);
        }
    }

    /// Queued edits sent of all (a stroke's progress), while any wait.
    pub fn progress(&self) -> Option<(usize, usize)> {
        (!self.queue.is_empty()).then(|| (self.queued - self.queue.len(), self.queued))
    }

    /// Undo pastes a step's earlier states back; redo sends its edits again. Either way the boxes'
    /// current states are kept, so the step can go back the other way. A step of several edits
    /// (a brush stroke) is queued for [`pump`](Self::pump).
    fn replay(
        &mut self,
        undo: bool,
        ctx: &ToolContext,
        world: &mut impl BoxSource,
    ) -> Option<Message> {
        if self.busy() {
            return None;
        }
        let from = if undo { &self.undo } else { &self.redo };
        let Some(step) = from.last().cloned() else {
            self.note = if undo {
                "Nothing to undo"
            } else {
                "Nothing to redo"
            }
            .into();
            return None;
        };
        let edit_of = |d: &Diff| {
            if undo {
                RegionEdit {
                    op: RegionEditOp::Paste,
                    min: d.min,
                    size: d.size,
                    material: 0,
                    from: 0,
                    cells: d.before.clone(),
                }
            } else {
                d.edit.clone()
            }
        };
        let mut message = None;
        let mut turned = Vec::with_capacity(step.len());
        if let [d] = step.as_slice() {
            let edit = edit_of(d);
            let current = self.sendable(&edit, ctx, world, Some(&d.before))?;
            turned.push(Diff {
                before: current,
                ..d.clone()
            });
            message = Some(Message::RegionEdit(edit));
        } else {
            let eye = ctx.eye?;
            let mut edits = Vec::with_capacity(step.len());
            for d in &step {
                let edit = edit_of(d);
                if !self.in_reach(&edit, eye) {
                    return None;
                }
                let Some(current) = world.read_box(d.min, dims(d.size)) else {
                    self.note = "Not loaded yet".into();
                    return None;
                };
                turned.push(Diff {
                    before: self.masked(current, Some(&d.before)),
                    ..d.clone()
                });
                edits.push(edit);
            }
            self.enqueue(edits);
        }
        if undo {
            self.undo.pop();
            self.redo.push(turned);
            self.note = "Undone".into();
        } else {
            self.redo.pop();
            self.push_undo(turned);
            self.note = "Redone".into();
        }
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dwell_blocks::{parse_state, require};
    use dwell_world::edit::EditCapsule;
    use dwell_world::region::{RegionCheck, check_region_edit};
    use dwell_world::{M, empty_generator};

    /// The client's world and the server's in one: requests are validated and applied at once.
    fn world() -> VoxelWorld {
        VoxelWorld::new(empty_generator(), None)
    }

    fn apply(w: &mut VoxelWorld, m: Option<Message>) {
        let Some(Message::RegionEdit(r)) = m else {
            panic!("no region edit: {m:?}")
        };
        let out = check_region_edit(w, &r, [0.5, 1.6, 0.5], &[] as &[EditCapsule], 64.0);
        assert_eq!(out.check, RegionCheck::Ok);
        for (c, s) in out.changes {
            w.set_voxel(c[0], c[1], c[2], s);
        }
    }

    fn step(t: &mut RegionTools, w: &mut VoxelWorld, a: ToolAction, c: &ToolContext) {
        let m = t.act(a, c, w);
        apply(w, m);
    }

    fn ctx(target: Option<[i32; 3]>, material: StateId) -> ToolContext {
        ToolContext {
            target,
            target_state: 0,
            placement: target.map(|t| [t[0], t[1] + 1, t[2]]),
            material,
            eye: Some([0.5, 1.6, 0.5]),
            now_ms: 0.0,
        }
    }

    fn tools() -> RegionTools {
        RegionTools {
            enabled: true,
            keep: true,
            structures: true,
            ..RegionTools::new()
        }
    }

    fn snapshot(w: &mut VoxelWorld) -> Vec<StateId> {
        read_box(w, [-4, -4, -4], [16, 12, 16]).expect("reads")
    }

    #[test]
    fn keys_pick_tools_and_command_keys_never_select_corners() {
        assert_eq!(
            tool_for_key("KeyB", false, false),
            Some(ToolAction::CornerA)
        );
        assert_eq!(tool_for_key("KeyC", true, false), Some(ToolAction::Copy));
        assert_eq!(tool_for_key("KeyZ", true, true), Some(ToolAction::Redo));
        assert_eq!(tool_for_key("KeyZ", true, false), Some(ToolAction::Undo));
        assert_eq!(tool_for_key("KeyB", true, false), None);
        // Movement keys stay movement keys.
        for k in ["KeyW", "KeyA", "KeyS", "KeyD", "KeyC", "Space"] {
            assert_eq!(tool_for_key(k, false, false), None, "{k}");
        }
    }

    #[test]
    fn a_loaded_structure_pastes_turned_and_keeps_its_kept_cells() {
        let (stone, sand) = (require("dwell:stone"), require("dwell:sand"));
        let mut w = world();
        w.set_voxel(1, 1, 0, sand); // under a kept cell: stays
        w.set_voxel(0, 1, 0, sand); // under a set cell: replaced
        let mut t = tools();
        // 2 × 1 × 1: stone at x = 0, kept at x = 1.
        assert!(
            t.load_clipboard([2, 1, 1], vec![stone, M.air], vec![false, true], "post")
                .is_ok()
        );
        assert!(t.pasting());
        assert_eq!(
            t.status(),
            "Paste 2×1×1 (0°) · R rotate · M mirror · right click paste · post: aim and right click to paste"
        );
        // Aimed at (1, 0, 0): the box stands on (1, 1, 0) with its bottom centre there.
        step(&mut t, &mut w, ToolAction::Paste, &ctx(Some([1, 0, 0]), 0));
        assert_eq!((w.get_voxel(0, 1, 0), w.get_voxel(1, 1, 0)), (stone, sand));
        // The box is now 1 × 1 × 2 on (5, 1, 4)..(5, 1, 5); the kept cell turned with it.
        w.set_voxel(5, 1, 4, sand);
        w.set_voxel(5, 1, 5, sand);
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        step(&mut t, &mut w, ToolAction::Paste, &ctx(Some([5, 0, 5]), 0));
        assert_eq!((w.get_voxel(5, 1, 4), w.get_voxel(5, 1, 5)), (stone, sand));
        assert!(
            t.load_clipboard([1 << 11, 1 << 11, 2], vec![], vec![], "huge")
                .is_err()
        );
        assert!(
            t.load_clipboard([2, 1, 1], vec![stone], vec![], "short")
                .is_err()
        );
        assert!(
            t.load_clipboard([2, 1, 1], vec![stone; 2], vec![true], "mask")
                .is_err()
        );
    }

    /// ARCHITECTURE.md §8.3, STRUCTURES.md §9: a library structure's paste is a structure
    /// placement (`Structure`) to a server that announced `structures.place`, a plain `Paste` to
    /// an older one; a copy's paste is always a `Paste`. Tiles of a larger structure likewise.
    #[test]
    fn a_library_structure_pastes_as_a_structure_placement_only_where_the_server_takes_one() {
        // face-a-only: the op is chosen before anything face-dependent (the both-faces tile test
        // covers the placement itself).
        let stone = require("dwell:stone");
        let op_of = |m: Option<Message>| match m {
            Some(Message::RegionEdit(e)) => Some(e.op),
            _ => None,
        };
        for (structure_ops, want) in [
            (true, RegionEditOp::Structure),
            (false, RegionEditOp::Paste),
        ] {
            let mut w = world();
            let mut t = RegionTools {
                structure_ops,
                ..tools()
            };
            t.load_clipboard([1, 1, 1], vec![stone], vec![], "one")
                .unwrap();
            let m = t.act(ToolAction::Paste, &ctx(Some([2, 0, 2]), 0), &mut w);
            assert_eq!(op_of(m), Some(want), "structure_ops {structure_ops}");
            // Wider than one edit: every tile goes as the same op.
            let mut t = RegionTools {
                structure_ops,
                ..tools()
            };
            t.load_clipboard([40, 1, 1], vec![stone; 40], vec![], "wide")
                .unwrap();
            assert!(
                t.act(ToolAction::Paste, &ctx(Some([2, 0, 2]), 0), &mut w)
                    .is_none()
            );
            let sent: Vec<_> = t.pump(1e9).into_iter().map(|m| op_of(Some(m))).collect();
            assert!(sent.len() >= 2, "tiles: {}", sent.len());
            assert!(sent.iter().all(|o| *o == Some(want)), "{sent:?}");
        }
        // A copy (Ctrl+C) pastes as a plain Paste whatever the server takes.
        let mut w = world();
        w.set_voxel(0, 1, 0, stone);
        let mut t = RegionTools {
            structure_ops: true,
            ..tools()
        };
        t.act(ToolAction::CornerA, &ctx(Some([0, 1, 0]), 0), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([0, 1, 0]), 0), &mut w);
        t.act(ToolAction::Copy, &ctx(None, 0), &mut w);
        t.act(ToolAction::PasteMode, &ctx(None, 0), &mut w);
        let m = t.act(ToolAction::Paste, &ctx(Some([3, 0, 3]), 0), &mut w);
        assert_eq!(op_of(m), Some(RegionEditOp::Paste));
    }

    #[test]
    fn two_corners_select_a_box_in_any_order() {
        let mut t = tools();
        let mut w = world();
        t.act(ToolAction::CornerA, &ctx(Some([5, 0, -2]), 0), &mut w);
        assert_eq!(t.selection(), Some(([5, 0, -2], [1, 1, 1])));
        t.act(ToolAction::CornerB, &ctx(Some([2, 3, 1]), 0), &mut w);
        assert_eq!(t.selection(), Some(([2, 0, -2], [4, 4, 4])));
        assert_eq!(t.status(), "Selection 4×4×4 (64 blocks)");
        assert_eq!(t.boxes().len(), 1);
        t.act(ToolAction::Deselect, &ctx(None, 0), &mut w);
        assert_eq!(t.selection(), None);
    }

    #[test]
    fn fill_then_undo_and_redo_round_trip() {
        let (stone, sand) = (require("dwell:stone"), require("dwell:sand"));
        let mut w = world();
        w.set_voxel(2, 0, 2, sand); // something to restore
        let original = snapshot(&mut w);
        let mut t = tools();
        t.act(ToolAction::CornerA, &ctx(Some([0, 0, 0]), stone), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([3, 2, 3]), stone), &mut w);
        let fill = t.act(ToolAction::Fill, &ctx(None, stone), &mut w);
        apply(&mut w, fill);
        assert_eq!(w.get_voxel(2, 0, 2), stone);
        let filled = snapshot(&mut w);
        let hollow = t.act(ToolAction::Hollow, &ctx(None, sand), &mut w);
        apply(&mut w, hollow);
        assert_eq!((w.get_voxel(1, 1, 1), w.get_voxel(0, 0, 0)), (M.air, sand));
        assert_eq!(t.undo_depth(), (2, 0));

        step(&mut t, &mut w, ToolAction::Undo, &ctx(None, 0));
        assert_eq!(snapshot(&mut w), filled);
        step(&mut t, &mut w, ToolAction::Undo, &ctx(None, 0));
        assert_eq!(snapshot(&mut w), original);
        assert_eq!(t.undo_depth(), (0, 2));
        assert_eq!(t.act(ToolAction::Undo, &ctx(None, 0), &mut w), None);

        step(&mut t, &mut w, ToolAction::Redo, &ctx(None, 0));
        assert_eq!(snapshot(&mut w), filled);
        // A new edit forgets what could be redone.
        let clear = t.act(ToolAction::Clear, &ctx(None, 0), &mut w);
        apply(&mut w, clear);
        assert_eq!(w.get_voxel(1, 1, 1), M.air);
        assert_eq!(t.undo_depth(), (2, 0));
        step(&mut t, &mut w, ToolAction::Undo, &ctx(None, 0));
        assert_eq!(snapshot(&mut w), filled);
    }

    #[test]
    fn undo_of_a_box_over_an_unbreakable_block_round_trips() {
        // The sandbox's floor is unbreakable: a fill over it leaves it, and its undo must not
        // paste it back (the server rejects a paste that puts down an unbreakable block).
        let (stone, floor) = (require("dwell:stone"), require("dwell:sandbox_floor"));
        let mut w = world();
        for x in 0..3 {
            w.set_voxel(x, 0, 0, floor);
        }
        let original = snapshot(&mut w);
        let mut t = tools();
        t.act(ToolAction::CornerA, &ctx(Some([0, 0, 0]), stone), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([2, 1, 0]), stone), &mut w);
        step(&mut t, &mut w, ToolAction::Fill, &ctx(None, stone));
        assert_eq!((w.get_voxel(1, 0, 0), w.get_voxel(1, 1, 0)), (floor, stone));
        step(&mut t, &mut w, ToolAction::Undo, &ctx(None, 0));
        assert_eq!(snapshot(&mut w), original);
        step(&mut t, &mut w, ToolAction::Redo, &ctx(None, 0));
        assert_eq!(w.get_voxel(1, 1, 0), stone);
    }

    #[test]
    fn replace_turns_the_aimed_at_state_into_the_hotbar_block() {
        let (stone, sand, dirt) = (
            require("dwell:stone"),
            require("dwell:sand"),
            require("dwell:dirt"),
        );
        let mut w = world();
        for x in 0..3 {
            w.set_voxel(x, 0, 0, if x == 1 { sand } else { stone });
        }
        let mut t = tools();
        t.act(ToolAction::CornerA, &ctx(Some([0, 0, 0]), 0), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([2, 0, 0]), 0), &mut w);
        let aim = ToolContext {
            target_state: stone,
            ..ctx(Some([0, 0, 0]), dirt)
        };
        step(&mut t, &mut w, ToolAction::Replace, &aim);
        assert_eq!(
            [
                w.get_voxel(0, 0, 0),
                w.get_voxel(1, 0, 0),
                w.get_voxel(2, 0, 0)
            ],
            [dirt, sand, dirt]
        );
    }

    #[test]
    fn copy_then_paste_turned_and_mirrored_with_its_ghost_box() {
        let stone = require("dwell:stone");
        let east = parse_state("dwell:ladder[facing=east]").expect("ladder");
        let mut w = world();
        // An L: three stones along +X, a ladder facing east on top of the first.
        for x in 0..3 {
            w.set_voxel(x, 0, 0, stone);
        }
        w.set_voxel(0, 1, 0, east);
        let mut t = tools();
        t.act(ToolAction::CornerA, &ctx(Some([0, 0, 0]), 0), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([2, 1, 0]), 0), &mut w);
        t.act(ToolAction::Copy, &ctx(None, 0), &mut w);
        assert_eq!(t.act(ToolAction::PasteMode, &ctx(None, 0), &mut w), None);
        assert!(t.pasting());
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        // The ghost stands on the cell above the aimed-at block (10, 0, 10), centred.
        t.aim(Some([10, 1, 10]));
        let ghost = t.boxes()[1];
        assert_eq!(ghost.kind, ToolBoxKind::Ghost);
        assert_eq!((ghost.min, ghost.size), ([10, 1, 9], [1, 2, 3]));
        step(
            &mut t,
            &mut w,
            ToolAction::Paste,
            &ctx(Some([10, 0, 10]), 0),
        );
        // Turned clockwise: +X became +Z, and the ladder faces south.
        for z in 9..12 {
            assert_eq!(w.get_voxel(10, 1, z), stone, "z {z}");
        }
        assert_eq!(
            dwell_blocks::info(w.get_voxel(10, 2, 9)).name,
            "dwell:ladder[facing=south,flooded=false]"
        );
        // Mirrored and unturned elsewhere: the L runs towards −X and the ladder faces west.
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        t.act(ToolAction::Mirror, &ctx(None, 0), &mut w);
        step(&mut t, &mut w, ToolAction::Paste, &ctx(Some([20, 0, 0]), 0));
        assert_eq!(
            dwell_blocks::info(w.get_voxel(21, 2, 0)).name,
            "dwell:ladder[facing=west,flooded=false]"
        );
        assert_eq!(w.get_voxel(19, 1, 0), stone);
        t.act(ToolAction::PasteMode, &ctx(None, 0), &mut w);
        assert!(!t.pasting());
    }

    #[test]
    fn refuses_what_the_server_would() {
        let stone = require("dwell:stone");
        let mut w = world();
        let mut t = RegionTools::new(); // an older server: no region edits
        t.act(ToolAction::CornerA, &ctx(Some([0, 0, 0]), 0), &mut w);
        assert_eq!(t.act(ToolAction::Fill, &ctx(None, stone), &mut w), None);
        assert!(
            t.status().contains("doesn't take region edits"),
            "{}",
            t.status()
        );
        t.enabled = true;
        // Too big.
        t.act(ToolAction::CornerB, &ctx(Some([32, 0, 0]), 0), &mut w);
        assert_eq!(t.act(ToolAction::Fill, &ctx(None, stone), &mut w), None);
        assert!(t.status().contains("at most 32"), "{}", t.status());
        // Too far.
        t.act(ToolAction::CornerA, &ctx(Some([100, 0, 0]), 0), &mut w);
        assert_eq!(t.act(ToolAction::Fill, &ctx(None, stone), &mut w), None);
        assert!(t.status().contains("Too far"), "{}", t.status());
        // Too fast: the burst of full boxes, then a wait.
        t.act(ToolAction::CornerA, &ctx(Some([-16, 0, -16]), 0), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([15, 31, 15]), 0), &mut w);
        let burst = region_edit::BURST_CELLS / region_edit::MAX_SIDE.pow(3);
        for _ in 0..burst {
            assert!(t.act(ToolAction::Clear, &ctx(None, 0), &mut w).is_some());
        }
        assert_eq!(t.act(ToolAction::Clear, &ctx(None, 0), &mut w), None);
        assert!(t.status().contains("Too fast"));
        let later = ToolContext {
            now_ms: 500.0,
            ..ctx(None, 0)
        };
        assert!(t.act(ToolAction::Clear, &later, &mut w).is_some());
        // Outside the build sphere (ADR 0025): over its top, eye beside it.
        let top = dwell_world::BUILD_VOLUME.y_range().1 as i32;
        let near_top = ToolContext {
            eye: Some([0.5, f64::from(top) - 1.0, 0.5]),
            now_ms: 10_000.0,
            ..ctx(None, stone)
        };
        t.act(ToolAction::CornerA, &ctx(Some([0, top - 3, 0]), 0), &mut w);
        t.act(ToolAction::CornerB, &ctx(Some([1, top, 1]), 0), &mut w);
        assert_eq!(t.act(ToolAction::Fill, &near_top, &mut w), None);
        assert!(t.status().contains("build volume"), "{}", t.status());
        // Not loaded: a streamed world without the chunks.
        let mut streamed = VoxelWorld::streamed();
        assert_eq!(t.act(ToolAction::Copy, &ctx(None, 0), &mut streamed), None);
        assert!(t.status().contains("Not loaded"));
    }

    /// Copy an L with a ladder on `from`'s face, turn it, and paste it on `to`'s ground: the
    /// world near the paste, face-local.
    fn copy_paste_local(from: dwell_world::FaceFrame, to: dwell_world::FaceFrame) -> Vec<StateId> {
        let stone = require("dwell:stone");
        let east = parse_state("dwell:ladder[facing=east]").expect("ladder");
        let mut w = world();
        let put = |w: &mut VoxelWorld, f: dwell_world::FaceFrame, c: [i32; 3], m: StateId| {
            let [x, y, z] = f.cell(c);
            w.set_voxel(x, y, z, f.state(m));
        };
        for x in 0..3 {
            put(&mut w, from, [x, 0, 0], stone);
        }
        put(&mut w, from, [0, 1, 0], east);
        put(&mut w, from, [2, 2, 0], stone);
        let at = |f: dwell_world::FaceFrame, target: [i32; 3]| ToolContext {
            target: Some(f.cell(target)),
            target_state: 0,
            placement: Some(f.cell([target[0], target[1] + 1, target[2]])),
            material: 0,
            eye: Some([0.5, 1.6, 0.5]),
            now_ms: 0.0,
        };
        let mut t = tools();
        t.act(ToolAction::CornerA, &at(from, [0, 0, 0]), &mut w);
        t.act(ToolAction::CornerB, &at(from, [2, 2, 0]), &mut w);
        t.act(ToolAction::Copy, &ctx(None, 0), &mut w);
        t.act(ToolAction::PasteMode, &ctx(None, 0), &mut w);
        t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        // The ghost stands on the aimed-at ground, toward the face's up.
        t.aim(Some(to.cell([10, 1, 10])));
        let ghost = t.boxes()[1];
        let (lo, hi) = (
            to.cell(ghost.min),
            to.cell(std::array::from_fn(|i| ghost.min[i] + ghost.size[i] - 1)),
        );
        assert_eq!(
            lo[1].min(hi[1]),
            1,
            "the ghost stands on the ground: {:?}",
            to.face
        );
        step(&mut t, &mut w, ToolAction::Paste, &at(to, [10, 0, 10]));
        let mut out = Vec::new();
        for y in 0..5 {
            for z in 7..14 {
                for x in 8..13 {
                    let [wx, wy, wz] = to.cell([x, y, z]);
                    out.push(to.state(w.get_voxel(wx, wy, wz)));
                }
            }
        }
        out
    }

    dwell_world::both_faces!(
        /// Face parity (BIFACIAL_WORLD.md §11): a copy keeps its cells face-local and a paste
        /// stands on the aimed-at ground toward the face's up, so a structure copied on either
        /// face pastes upright on either. Regression: on face B the paste box ran from the
        /// placement cell into the ground (+y), and a copy from face A pasted upside down.
        fn a_copy_pastes_upright_on_either_face(f: dwell_world::FaceFrame) {
            let a = dwell_world::FaceFrame::A;
            let expected = copy_paste_local(a, a);
            assert_eq!(
                copy_paste_local(f, f),
                expected,
                "{:?} → {:?}",
                f.face,
                f.face
            );
            assert_eq!(copy_paste_local(a, f), expected, "A → {:?}", f.face);
            assert_eq!(copy_paste_local(f, a), expected, "{:?} → A", f.face);
        }
    );

    /// Pastes a 40 × 3 × 2 library structure (wider than one edit) on face `f`, aimed at local
    /// (0, 0, 0), turned `turns` quarter turns; the states of the face-local box it lands in.
    fn library_paste_local(f: dwell_world::FaceFrame, turns: u8) -> Vec<StateId> {
        let (stone, sand, dirt) = (
            require("dwell:stone"),
            require("dwell:sand"),
            require("dwell:dirt"),
        );
        let size = [40, 3, 2];
        let mut cells = vec![stone; 240];
        let mut keep = vec![false; 240];
        let i = |x: i32, y: i32, z: i32| (x + 40 * (z + 2 * y)) as usize;
        cells[i(37, 2, 1)] = sand;
        keep[i(0, 0, 0)] = true;
        let mut w = world();
        // Under every cell the paste could keep: dirt, which a kept cell leaves.
        for z in -21..22 {
            for x in -21..22 {
                let [wx, wy, wz] = f.cell([x, 1, z]);
                w.set_voxel(wx, wy, wz, f.state(dirt));
            }
        }
        let mut t = tools();
        t.load_clipboard(size, cells, keep, "wall").expect("loads");
        for _ in 0..turns {
            t.act(ToolAction::Rotate, &ctx(None, 0), &mut w);
        }
        let at = ToolContext {
            target: Some(f.cell([0, 0, 0])),
            target_state: 0,
            placement: Some(f.cell([0, 1, 0])),
            material: 0,
            eye: Some([0.5, 1.6, 0.5]),
            now_ms: 0.0,
        };
        // Over one edit's side: nothing at once; its tiles queue as one undo step.
        assert_eq!(t.act(ToolAction::Paste, &at, &mut w), None, "{}", t.note());
        let sent = t.pump(1.0e9);
        assert_eq!(sent.len(), 2, "two tiles across");
        for m in sent {
            if let Message::RegionEdit(r) = &m {
                assert!(
                    r.size
                        .iter()
                        .all(|&s| usize::from(s) <= region_edit::MAX_SIDE)
                );
            }
            apply(&mut w, Some(m));
        }
        assert_eq!(t.undo_depth(), (1, 0));
        let mut out = Vec::new();
        for y in 1..4 {
            for z in -21..22 {
                for x in -21..22 {
                    let [wx, wy, wz] = f.cell([x, y, z]);
                    out.push(f.state(w.get_voxel(wx, wy, wz)));
                }
            }
        }
        out
    }

    dwell_world::both_faces!(
        /// A library structure larger than one edit (STRUCTURES.md §9) pastes as tiles of at most
        /// `MAX_SIDE`, upright on either face and turned with R: its blocks land, its kept cells
        /// keep what was there.
        fn a_library_structure_pastes_as_tiles_turned_on_either_face(f: dwell_world::FaceFrame) {
            let (stone, sand, dirt) = (
                require("dwell:stone"),
                require("dwell:sand"),
                require("dwell:dirt"),
            );
            let at = |out: &[StateId], [x, y, z]: [i32; 3]| {
                out[((x + 21) + 43 * ((z + 21) + 43 * (y - 1))) as usize]
            };
            // Unturned: the box stands on (0, 1, 0) with its bottom centre there: x −20..19, z −1..0.
            let out = library_paste_local(f, 0);
            assert_eq!(at(&out, [-20, 1, -1]), dirt, "kept cell");
            assert_eq!(at(&out, [-19, 1, -1]), stone);
            assert_eq!(at(&out, [17, 3, 0]), sand);
            assert_eq!(at(&out, [19, 2, 0]), stone, "the second tile");
            assert_eq!(at(&out, [20, 2, 0]), M.air, "outside the box");
            assert_eq!(
                out,
                library_paste_local(dwell_world::FaceFrame::A, 0),
                "face parity"
            );
            // One quarter turn: 2 × 3 × 40 at x −1..0, z −20..19; (37, 2, 1) → (0, 2, 37).
            let out = library_paste_local(f, 1);
            assert_eq!(at(&out, [-1, 3, 17]), sand);
            assert_eq!(at(&out, [0, 1, -20]), dirt, "kept cell turned");
            assert_eq!(at(&out, [17, 3, 0]), M.air);
            assert_eq!(
                out,
                library_paste_local(dwell_world::FaceFrame::A, 1),
                "face parity"
            );
        }
    );

    #[test]
    fn a_structure_loads_only_where_the_mode_places_structures() {
        let stone = require("dwell:stone");
        let mut t = RegionTools {
            structures: false,
            ..tools()
        };
        let e = t
            .load_clipboard([1, 1, 1], vec![stone], vec![], "post")
            .expect_err("adventure");
        assert!(e.contains("creative"), "{e}");
        assert!(!t.pasting());
    }

    #[test]
    fn the_librarys_data_limit_fits_the_clipboard() {
        // shared/structures/library.json: `tileSize` and `maxTiles` (the save and load limit).
        let text = include_str!("../../../shared/structures/library.json");
        let num = |key: &str| -> Vec<usize> {
            let at = text.find(&format!("\"{key}\"")).expect(key) + key.len() + 3;
            let rest = &text[at..];
            let end = rest.find(['\n', ',']).map_or(rest.len(), |e| {
                if rest.trim_start().starts_with('[') {
                    rest.find(']').expect("]") + 1
                } else {
                    e
                }
            });
            rest[..end]
                .trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace())
                .split(',')
                .map(|n| n.trim().parse().expect("a number"))
                .collect()
        };
        let tile = num("tileSize")[0];
        assert!(tile <= region_edit::MAX_SIDE, "a tile is one edit");
        let most: usize = num("maxTiles").iter().map(|&n| n * tile).product();
        assert!(most <= MAX_CLIPBOARD_CELLS, "{most} cells");
    }
}
