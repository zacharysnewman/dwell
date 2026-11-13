//! The other grids on the server (docs/proposals/MOON_WORLD.md §7.2, M-7b): the moon's space held
//! beside the home world, its players carried across by `travel.rs`'s rules, its chunks streamed per
//! grid and its edited chunks saved per grid.
//!
//! - **Spaces.** One [`GridSlot`] per grid of `grids.json` with a generator of its own (the moon):
//!   its `GridSpace` (voxels from its generator in its local frame, its physics and players), and
//!   the chunks edited since the last save. The home grid stays the server's own world, physics and
//!   players, so everything built on them is unchanged.
//! - **Travel** ([`Server::travel_players`], before the inputs each tick): a home flier inside a
//!   grid's `captureM` is carried into its space (`travel::carry`, the world position and velocity
//!   kept), and an away player past `captureM × releaseFactor` back to home. While away the session's
//!   home handle is `None` and `away` names its slot and handle there; every player's far-flight
//!   speed is set from its world distance to the nearest grid's slab.
//! - **The wire** (`grid.frames`): an away player's snapshot carries its grid-local state and a
//!   `GridView` (its grid's index and frame); a client without the capability gets its world
//!   position instead and never a grid id. The other grids' chunks go only to a `grid.frames`
//!   client in that grid, as `ChunkData`/`ChunkUnload` ending with the grid's byte.
//! - **Edits** by an away player apply to its grid (the cell in the grid's own space); the chunk
//!   goes out whole to the grid's clients, and is saved in the world file's `grid_chunks` table.
//!
//! Not yet: the other systems (items, vitals, creatures, water, felling) for away players; the
//! player's grid in its saved record (a player who leaves on the moon is saved at its world
//! position and comes back at home's gravity).

use super::*;
use crate::travel::{
    GRID_CENTRE, GridSpace, Transfer, carry, grid_frame_at, grid_generator, slab_distance,
};
use dwell_look::celestial::Celestial;
use dwell_protocol::capabilities::Capabilities;
use dwell_protocol::capability_names::GRID_FRAMES;
use dwell_world::GridFrame;
use dwell_worldgen::grids::{GridDef, TravelRules};

type V3 = [f64; 3];

/// Chunks of another grid sent to one client a tick, at most.
const GRID_CHUNKS_PER_TICK: usize = 8;

/// Where a session's player is while on another grid: its slot and its handle in that space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Away {
    pub slot: usize,
    pub handle: PlayerHandle,
}

/// Another grid's space on the server.
pub struct GridSlot {
    /// Its index in `grids.json` (the wire's grid byte; the home grid is 0).
    pub index: u8,
    pub space: GridSpace,
    /// Chunks edited since the last save.
    dirty: HashSet<ChunkCoord>,
    /// Its lava fields (docs/proposals/SUN_WORLD.md §6): the sun's rivers kept flowing near its
    /// players. Not saved: their cells are in the grid's chunks, and the fields are placed again.
    pub lava: crate::lava::Lava,
    /// Each field's river head (face B, flow cell), in `lava.fields`' order.
    lava_heads: Vec<(bool, [i32; 2])>,
    /// Every river head placed (or tried) since the server started.
    heads_tried: HashSet<(bool, [i32; 2])>,
    /// The generator its river heads come from, and its `flow` rules (a `volcanic` grid with
    /// them; `None` places no source).
    flow: Option<(
        dwell_worldgen::volcanic::VolcanicTerrain,
        dwell_worldgen::volcanic::FlowRule,
    )>,
}

