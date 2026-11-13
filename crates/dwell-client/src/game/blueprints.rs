//! The game's side of blueprints (WORK_QUEUE item 55, docs/BLUEPRINTS.md): placing and removing
//! one, the aimed one's ghost and panel each frame, and Place aimed into one building its ghost
//! cells only, from what the player has, one piece every `player.paceMs` while held — as one-cell
//! region fills, the drag building's path (ITEMS.md §5).

use dwell_blocks::StateId;
use dwell_protocol::blueprints::{Blueprint, BlueprintsMessage};
use dwell_protocol::{Message, players};
use dwell_world::blueprint::{cost_item, record_of};
use dwell_world::chunk_of;

use super::*;

impl<S: Scene> Game<S> {
    /// The server keeps the world's blueprints (`blueprints.world`).
    pub fn set_blueprints_synced(&mut self, on: bool) {
        self.blueprints_synced = on;
    }

    pub fn blueprints_synced(&self) -> bool {
        self.blueprints_synced
    }

    /// How many of `item` the player has (blocks unlimited, or no inventory: plenty).
    fn blueprint_count(&self, item: &str) -> u32 {
        match &self.loose.inventory {
            Some((true, inv)) => inv.count_of(item),
            _ => u32::MAX,
        }
    }

    /// A key for what the player has: the ghost's colours change with it.
    fn blueprint_have_key(&self) -> u64 {
        match &self.loose.inventory {
            Some((true, inv)) => {
                use std::hash::Hasher;
                let mut h = dwell_world::hash::FxHasher::default();
                h.write(&inv.to_bytes());
                h.finish()
            }
            _ => 0,
        }
    }

