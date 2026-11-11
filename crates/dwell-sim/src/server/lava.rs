//! Lava on the server (docs/proposals/VOLCANOES.md §6, V1 slice 2): the lava tick, its writes into
//! the world (one modification each lava tick, through the edit sync, so the water grid hears of
//! them too), a player's burn through the server's damage, and a dev way to place a vent.

use dwell_protocol::DamageCause;

use super::*;
use crate::lava::{Steam, VentSpot};

impl Server {
    /// Every `tickEvery` ticks, the lava's tick (`shared/fluids/lava.json`): each field run near a
    /// player, owed elsewhere; what it changed goes out as one modification.
    pub(super) fn flow_lava(&mut self) {
        let every = dwell_lava::DATA.tick_every.max(1);
        if self.lava.is_empty() || !self.tick.is_multiple_of(every) {
            return;
        }
        let players: Vec<[f64; 3]> = self
            .sessions
            .values()
            .filter_map(|s| s.handle)
            .map(|h| self.players.position(&self.physics, h).to_array())
            .collect();
        let real_s = every as f32 / SIM_HZ as f32;
        let world = &mut self.world;
        let out = self
            .lava
            .tick(real_s, &players, |[x, y, z]| world.get_voxel(x, y, z));
        self.lava_steam.extend(out.steam);
        if out.writes.is_empty() {
            return;
        }
        let mut changes = TickChanges::default();
        for (c, m) in out.writes {
            changes.set(&mut self.world, c, m);
        }
        self.broadcast_changes(changes, VoxelModificationReason::Rebake);
    }

    /// A player standing in lava takes its `damagePerSecond`, and burns at `burnDamagePerSecond`
    /// for `burnSeconds` after leaving it (`dwell_lava::Burn`), through `damage` (so not in a mode
    /// without damage). Sent as cause `Fall`: a `Burn` cause on the wire waits for the clients
    /// (an older client refuses an unknown cause).
    pub(super) fn burn(&mut self, id: SessionId) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let p = self.players.position(&self.physics, h);
        let feet = [p.x, self.feet_y(h), p.z];
        let in_lava = self.lava.fluid_at(feet);
        let s = self.sessions.get_mut(&id).expect("session");
        if in_lava.is_none() && !s.burn.burning() {
            return;
        }
        // Burning after leaving: the fluid the player was last in (the build's one hot fluid).
        let Some(fluid) = in_lava.or_else(|| dwell_lava::DATA.fluids.first()) else {
            return;
        };
        let due = s.burn.step(fluid, in_lava.is_some(), 1.0 / SIM_HZ as f32);
        if due > 0 {
            self.damage(id, due, DamageCause::Fall);
        }
    }

    /// Places a lava vent (a dev command, a test world): at world column `column` (x, z) on face
    /// `f`, its bed looked for round the face-local row `row`. Returns the field's index, or why
    /// not (an unknown fluid, no ground).
    pub fn place_lava_vent(
        &mut self,
        f: FaceFrame,
        column: [i32; 2],
        row: i32,
        spot: &VentSpot,
    ) -> Result<usize, String> {
        let world = &mut self.world;
        self.lava
            .place(&dwell_lava::DATA, f, column, row, spot, |[x, y, z]| {
                world.get_voxel(x, y, z)
            })
    }

    /// The world's lava fields.
    pub fn lava(&self) -> &crate::lava::Lava {
        &self.lava
    }

    /// Whether player `player_id` is burning (in lava or after it).
    pub fn burning(&self, player_id: u16) -> bool {
        self.session_of_player(player_id)
            .is_some_and(|id| self.sessions[&id].burn.burning())
    }

    /// The steam since the last call (lava meeting water; the clients' puffs, later).
    pub fn take_lava_steam(&mut self) -> Vec<Steam> {
        std::mem::take(&mut self.lava_steam)
    }
}