impl GridSlot {
    /// Puts a source at each river head (`flow`) within `nearM` of a player (grid-local points)
    /// not placed yet, while there is room under `maxFields`: a full grid drops its farthest field
    /// with no player near for it, or places no more.
    fn place_river_heads(&mut self, players: &[V3]) {
        let Some((terrain, fl)) = &self.flow else {
            return;
        };
        let rules = dwell_lava::LavaData {
            max_tiles: fl.max_tiles,
            near_radius_m: fl.near_m as f32,
            ..dwell_lava::DATA
        };
        for &p in players {
            let f = FaceFrame::at_height(p[1]);
            let l = f.point(p);
            for h in terrain.river_heads(f.face, l[0], l[2], fl.near_m) {
                let key = (f.is_b(), h.cell);
                if self.heads_tried.contains(&key) {
                    continue;
                }
                if self.lava.fields.len() >= fl.max_fields as usize {
                    let far = (0..self.lava.fields.len())
                        .filter(|&i| {
                            let q = &self.lava.fields[i];
                            !q.field.is_near(q.face, players)
                        })
                        .max_by(|&a, &b| {
                            let d = |i: usize| {
                                let o = self.lava.fields[i].field.origin;
                                players
                                    .iter()
                                    .map(|p| (p[0] - f64::from(o[0])).hypot(p[2] - f64::from(o[1])))
                                    .fold(f64::INFINITY, f64::min)
                            };
                            d(a).total_cmp(&d(b))
                        });
                    let Some(i) = far else {
                        return;
                    };
                    self.lava.remove(i);
                    let gone = self.lava_heads.remove(i);
                    self.heads_tried.remove(&gone);
                }
                self.heads_tried.insert(key);
                let spot = crate::lava::VentSpot {
                    fluid: fl.fluid.clone(),
                    kind: dwell_lava::VentKind::River,
                    rate_m3s: fl.rate_m3s as f32,
                    depth_m: 0.0,
                    radius: fl.radius as usize,
                };
                let world = &mut self.space.world;
                let column = [h.x.floor() as i32, h.z.floor() as i32];
                if self
                    .lava
                    .place(
                        &rules,
                        f,
                        column,
                        dwell_world::row_of_m(h.lava),
                        &spot,
                        |[x, y, z]| world.get_voxel(x, y, z),
                    )
                    .is_ok()
                {
                    self.lava_heads.push(key);
                }
            }
        }
    }
}

/// The grids beside the home world.
pub struct Grids {
    pub celestial: Celestial,
    pub rules: TravelRules,
    pub home: &'static GridDef,
    /// The home disc's radius (m).
    pub home_radius_m: f64,
    pub slots: Vec<GridSlot>,
    /// The moves between spaces since the last `Server::take_transfers` (player id, the move;
    /// `from`/`to` are grids.json indices).
    transfers: Vec<(u16, Transfer)>,
}

impl Grids {
    /// No other grid (until the server builds them, `Grids::new`).
    pub fn empty() -> Self {
        let all = dwell_worldgen::grids::grids();
        Self {
            celestial: Celestial::new(dwell_look::daynight::DAY_NIGHT, None),
            rules: dwell_worldgen::grids::travel(),
            home: &all[0],
            home_radius_m: 0.0,
            slots: Vec::new(),
            transfers: Vec::new(),
        }
    }

    /// Every grid with a generator of its own, its saved chunks put back.
    pub fn new(
        world_seed: u64,
        home_radius_m: f64,
        celestial: Celestial,
        db: Option<&dwell_storage::WorldDb>,
    ) -> Self {
        let all = dwell_worldgen::grids::grids();
        let mut slots = Vec::new();
        for (i, def) in all.iter().enumerate().skip(1) {
            let (Some((generator, air)), Some(radius)) =
                (grid_generator(def, world_seed), def.radius_m)
            else {
                continue;
            };
            let mut space = GridSpace::new(def, radius, generator, Some(air));
            for saved in db.map(|d| d.grid_chunks(&def.name)).unwrap_or_default() {
                if let Some(chunk) = Chunk::from_voxels(&saved.voxels, saved.revision) {
                    space.world.put(saved.coord, chunk);
                }
            }
            let flow = dwell_worldgen::volcanic::VolcanicTerrain::new(def, world_seed)
                .and_then(|t| t.rules().flow.clone().map(|f| (t, f)));
            slots.push(GridSlot {
                index: i as u8,
                space,
                dirty: HashSet::default(),
                lava: Default::default(),
                lava_heads: Vec::new(),
                heads_tried: HashSet::default(),
                flow,
            });
        }
        Self {
            celestial,
            rules: dwell_worldgen::grids::travel(),
            home: &all[0],
            home_radius_m,
            slots,
            transfers: Vec::new(),
        }
    }

    /// The travel rules without the spaces, as the client's predictor runs them (MOON_WORLD.md
    /// §7.3): a client builds the same from the world's seed, size and settings (`TravelMap::new`).
    pub fn travel_map(&self) -> crate::travel::TravelMap {
        crate::travel::TravelMap {
            celestial: self.celestial,
            rules: self.rules,
            home: self.home,
            home_radius_m: self.home_radius_m,
            grids: self
                .slots
                .iter()
                .map(|s| (s.index, s.space.def, s.space.radius_m))
                .collect(),
        }
    }