    /// Each frame: the blueprint aimed into within reach, its ghost, and a held Place's next piece.
    pub(super) fn blueprint_frame(&mut self, eye: Option<[f64; 3]>, dir: [f64; 3], now_ms: f64) {
        if self.blueprints.ghosts.is_empty() {
            self.blueprint_aimed = None;
            self.blueprints.held = false;
            self.blueprints
                .update_outline(None, &|_| None, &|_| false, 0);
            return;
        }
        let reach = f64::from(players::REACH_DISTANCE);
        // The ghost shows from farther than a place reaches, so a blueprint is seen to walk to.
        self.blueprint_aimed = eye.and_then(|e| self.blueprints.aimed(e, dir, 4.0 * reach));
        let key = self.blueprint_have_key();
        let counts: Vec<(&'static str, u32)> = self
            .blueprint_aimed
            .and_then(|i| self.blueprints.ghosts.get(i))
            .map(|g| {
                let mut items: Vec<&'static str> =
                    g.cells.iter().map(|c| cost_item(c.state)).collect();
                items.sort_unstable();
                items.dedup();
                items
                    .into_iter()
                    .map(|n| (n, self.blueprint_count(n)))
                    .collect()
            })
            .unwrap_or_default();
        let has = |s: StateId| {
            let n = cost_item(s);
            counts.iter().any(|&(i, c)| i == n && c > 0)
        };
        let aimed = self.blueprint_aimed;
        let store = &*self.streamer.store_mut();
        let get = |c: [i32; 3]| {
            store.find(chunk_of(c[0], c[1], c[2]))?;
            Some(store.get_voxel_if_loaded(c))
        };
        self.blueprints.update_outline(aimed, &get, &has, key);
        if self.blueprints.held
            && now_ms >= self.blueprints.next_ms
            && let Some(e) = eye
        {
            self.blueprint_build(e, dir, now_ms);
        }
    }

    /// Place pressed: aimed into a blueprint within reach, it builds there (held: on, at the pace)
    /// and the press is spent (Some: whether a piece went); else None, an ordinary place.
    pub(super) fn blueprint_press(&mut self, held: bool, now_ms: f64) -> Option<bool> {
        let eye = self.target_eye?;
        let dir = self.view_dir;
        let reach = f64::from(players::REACH_DISTANCE);
        self.blueprints.aimed(eye, dir, reach)?;
        self.blueprints.held = held;
        let placed = self.blueprint_build(eye, dir, now_ms);
        if !placed {
            self.sounds.push(sound_event(SoundKind::Deny));
        }
        Some(placed)
    }

    /// The next piece along the view: a ghost cell left, free, beside something solid, whose
    /// block the player has; sent as a one-cell fill of the ghost's own state.
    fn blueprint_build(&mut self, eye: [f64; 3], dir: [f64; 3], now_ms: f64) -> bool {
        let reach = f64::from(players::REACH_DISTANCE);
        let Some(i) = self.blueprints.aimed(eye, dir, reach) else {
            return false;
        };
        let pick = {
            let inv = match &self.loose.inventory {
                Some((true, inv)) => Some(inv.clone()),
                _ => None,
            };
            let has = |s: StateId| inv.as_ref().is_none_or(|v| v.count_of(cost_item(s)) > 0);
            let store = &*self.streamer.store_mut();
            let get = |c: [i32; 3]| {
                store.find(chunk_of(c[0], c[1], c[2]))?;
                Some(store.get_voxel_if_loaded(c))
            };
            self.blueprints.ghosts[i].pick(eye, dir, reach, &get, &has)
        };
        let Some(cell) = pick else {
            return false;
        };
        let fill = dwell_protocol::RegionEdit {
            op: dwell_protocol::RegionEditOp::Fill,
            min: cell.world,
            size: [1, 1, 1],
            material: cell.state,
            from: 0,
            cells: Vec::new(),
        };
        self.edit_sounds
            .local_place(cell.world, cell.state, now_ms, &mut self.sounds);
        self.bodies
            .swing_arms(dwell_characters::animator::Swing::Place);
        self.send_drag_fill(fill, 0);
        self.blueprints.next_ms = now_ms + self.blueprints.rules.player_pace_ms;
        true
    }

    /// The aimed blueprint's panel: (name, [(item, have, need)], rows folded, cells left).
    pub fn blueprint_panel(&mut self) -> Option<crate::blueprints::Panel> {
        let i = self.blueprint_aimed?;
        let names: Vec<&'static str> = self
            .blueprints
            .ghosts
            .get(i)?
            .cells
            .iter()
            .map(|c| cost_item(c.state))
            .collect();
        let counts: Vec<(&'static str, u32)> = names
            .iter()
            .map(|&n| (n, self.blueprint_count(n)))
            .collect();
        let have = |n: &str| counts.iter().find(|c| c.0 == n).map_or(0, |c| c.1);
        let store = &*self.streamer.store_mut();
        let get = |c: [i32; 3]| {
            store.find(chunk_of(c[0], c[1], c[2]))?;
            Some(store.get_voxel_if_loaded(c))
        };
        self.blueprints.panel(i, &get, &have)
    }

    /// A structure placed as a blueprint at the cell a place would fill now (the crosshair's),
    /// on the player's face, turned `turn` quarter turns; false with no aim, a structure that
    /// doesn't fit a record, or a server that doesn't keep blueprints.
    pub fn place_blueprint_structure(
        &mut self,
        s: &dwell_world::structure::Structure,
        turn: u8,
    ) -> bool {
        if !self.blueprints_synced {
            return false;
        }
        let Some((cell, _)) = self.interaction.placement() else {
            return false;
        };
        let f = self.current.face;
        let Some(b) = record_of(s, f, f.cell(cell), turn) else {
            return false;
        };
        self.send_blueprint(BlueprintsMessage::Place(b));
        true
    }

    /// A blueprint record placed as it is (a shell-built one).
    pub fn place_blueprint(&mut self, b: Blueprint) -> bool {
        if !self.blueprints_synced {
            return false;
        }
        self.send_blueprint(BlueprintsMessage::Place(b));
        true
    }

    /// Removes the blueprint the view aims into; false when none is.
    pub fn remove_aimed_blueprint(&mut self) -> bool {
        let Some(id) = self
            .blueprint_aimed
            .and_then(|i| self.blueprints.ghosts.get(i))
            .map(|g| g.id)
        else {
            return false;
        };
        self.send_blueprint(BlueprintsMessage::Remove(id));
        true
    }

    fn send_blueprint(&mut self, m: BlueprintsMessage) {
        self.outbox
            .borrow_mut()
            .push(Send::Control(Message::Blueprints(m)));
    }
}
