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
        let bodies: Vec<[f64; 3]> = self
            .sessions
            .values()
            .filter_map(|s| s.handle)
            .map(|h| self.players.position(&self.physics, h).to_array())
            .collect();
        // Run near the bodies and the theater cameras' viewpoints (THEATER_MODE.md §7).
        let players = self.presence_points(&bodies);
        let real_s = every as f32 / SIM_HZ as f32;
        let world = &mut self.world;
        let out = self
            .lava
            .tick(real_s, &players, |[x, y, z]| world.get_voxel(x, y, z));
        self.lava_steam.extend(out.steam);
        if out.writes.is_empty() {
            return;
        }
        let mut changes = TickChanges {
            lava: true,
            ..TickChanges::default()
        };
        for (c, m) in out.writes {
            changes.set(&mut self.world, c, m);
        }
        self.broadcast_changes(changes, VoxelModificationReason::Rebake);
    }

    /// A player standing in lava takes its `damagePerSecond`, and burns at `burnDamagePerSecond`
    /// for `burnSeconds` after leaving it (`dwell_lava::Burn`), through `damage` (so not in a mode
    /// without damage), cause `Burn` (sent as `Fall` to a client that never said it knows `Burn`).
    pub(super) fn burn(&mut self, id: SessionId) {
        let Some(h) = self.sessions[&id].handle else {
            return;
        };
        let p = self.players.position(&self.physics, h);
        let feet = [p.x, self.feet_y(h), p.z];
        let in_lava = self.lava.fluid_at(feet);
        self.burn_by(id, in_lava);
    }

    /// One tick of burning for a player in `in_lava` (or not): home's or another grid's.
    pub(super) fn burn_by(
        &mut self,
        id: SessionId,
        in_lava: Option<&'static dwell_lava::HotFluid>,
    ) {
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
            self.damage(id, due, DamageCause::Burn);
        }
    }

    /// The world's own vents (its worldgen config's `tectonics.lavaVents`, VOLCANOES.md §6.2): placed
    /// on each face when the world is created or first loaded with none (a saved world keeps its
    /// fields, and a newer build's lava record is left alone), each on its cone's summit as the
    /// terrain generates it. The build's ground lists none, so the default world gets no lava.
    pub(super) fn place_world_vents(&mut self) {
        if !self.lava.is_empty() || self.lava_unread || self.worldgen_text.is_empty() {
            return;
        }
        let sites = match dwell_worldgen::config::lava_vent_sites(&self.worldgen_text) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("lava: the world's vents: {e}");
                return;
            }
        };
        if sites.is_empty() {
            return;
        }
        let Some(terrain) = dwell_worldgen::bifacial::BifacialTerrain::for_generator(
            self.config.generator_version,
            self.config.world_seed,
        ) else {
            return;
        };
        for face in [dwell_world::Face::A, dwell_world::Face::B] {
            let f = FaceFrame::of(face);
            for (v, [x, z]) in &sites {
                if !v.faces[usize::from(f.is_b())] {
                    continue;
                }
                let column = [x.floor() as i32, z.floor() as i32];
                let Some(y) = terrain.local(face).surface_y_at(column[0], column[1]) else {
                    continue;
                };
                let spot = VentSpot {
                    fluid: v.fluid.clone(),
                    kind: match v.kind {
                        dwell_worldgen::tectonics::LavaVentKind::Lake => dwell_lava::VentKind::Lake,
                        dwell_worldgen::tectonics::LavaVentKind::River => {
                            dwell_lava::VentKind::River
                        }
                    },
                    rate_m3s: v.rate_m3s,
                    depth_m: v.depth_m,
                    radius: v.radius as usize,
                };
                if let Err(e) = self.place_lava_vent(f, column, dwell_world::row_of_m(y), &spot) {
                    eprintln!("lava: a vent of the world at {column:?}: {e}");
                }
            }
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

    /// The lava fields as a dev report (`__dwell.lava()`, docs/DEV_TOOLS.md): JSON
    /// `{"worldgen":…, "fields":[{"face":"a"|"b","centre":[x,z],"origin":[x,z],"w","h","tiles",
    /// "molten","maxDepthM","vents":[{"x","z","rateM3s"}]}]}`, `molten` the columns holding lava;
    /// `centre` its first vent (else its tiles' middle), `origin`, `w` and `h` the box its tiles
    /// cover now (a field grows with its flow, V2).
    pub fn lava_report(&self) -> String {
        let fields: Vec<String> = self
            .lava
            .fields
            .iter()
            .map(|p| {
                let f = &p.field;
                let t = dwell_lava::TILE as i32;
                let (lo, hi) = f.tiles().iter().fold(
                    ([i32::MAX; 2], [i32::MIN; 2]),
                    |(lo, hi), k| {
                        (
                            [lo[0].min(k[0] * t), lo[1].min(k[1] * t)],
                            [hi[0].max(k[0] * t + t), hi[1].max(k[1] * t + t)],
                        )
                    },
                );
                let (lo, hi) = if f.tiles().is_empty() { ([0; 2], [0; 2]) } else { (lo, hi) };
                let centre = f
                    .vents
                    .first()
                    .map_or([(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2], |v| [v.x, v.z]);
                let molten = f.depth.iter().filter(|d| **d > 0.0).count();
                let max = f.depth.iter().copied().fold(0.0_f32, f32::max);
                let vents: Vec<String> = f
                    .vents
                    .iter()
                    .map(|v| {
                        format!(
                            r#"{{"x":{},"z":{},"rateM3s":{}}}"#,
                            f.origin[0] + v.x,
                            f.origin[1] + v.z,
                            v.rate_m3s
                        )
                    })
                    .collect();
                format!(
                    r#"{{"face":"{}","centre":[{},{}],"origin":[{},{}],"w":{},"h":{},"tiles":{},"molten":{molten},"maxDepthM":{max},"vents":[{}]}}"#,
                    if p.face.is_b() { "b" } else { "a" },
                    f.origin[0] + centre[0],
                    f.origin[1] + centre[1],
                    f.origin[0] + lo[0],
                    f.origin[1] + lo[1],
                    hi[0] - lo[0],
                    hi[1] - lo[1],
                    f.tiles().len(),
                    vents.join(",")
                )
            })
            .collect();
        let worldgen = if self.worldgen_text.len() > 64 {
            "(text)"
        } else {
            &self.worldgen_text
        };
        format!(
            r#"{{"worldgen":{:?},"steamCells":{},"fields":[{}]}}"#,
            worldgen,
            self.lava_steam_cells,
            fields.join(",")
        )
    }

    /// Multiplies every vent's rate by `k` (a dev way to fill a lake fast: `?lavaRate=`,
    /// `__dwell.lavaRate(k)`); returns the vents changed.
    pub fn scale_lava_rate(&mut self, k: f32) -> usize {
        let mut n = 0;
        for p in &mut self.lava.fields {
            for v in &mut p.field.vents {
                v.rate_m3s *= k;
                n += 1;
            }
        }
        n
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

    /// The steam since the last call (lava meeting water; what `send_lava_steam` sends).
    pub fn take_lava_steam(&mut self) -> Vec<Steam> {
        std::mem::take(&mut self.lava_steam)
    }

    /// Each tick: the steam since the last as `Steam` to every client that said `SteamHello`
    /// (an older client never gets the message), at most `MAX_STEAM_PER_MESSAGE` cells.
    pub(super) fn send_lava_steam(&mut self) {
        if self.lava_steam.is_empty() {
            return;
        }
        self.lava_steam_cells += self.lava_steam.len() as u64;
        let cells: Vec<[i32; 3]> = self
            .take_lava_steam()
            .into_iter()
            .map(|s| s.cell)
            .take(dwell_protocol::MAX_STEAM_PER_MESSAGE)
            .collect();
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.steam_hello)
            .map(|(&id, _)| id)
            .collect();
        let m = Message::Steam(cells);
        for id in to {
            self.send_reliable(id, &m, Channel::World);
        }
    }
}