    /// A slot's frame at `world_s`.
    pub fn frame(&self, slot: usize, world_s: f64) -> GridFrame {
        grid_frame_at(&self.celestial, self.slots[slot].space.def, world_s)
    }

    /// The far-flight speed at world point `p` (0 near every grid's slab) for a client with the
    /// capabilities `caps`: the grids it can't be captured into (`GridDef::open_to`) don't count, so
    /// it flies on past them as its own build predicts.
    pub fn far_speed(&self, p: V3, world_s: f64, caps: &Capabilities) -> f64 {
        let home = slab_distance(self.home_radius_m, self.home.surface_m, p);
        let d = (0..self.slots.len())
            .filter(|&i| self.slots[i].space.def.open_to(|c| caps.has(c)))
            .map(|i| {
                let s = &self.slots[i].space;
                slab_distance(
                    s.radius_m,
                    s.def.surface_m,
                    self.frame(i, world_s).to_grid(p),
                )
            })
            .fold(home, f64::min);
        self.rules.far_speed(d)
    }

    /// The slot whose capture sphere holds world point `p`, among those a client with the
    /// capabilities `caps` may be captured into (a grid's `capability`, docs/proposals/SUN_WORLD.md §5).
    fn capturing(&self, p: V3, world_s: f64, caps: &Capabilities) -> Option<usize> {
        (0..self.slots.len()).find(|&i| {
            if !self.slots[i].space.def.open_to(|c| caps.has(c)) {
                return false;
            }
            let local = self.frame(i, world_s).to_grid(p);
            let d = sub(local, GRID_CENTRE);
            self.slots[i]
                .space
                .def
                .capture_m
                .is_some_and(|c| len(d) < c)
        })
    }

    /// One tick of every space that holds a player.
    pub fn step(&mut self, dt: f64) {
        for s in &mut self.slots {
            if !s.space.players.handles().is_empty() {
                s.space.step(dt);
            }
        }
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn len(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

impl Server {
    /// The world clock (s) the grids' frames are placed by.
    pub(super) fn world_s(&self) -> f64 {
        f64::from(self.tick) / f64::from(SIM_HZ)
    }

    /// The grids beside the home world.
    pub fn grids(&self) -> &Grids {
        &self.grids
    }

    /// A player's grid (0 home) and its grid-local position (capsule centre) and velocity.
    pub fn grid_state_of(&self, player_id: u16) -> Option<(u8, V3, V3)> {
        let id = self.session_of_player(player_id)?;
        let s = &self.sessions[&id];
        if let Some(a) = s.away {
            let sp = &self.grids.slots[a.slot].space;
            let p = sp.players.position(&sp.physics, a.handle).to_array();
            let v = sp
                .players
                .velocity(&sp.physics, a.handle)
                .as_dvec3()
                .to_array();
            return Some((self.grids.slots[a.slot].index, p, v));
        }
        let h = s.handle?;
        let p = self.players.position(&self.physics, h).to_array();
        let v = self
            .players
            .velocity(&self.physics, h)
            .as_dvec3()
            .to_array();
        Some((0, p, v))
    }

    /// A player's world position (capsule centre) and velocity, wherever it is.
    pub fn world_state_of(&self, player_id: u16) -> Option<(V3, V3)> {
        let id = self.session_of_player(player_id)?;
        let (grid, p, v) = self.grid_state_of(player_id)?;
        Some(match self.sessions[&id].away {
            Some(a) if grid != 0 => self.grids.frame(a.slot, self.world_s()).release(p, v),
            _ => (p, v),
        })
    }

    /// Before the inputs: captures home fliers inside a grid's `captureM`, releases away players
    /// past `captureM × releaseFactor`, and sets every player's far-flight speed.
    pub(super) fn travel_players(&mut self) {
        if self.grids.slots.is_empty() {
            return;
        }
        self.spawn_on_grids();
        let world_s = self.world_s();
        let ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.joined())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let s = &self.sessions[&id];
            match (s.handle, s.away) {
                (Some(h), None) => {
                    if !self.players.controller(h).fly.flying {
                        continue;
                    }
                    let p = self.players.position(&self.physics, h).to_array();
                    let v = self
                        .players
                        .velocity(&self.physics, h)
                        .as_dvec3()
                        .to_array();
                    if let Some(slot) = self.grids.capturing(p, world_s, &s.capabilities) {
                        let (lp, lv) = self.grids.frame(slot, world_s).capture(p, v);
                        self.carry_to_grid(id, h, slot, lp, lv);
                        let pid = self.sessions[&id].player_id;
                        let (after, v_after) = self.world_state_of(pid).expect("carried");
                        let to = self.grids.slots[slot].index.into();
                        self.grids.transfers.push((
                            pid,
                            Transfer {
                                from: 0,
                                to,
                                world_before: p,
                                world_after: after,
                                velocity_before: v,
                                velocity_after: v_after,
                            },
                        ));
                    }
                }
                (None, Some(a)) => {
                    let sp = &self.grids.slots[a.slot].space;
                    let p = sp.players.position(&sp.physics, a.handle).to_array();
                    let release =
                        sp.def.capture_m.unwrap_or(f64::INFINITY) * self.grids.rules.release_factor;
                    if len(sub(p, GRID_CENTRE)) > release {
                        self.bring_home(id);
                    }
                }
                _ => {}
            }
        }
        // Far flight, from each player's world position.
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        for id in ids {
            let s = &self.sessions[&id];
            let pid = s.player_id;
            let (Some((p, _)), true) = (self.world_state_of(pid), s.joined()) else {
                continue;
            };
            let far = self.grids.far_speed(p, world_s, &s.capabilities) as f32;
            let s = &self.sessions[&id];
            match (s.handle, s.away) {
                (Some(h), _) => self.players.set_far_fly_speed(&mut self.physics, h, far),
                (None, Some(a)) => {
                    let sp = &mut self.grids.slots[a.slot].space;
                    sp.players.set_far_fly_speed(&mut sp.physics, a.handle, far);
                }
                _ => {}
            }
        }
    }

    /// The world's spawn grid (`ServerConfig::spawn_grid`, MOON_WORLD.md §8): its slot and face, and
    /// the stand point there (grid-local capsule centre), if the build has that grid.
    pub fn grid_spawn_point(&self) -> Option<(usize, FaceFrame, V3)> {
        let key = self.config.spawn_grid.as_deref()?;
        let gs = dwell_worldgen::grids::grid_spawn(key)?;
        let slot = self
            .grids
            .slots
            .iter()
            .position(|s| usize::from(s.index) == gs.index)?;
        let feet = dwell_worldgen::grids::stand_point(gs.def, self.config.world_seed, gs.face)?;
        let f = FaceFrame::of(gs.face);
        let half = f64::from(self.config.player_config.half_height(false));
        Some((slot, f, f.point([feet[0], feet[1] + half + 0.05, feet[2]])))
    }

    /// Players just spawned at home in a world whose spawn is on another grid go there once their
    /// client says it can be on it (`grid.frames` and the grid's `capability`); a client that
    /// hasn't within `GRID_SPAWN_GRACE_TICKS` (an older build) stays at home's spawn.
    fn spawn_on_grids(&mut self) {
        let due: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.grid_spawn_due.is_some() && s.handle.is_some())
            .map(|(id, _)| *id)
            .collect();
        if due.is_empty() {
            return;
        }
        let point = self.grid_spawn_point();
        for id in due {
            let s = &self.sessions[&id];
            let open = point.is_some_and(|(slot, ..)| {
                s.capabilities.has(GRID_FRAMES)
                    && self.grids.slots[slot]
                        .space
                        .def
                        .open_to(|c| s.capabilities.has(c))
            });
            let late = s.grid_spawn_due.is_some_and(|t| self.tick >= t);
            let (Some((slot, _, at)), true) = (point, open) else {
                if late || point.is_none() {
                    self.sessions.get_mut(&id).expect("session").grid_spawn_due = None;
                }
                continue;
            };
            let h = s.handle.expect("filtered");
            self.sessions.get_mut(&id).expect("session").grid_spawn_due = None;
            self.carry_to_grid(id, h, slot, at, [0.0; 3]);
        }
    }

    /// Carries home player `h` of session `id` into slot `slot`'s space at grid-local `lp`, `lv`.
    fn carry_to_grid(
        &mut self,
        id: SessionId,
        h: PlayerHandle,
        slot: usize,
        lp: V3,
        lv: V3,
    ) -> PlayerHandle {
        let space = &mut self.grids.slots[slot].space;
        let scale = space.def.gravity_scale as f32;
        let nh = carry(
            (&mut self.players, &mut self.physics),
            h,
            (&mut space.players, &mut space.physics),
            scale,
            lp,
            lv,
        );
        let s = self.sessions.get_mut(&id).expect("session");
        s.handle = None;
        s.away = Some(Away { slot, handle: nh });
        s.grid_streamed.clear();
        if s.capabilities.has(GRID_FRAMES) {
            s.forget_home_stream();
        }
        nh
    }

    /// Carries an away player back into the home space at its world position and velocity, and
    /// unloads its grid's chunks on its client.
    pub(super) fn bring_home(&mut self, id: SessionId) {
        let Some(a) = self.sessions.get(&id).and_then(|s| s.away) else {
            return;
        };
        let world_s = self.world_s();
        let frame = self.grids.frame(a.slot, world_s);
        let space = &mut self.grids.slots[a.slot].space;
        let p = space.players.position(&space.physics, a.handle).to_array();
        let v = space
            .players
            .velocity(&space.physics, a.handle)
            .as_dvec3()
            .to_array();
        let (wp, wv) = frame.release(p, v);
        let h = carry(
            (&mut space.players, &mut space.physics),
            a.handle,
            (&mut self.players, &mut self.physics),
            1.0,
            wp,
            wv,
        );
        let index = self.grids.slots[a.slot].index;
        let s = self.sessions.get_mut(&id).expect("session");
        s.handle = Some(h);
        s.away = None;
        let pid = s.player_id;
        let after = self.players.position(&self.physics, h).to_array();
        let v_after = self
            .players
            .velocity(&self.physics, h)
            .as_dvec3()
            .to_array();
        self.grids.transfers.push((
            pid,
            Transfer {
                from: index.into(),
                to: 0,
                world_before: wp,
                world_after: after,
                velocity_before: wv,
                velocity_after: v_after,
            },
        ));
        let s = self.sessions.get_mut(&id).expect("session");
        let mut gone: Vec<ChunkCoord> = s.grid_streamed.drain().collect();
        gone.sort_by_key(|c| (c.x, c.y, c.z));
        if s.capabilities.has(GRID_FRAMES) && !gone.is_empty() {
            for part in gone.chunks(0xFFFF) {
                let m = Message::ChunkUnload(ChunkUnload {
                    coords: part.iter().map(|c| c.to_array()).collect(),
                    grid: index,
                });
                self.send_reliable(id, &m, Channel::World);
            }
        }
        // The home view streams again from where the player is now.
        let s = self.sessions.get_mut(&id).expect("session");
        s.stream_center = None;
    }

    /// An away player's local state for its snapshot: its grid-local state and the `GridView` for
    /// a `grid.frames` client, or its world position for an older one.
    pub(super) fn away_local(&self, s: &Session) -> Option<(LocalPlayerState, Option<GridView>)> {
        let a = s.away?;
        let slot = &self.grids.slots[a.slot];
        let sp = &slot.space;
        let c = sp.players.controller(a.handle);
        let mut l = LocalPlayerState {
            health: s.health.clamp(0, 255) as u8,
            input_buffer: s.inputs.len().min(255) as u8,
            last_knockback_seq: s.last_knockback_seq,
            ..LocalPlayerState::default()
        };
        let p = sp.players.position(&sp.physics, a.handle).to_array();
        let v = sp.players.velocity(&sp.physics, a.handle);
        l.flags = player_flags_of(c, false);
        l.state = PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle);
        // Grounds on another grid are its own (no player or piece there is the home server's).
        l.controller = to_net(c, |_| 0);
        if s.capabilities.has(GRID_FRAMES) {
            l.position = p;
            l.velocity = v.to_array();
            let f = self.grids.frame(a.slot, self.world_s());
            let view = GridView {
                grid: slot.index,
                origin: f.origin,
                rotation: f.rotation,
            };
            Some((l, Some(view)))
        } else {
            let (wp, wv) = self
                .grids
                .frame(a.slot, self.world_s())
                .release(p, v.as_dvec3().to_array());
            l.position = wp;
            l.velocity = wv.map(|x| x as f32);
            Some((l, None))
        }
    }

    /// An away player as the others see it: at its world position.
    pub(super) fn away_remote(&self, s: &Session, r: &mut RemotePlayerState) {
        let Some(a) = s.away else { return };
        let sp = &self.grids.slots[a.slot].space;
        let c = sp.players.controller(a.handle);
        let feet = sp.players.feet(&sp.physics, a.handle);
        let p = sp.players.position(&sp.physics, a.handle).to_array();
        let v = sp
            .players
            .velocity(&sp.physics, a.handle)
            .as_dvec3()
            .to_array();
        let (wp, wv) = self
            .grids
            .frame(a.slot, self.world_s())
            .release([p[0], feet, p[2]], v);
        r.position = wp;
        r.velocity = wv.map(|x| x as f32);
        r.yaw = quantize_yaw(c.input.look_yaw);
        r.pitch = quantize_pitch(c.input.look_pitch);
        r.state = PlayerState::from_u8(c.state as u8).unwrap_or(PlayerState::Idle);
        r.flags = player_flags_of(c, false);
    }

    /// Streams an away `grid.frames` client its grid's chunks round its grid-local position,
    /// nearest first, and unloads those past the view.
    pub(super) fn stream_grid_chunks(&mut self, id: SessionId) {
        let s = &self.sessions[&id];
        let Some(a) = s.away else { return };
        if !s.capabilities.has(GRID_FRAMES) || s.chunk_mode == ChunkMode::AwaitingCheck {
            return;
        }
        // Always the voxels (or Air): the client's workers generate home's terrain only (M-7c).
        let generated = false;
        let index = self.grids.slots[a.slot].index;
        let sp = &self.grids.slots[a.slot].space;
        let p = sp.players.position(&sp.physics, a.handle);
        let center = chunk_at(p.x, p.y, p.z);
        let view = self.config.view_radius_chunks;
        let margin = wc::UNLOAD_MARGIN_CHUNKS as i32;
        let mut gone: Vec<ChunkCoord> = s
            .grid_streamed
            .iter()
            .copied()
            .filter(|c| !within_radius(*c, center, view + margin))
            .collect();
        gone.sort_by_key(|c| (c.x, c.y, c.z));
        let s = self.sessions.get_mut(&id).expect("session");
        for c in &gone {
            s.grid_streamed.remove(c);
        }
        if !gone.is_empty() {
            let m = Message::ChunkUnload(ChunkUnload {
                coords: gone.iter().map(|c| c.to_array()).collect(),
                grid: index,
            });
            self.send_reliable(id, &m, Channel::World);
        }
        let mut sent = 0;
        for i in 0..self.view_offsets.len() {
            if sent >= GRID_CHUNKS_PER_TICK {
                break;
            }
            let c = add(center, self.view_offsets[i]);
            if self.sessions[&id].grid_streamed.contains(&c) {
                continue;
            }
            let m = self.grid_chunk_message(a.slot, c, generated);
            let s = self.sessions.get_mut(&id).expect("session");
            s.grid_streamed.insert(c);
            Self::count_sent(&mut s.stream_stats, m.form);
            self.send_reliable(id, &Message::ChunkData(m), Channel::World);
            sent += 1;
        }
    }

    /// Chunk `c` of a slot's grid as a client gets it: Air off the slab, Generated when unedited
    /// (for a client that generates), else its voxels.
    fn grid_chunk_message(&mut self, slot: usize, c: ChunkCoord, generated: bool) -> ChunkData {
        let index = self.grids.slots[slot].index;
        let world = &mut self.grids.slots[slot].space.world;
        let edited = world.find(c).is_some_and(|ch| ch.revision() > 0);
        let mut m = ChunkData {
            form: ChunkForm::Explicit,
            coord: c.to_array(),
            revision: 0,
            voxels: Vec::new(),
            grid: index,
        };
        if !edited && world.is_air_chunk(c) {
            m.form = ChunkForm::Air;
        } else if !edited && generated {
            m.form = ChunkForm::Generated;
        } else {
            let chunk = world.get_or_create(c);
            m.revision = chunk.revision();
            m.voxels = chunk.voxels().to_vec();
        }
        m
    }

    /// A block edit by an away player, applied to its grid at once (the cell in the grid's own
    /// space): a break clears the cell, a place sets the cell against `face`. The chunk goes out
    /// whole to that grid's `grid.frames` clients that have it.
    pub(super) fn apply_grid_edit(&mut self, id: SessionId, edit: &BlockEditRequest) -> bool {
        let Some(a) = self.sessions[&id].away else {
            return false;
        };
        const NORMALS: [[i32; 3]; 6] = [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ];
        let (cell, state) = match edit.action {
            BlockEditAction::Break => (edit.cell, dwell_blocks::AIR),
            BlockEditAction::Place => {
                let Some(n) = NORMALS.get(usize::from(edit.face)) else {
                    return false;
                };
                if dwell_blocks::try_info(edit.material).is_none() {
                    return false;
                }
                let c = edit.cell;
                ([c[0] + n[0], c[1] + n[1], c[2] + n[2]], edit.material)
            }
        };
        let slot = &mut self.grids.slots[a.slot];
        let chunk = chunk_of(cell[0], cell[1], cell[2]);
        let world = &mut slot.space.world;
        if world.get_voxel(cell[0], cell[1], cell[2]) == state {
            return false;
        }
        world.set_voxel(cell[0], cell[1], cell[2], state);
        slot.dirty.insert(chunk);
        // A field over the cell reads its bed again (a dug channel, a dam).
        slot.lava.touch(cell);
        let generated = false;
        let m = self.grid_chunk_message(a.slot, chunk, generated);
        let to: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                s.away.is_some_and(|o| o.slot == a.slot) && s.grid_streamed.contains(&chunk)
            })
            .map(|(id, _)| *id)
            .collect();
        for to in to {
            self.send_reliable(to, &Message::ChunkData(m.clone()), Channel::World);
        }
        true
    }

    /// Every lava tick, each grid's lava fields (docs/proposals/SUN_WORLD.md §6): sources placed at
    /// the river heads near its players, the fields near them run (no other), and the cells they
    /// changed written into the grid's chunks, each changed chunk sent whole to that grid's clients
    /// that hold it (as an away player's edit is).
    pub(super) fn flow_grid_lava(&mut self) {
        let every = dwell_lava::DATA.tick_every.max(1);
        if self.grids.slots.is_empty() || !self.tick.is_multiple_of(every) {
            return;
        }
        let real_s = every as f32 / SIM_HZ as f32;
        for slot in 0..self.grids.slots.len() {
            let players: Vec<V3> = {
                let sp = &self.grids.slots[slot].space;
                self.sessions
                    .values()
                    .filter_map(|s| s.away.filter(|a| a.slot == slot))
                    .map(|a| sp.players.position(&sp.physics, a.handle).to_array())
                    .collect()
            };
            if players.is_empty() {
                continue;
            }
            let g = &mut self.grids.slots[slot];
            g.place_river_heads(&players);
            if g.lava.is_empty() {
                continue;
            }
            let world = &mut g.space.world;
            // Steam needs water, which no other grid has yet: dropped.
            let out = g.lava.tick_with(real_s, &players, false, |[x, y, z]| {
                world.get_voxel(x, y, z)
            });
            if out.writes.is_empty() {
                continue;
            }
            let mut chunks = HashSet::default();
            for ([x, y, z], m) in out.writes {
                world.set_voxel(x, y, z, m);
                chunks.insert(chunk_of(x, y, z));
            }
            g.dirty.extend(chunks.iter().copied());
            let mut chunks: Vec<ChunkCoord> = chunks.into_iter().collect();
            chunks.sort_by_key(|c| (c.x, c.y, c.z));
            for c in chunks {
                let m = self.grid_chunk_message(slot, c, false);
                let to: Vec<SessionId> = self
                    .sessions
                    .iter()
                    .filter(|(_, s)| {
                        s.away.is_some_and(|o| o.slot == slot) && s.grid_streamed.contains(&c)
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for to in to {
                    self.send_reliable(to, &Message::ChunkData(m.clone()), Channel::World);
                }
            }
        }
    }

    /// The hot fluid an away player's feet stand in on its grid: a field's lava, or any cell of a
    /// hot fluid's block at the feet (the sun's generated oceans and rivers), by lava.json's names.
    pub(super) fn away_hot_fluid(&mut self, a: Away) -> Option<&'static dwell_lava::HotFluid> {
        let g = &mut self.grids.slots[a.slot];
        let sp = &mut g.space;
        let p = sp.players.position(&sp.physics, a.handle).to_array();
        let feet = [p[0], sp.players.feet(&sp.physics, a.handle), p[2]];
        if let Some(h) = g.lava.fluid_at(feet) {
            return Some(h);
        }
        // Just inside the feet (face-local up a little: they rest on a cell's top).
        let f = FaceFrame::at_height(feet[1]);
        let l = f.point(feet);
        let cell = f.cell([
            l[0].floor() as i32,
            dwell_world::row_of_m(l[1] + 0.1),
            l[2].floor() as i32,
        ]);
        crate::lava::hot_fluid_of(sp.world.get_voxel(cell[0], cell[1], cell[2]))
    }

    /// Takes an away player out of its grid's space (its death there) and unloads that grid's
    /// chunks on its client: the feet's world position, where it died.
    pub(super) fn remove_away(&mut self, id: SessionId) -> Option<V3> {
        let a = self.sessions.get(&id).and_then(|s| s.away)?;
        let frame = self.grids.frame(a.slot, self.world_s());
        let sp = &mut self.grids.slots[a.slot].space;
        let p = sp.players.position(&sp.physics, a.handle).to_array();
        let feet = [p[0], sp.players.feet(&sp.physics, a.handle), p[2]];
        sp.players.despawn(&mut sp.physics, a.handle);
        let (death, _) = frame.release(feet, [0.0; 3]);
        let index = self.grids.slots[a.slot].index;
        let s = self.sessions.get_mut(&id).expect("session");
        s.away = None;
        let mut gone: Vec<ChunkCoord> = s.grid_streamed.drain().collect();
        gone.sort_by_key(|c| (c.x, c.y, c.z));
        if s.capabilities.has(GRID_FRAMES) && !gone.is_empty() {
            for part in gone.chunks(0xFFFF) {
                let m = Message::ChunkUnload(ChunkUnload {
                    coords: part.iter().map(|c| c.to_array()).collect(),
                    grid: index,
                });
                self.send_reliable(id, &m, Channel::World);
            }
        }
        let s = self.sessions.get_mut(&id).expect("session");
        s.stream_center = None;
        Some(death)
    }

    /// The grids' chunks edited since the last save, for the batch.
    pub(super) fn grid_chunks_to_save(&mut self) -> Vec<(String, dwell_storage::SavedChunk)> {
        let mut out = Vec::new();
        for slot in &mut self.grids.slots {
            let mut dirty: Vec<ChunkCoord> = slot.dirty.drain().collect();
            dirty.sort_by_key(|c| (c.x, c.y, c.z));
            for c in dirty {
                if let Some(chunk) = slot.space.world.find(c) {
                    out.push((
                        slot.space.def.name.clone(),
                        dwell_storage::SavedChunk {
                            coord: c,
                            revision: chunk.revision(),
                            voxels: chunk.voxels().to_vec(),
                        },
                    ));
                }
            }
        }
        out
    }

    /// The moves between the grids' spaces since the last call (player id, the move).
    pub fn take_transfers(&mut self) -> Vec<(u16, Transfer)> {
        std::mem::take(&mut self.grids.transfers)
    }

    /// Puts a player that is on grid `index` (0 home) at grid-local `p` (capsule centre) with
    /// grid-local velocity `v` (tests, tools).
    pub fn place_on_grid(&mut self, player_id: u16, p: V3, v: V3) -> bool {
        let Some(id) = self.session_of_player(player_id) else {
            return false;
        };
        let s = &self.sessions[&id];
        let vel = Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32);
        match (s.handle, s.away) {
            (Some(h), _) => {
                self.players
                    .teleport(&mut self.physics, h, DVec3::from(p), vel);
                true
            }
            (None, Some(a)) => {
                let sp = &mut self.grids.slots[a.slot].space;
                sp.players
                    .teleport(&mut sp.physics, a.handle, DVec3::from(p), vel);
                true
            }
            _ => false,
        }
    }

    /// The block at a grid's cell (tests, tools): the slot of grid `index`.
    pub fn grid_voxel(&mut self, index: u8, cell: [i32; 3]) -> Option<dwell_blocks::StateId> {
        let slot = self.grids.slots.iter_mut().find(|s| s.index == index)?;
        Some(slot.space.world.get_voxel(cell[0], cell[1], cell[2]))
    }
}
