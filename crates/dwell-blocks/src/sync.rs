//! Registry sync (ADR 0036, docs/BLOCK_REGISTRY.md §8): the **registry payload** a server sends
//! at join — its blocks, their properties and every state's definition as tagged fields — and the
//! client's side, which maps each server state to a state of its own table, adopting the server's
//! definitions where they differ and recording every difference for the join prompt.
//!
//! The payload (format 1; pinned by `shared/protocol/vectors.txt`, `registry_payload`):
//!
//! ```text
//! u16 format
//! u16 shape count,  each: u16 length, bytes       — baked shapes (`shape_bytes`), server order
//! u16 fluid count,  each: str                     — fluid look names states reference
//! u16 block count,  each:
//!   str name, u16 first state id, u8 property count, each: str name, u8 value count, str values
//!   then per state (property order): u8 field count, each: u8 tag, u16 length, bytes
//! ```
//!
//! A tag with the top bit set is a look (a client may ignore it); any other must be understood.

use dwell_protocol::bytes::{ByteReader, ByteWriter};

use crate::{
    AIR, FLUIDS, Facing, Grid, NewBlock, Origin, SHAPES, STATES, ShapeInfo, StateId, StateInfo,
    Textures, VoxelShape, blocks, build_state, info, intern, register, shape::faces_of,
    state_count,
};

/// The payload format this build reads and writes.
pub const FORMAT: u16 = 1;

/// Payloads above this are split into parts (`BlockRegistry` messages).
pub const PART_BYTES: usize = 64 * 1024;

/// Field tags. The top bit marks a look.
pub mod tag {
    pub const LOOK_BIT: u8 = 0x80;
    /// `u32` rule flags (`super::flag`).
    pub const FLAGS: u8 = 1;
    /// `u8` occupancy (0 empty, 1 full, 2 shaped), `u16` shape index.
    pub const SHAPE: u8 = 2;
    /// `f32` kg/m³.
    pub const DENSITY: u8 = 3;
    /// `u8` facing (0 none, 1 north, 2 east, 3 south, 4 west).
    pub const FACING: u8 = 4;
    /// `f32`.
    pub const CLIMB_SPEED: u8 = 5;
    // 6–9 reserved (#95): selection box, placeOn, needsSupport.
    /// `str`.
    pub const LOOK: u8 = 0x81;
    /// `u32` sRGB, `f32` opacity.
    pub const COLOR: u8 = 0x82;
    /// `str` top, side, bottom.
    pub const TEXTURES: u8 = 0x83;
    /// `u8` every, `f32` strength.
    pub const GRID: u8 = 0x84;
    /// `u16` index into the payload's fluid names.
    pub const FLUID: u8 = 0x85;
    // 0x86 reserved: sound set; 0x87–0x8a reserved (#95): render mode, cross, wind, fade.
}

/// Rule flag bits (tag `FLAGS`). Bits 6 (`adaptive`, #82) and 7 (`replaceable`, #95) are reserved.
pub mod flag {
    pub const SOLID: u32 = 1;
    pub const INDESTRUCTIBLE: u32 = 2;
    pub const FLOODED: u32 = 4;
    pub const CLIMBABLE: u32 = 8;
    pub const LIQUID: u32 = 16;
    pub const PLACEABLE: u32 = 32;
    pub const KNOWN: u32 = 63;
    pub const NAMES: [(u32, &str); 6] = [
        (SOLID, "solid"),
        (INDESTRUCTIBLE, "indestructible"),
        (FLOODED, "flooded"),
        (CLIMBABLE, "climbable"),
        (LIQUID, "liquid"),
        (PLACEABLE, "placeable"),
    ];
}

const MAX_STR: usize = 1024;

/// One state's definition: `(tag, bytes)` in tag order.
pub type Fields = Vec<(u8, Vec<u8>)>;

/// A block of the payload.
#[derive(Clone, Debug, PartialEq)]
pub struct PayloadBlock {
    pub name: String,
    /// The server id of its first state; the others follow.
    pub first_state: StateId,
    pub properties: Vec<(String, Vec<String>)>,
    pub states: Vec<Fields>,
}

/// A decoded registry payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Payload {
    pub format: u16,
    pub shapes: Vec<Vec<u8>>,
    pub fluids: Vec<String>,
    pub blocks: Vec<PayloadBlock>,
}

/// FNV-1a 64 over the payload bytes: the definitions hash (`Welcome.registry_hash`).
pub fn definitions_hash(payload: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in payload {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A baked shape's geometry, as the payload carries it and as shapes are matched.
pub fn shape_bytes(s: &ShapeInfo) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = ByteWriter::new(&mut out);
    let faces = faces_of(s);
    w.u8(faces.len() as u8);
    for f in faces {
        w.u8(f.tag);
        w.u8(f.count);
        for p in &f.v {
            for &c in p {
                w.f32(c);
            }
        }
    }
    w.bytes(&s.corners);
    w.u8(u8::from(s.inverted));
    w.u8(s.diagonal);
    w.u8(u8::from(s.convex));
    w.f32(s.volume);
    for side in &s.sides {
        w.bytes(side);
    }
    w.u8(u8::from(s.full_top));
    w.u8(u8::from(s.full_bottom));
    w.f32(s.min_y);
    w.f32(s.max_y);
    out
}

fn facing_code(f: Facing) -> u8 {
    match f {
        Facing::None => 0,
        Facing::North => 1,
        Facing::East => 2,
        Facing::South => 3,
        Facing::West => 4,
    }
}

fn facing_of(c: u8) -> Option<Facing> {
    Some(match c {
        0 => Facing::None,
        1 => Facing::North,
        2 => Facing::East,
        3 => Facing::South,
        4 => Facing::West,
        _ => return None,
    })
}

fn occupancy_code(s: VoxelShape) -> u8 {
    match s {
        VoxelShape::Empty => 0,
        VoxelShape::Full => 1,
        VoxelShape::Shaped => 2,
    }
}

fn field(tag: u8, f: impl FnOnce(&mut ByteWriter)) -> (u8, Vec<u8>) {
    let mut out = Vec::new();
    f(&mut ByteWriter::new(&mut out));
    (tag, out)
}

/// A state's definition fields (shape and fluid indices into the build's tables).
pub fn state_fields(s: &StateInfo) -> Fields {
    let mut flags = 0;
    for (bit, on) in [
        (flag::SOLID, s.solid),
        (flag::INDESTRUCTIBLE, s.indestructible),
        (flag::FLOODED, s.flooded),
        (flag::CLIMBABLE, s.climbable),
        (flag::LIQUID, s.liquid),
        (flag::PLACEABLE, s.placeable),
    ] {
        if on {
            flags |= bit;
        }
    }
    let mut out = vec![
        field(tag::FLAGS, |w| w.u32(flags)),
        field(tag::SHAPE, |w| {
            w.u8(occupancy_code(s.shape));
            w.u16(s.shape_index);
        }),
        field(tag::DENSITY, |w| w.f32(s.density_kg_m3)),
        field(tag::FACING, |w| w.u8(facing_code(s.facing))),
        field(tag::CLIMB_SPEED, |w| w.f32(s.climb_speed_scale)),
        field(tag::LOOK, |w| w.str(s.look)),
        field(tag::COLOR, |w| {
            w.u32(s.color);
            w.f32(s.opacity);
        }),
    ];
    if let Some(t) = s.textures {
        out.push(field(tag::TEXTURES, |w| {
            w.str(t.top);
            w.str(t.side);
            w.str(t.bottom);
        }));
    }
    if let Some(g) = s.grid {
        out.push(field(tag::GRID, |w| {
            w.u8(g.every);
            w.f32(g.strength);
        }));
    }
    if let Some(f) = s.fluid {
        out.push(field(tag::FLUID, |w| w.u16(u16::from(f))));
    }
    out
}

impl Payload {
    /// The registry this process serves: the build's blocks and every block registered as a
    /// server's (never states it synced from another server).
    pub fn served() -> Payload {
        Payload {
            format: FORMAT,
            shapes: SHAPES.iter().map(shape_bytes).collect(),
            fluids: FLUIDS.iter().map(|f| f.name.to_owned()).collect(),
            blocks: blocks()
                .filter(|b| b.origin.served())
                .map(|b| PayloadBlock {
                    name: b.name.to_owned(),
                    first_state: b.first_state,
                    properties: b
                        .properties
                        .iter()
                        .map(|p| {
                            (
                                p.name.to_owned(),
                                p.values.iter().map(|v| (*v).to_owned()).collect(),
                            )
                        })
                        .collect(),
                    states: b
                        .states()
                        .map(|id| state_fields(info(id as StateId)))
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = ByteWriter::new(&mut out);
        w.u16(self.format);
        w.u16(self.shapes.len() as u16);
        for s in &self.shapes {
            w.u16(s.len() as u16);
            w.bytes(s);
        }
        w.u16(self.fluids.len() as u16);
        for f in &self.fluids {
            w.str(f);
        }
        w.u16(self.blocks.len() as u16);
        for b in &self.blocks {
            w.str(&b.name);
            w.u16(b.first_state);
            w.u8(b.properties.len() as u8);
            for (name, values) in &b.properties {
                w.str(name);
                w.u8(values.len() as u8);
                for v in values {
                    w.str(v);
                }
            }
            for fields in &b.states {
                w.u8(fields.len() as u8);
                for (t, bytes) in fields {
                    w.u8(*t);
                    w.u16(bytes.len() as u16);
                    w.bytes(bytes);
                }
            }
        }
        out
    }

    /// Decodes a payload. Errors: truncation, trailing bytes, a format this build cannot read,
    /// overlapping or out-of-range ids, a block without states, or state 0 not being air.
    pub fn decode(bytes: &[u8]) -> Result<Payload, String> {
        let mut r = ByteReader::new(bytes);
        let format = r.u16();
        if r.ok() && format != FORMAT {
            return Err(format!(
                "the server's block registry is format {format}; this version reads format {FORMAT}"
            ));
        }
        let mut shapes = Vec::new();
        for _ in 0..r.u16() {
            let n = usize::from(r.u16());
            shapes.push(r.bytes(n));
            if !r.ok() {
                break;
            }
        }
        let mut fluids = Vec::new();
        for _ in 0..r.u16() {
            fluids.push(r.str(MAX_STR));
            if !r.ok() {
                break;
            }
        }
        let mut blocks = Vec::new();
        let mut next = 0usize;
        for _ in 0..r.u16() {
            let name = r.str(MAX_STR);
            let first_state = r.u16();
            let mut properties = Vec::new();
            for _ in 0..r.u8() {
                let p = r.str(MAX_STR);
                let values: Vec<String> = (0..r.u8()).map(|_| r.str(MAX_STR)).collect();
                r.check(!values.is_empty());
                properties.push((p, values));
            }
            if !r.ok() {
                break;
            }
            let count: usize = properties.iter().map(|(_, v)| v.len()).product();
            if usize::from(first_state) < next || usize::from(first_state) + count > 1 << 16 {
                return Err(format!("{name}: state ids overlap or pass u16"));
            }
            next = usize::from(first_state) + count;
            let mut states = Vec::with_capacity(count);
            for _ in 0..count {
                let mut fields = Vec::new();
                for _ in 0..r.u8() {
                    let t = r.u8();
                    let n = usize::from(r.u16());
                    fields.push((t, r.bytes(n)));
                }
                if !r.ok() {
                    break;
                }
                states.push(fields);
            }
            blocks.push(PayloadBlock {
                name,
                first_state,
                properties,
                states,
            });
        }
        if !r.at_end() {
            return Err("the server's block registry is malformed".into());
        }
        if blocks
            .first()
            .is_none_or(|b| b.first_state != 0 || b.name != "dwell:air" || b.states.len() != 1)
        {
            return Err("the server's block registry does not start with dwell:air".into());
        }
        Ok(Payload {
            format,
            shapes,
            fluids,
            blocks,
        })
    }

    /// Its states: (server id, canonical name, fields).
    pub fn states(&self) -> impl Iterator<Item = (StateId, String, &Fields)> {
        self.blocks.iter().flat_map(|b| {
            b.states.iter().enumerate().map(move |(k, f)| {
                (
                    b.first_state + k as StateId,
                    crate::canonical(&b.name, &b.properties, k),
                    f,
                )
            })
        })
    }

    /// One more than the highest server id.
    pub fn id_end(&self) -> usize {
        self.blocks
            .last()
            .map_or(0, |b| usize::from(b.first_state) + b.states.len())
    }
}

/// A state the join prompt lists: its canonical name and what differs (or a note).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Difference {
    pub name: String,
    pub fields: Vec<String>,
}

/// What a server's registry does differently from this build (ADR 0036 decision 3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Differences {
    /// Known by name, with different rules or shape: the server's are used.
    pub different: Vec<Difference>,
    /// Unknown to this build: shown with the received look, or the fallback look.
    pub added: Vec<Difference>,
    /// This build's blocks or states the server does not have (never sent).
    pub client_only: Vec<String>,
}

impl Differences {
    pub fn is_empty(&self) -> bool {
        self.different.is_empty() && self.added.is_empty() && self.client_only.is_empty()
    }
    /// One line per group, for logs and the F3 overlay.
    pub fn summary(&self) -> String {
        format!(
            "registry: {} different, {} new, {} only here",
            self.different.len(),
            self.added.len(),
            self.client_only.len()
        )
    }
}

/// A session's id translation (ADR 0036 decision 5) and what it found.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    /// Server id → client id (`None`: not a state of the server's registry).
    pub to_client: Vec<Option<StateId>>,
    /// Client id → server id (`None`: a state the server does not have).
    pub to_server: Vec<Option<StateId>>,
    /// Every server id maps to itself, and every client id the server knows maps back to itself.
    pub identity: bool,
    /// No state is "different": locally generated terrain holds the server's rules, so the
    /// worldgen check may pass.
    pub generated_ok: bool,
    pub hash: u64,
    pub differences: Differences,
}

impl Mapping {
    /// The client id of a server id.
    #[inline]
    pub fn client(&self, server: u16) -> Option<StateId> {
        self.to_client.get(usize::from(server)).copied().flatten()
    }
    /// The server id of a client id.
    #[inline]
    pub fn server(&self, client: u16) -> Option<StateId> {
        self.to_server.get(usize::from(client)).copied().flatten()
    }
    /// Translates server ids in place; false if one is not in the server's registry.
    pub fn to_client_ids(&self, ids: &mut [u16]) -> bool {
        if self.identity {
            let end = self.to_client.len();
            return ids.iter().all(|&v| usize::from(v) < end);
        }
        let mut ok = true;
        for v in ids {
            match self.client(*v) {
                Some(c) => *v = c,
                None => ok = false,
            }
        }
        ok
    }
}

/// The parts of a decoded definition.
struct Decoded {
    flags: u32,
    occupancy: u8,
    shape: Option<u16>,
    density: f32,
    facing: u8,
    climb: f32,
    unknown: Vec<u8>,
}

fn decode_rules(fields: &Fields) -> Decoded {
    let mut d = Decoded {
        flags: 0,
        occupancy: 0,
        shape: None,
        density: 0.0,
        facing: 0,
        climb: 1.0,
        unknown: Vec::new(),
    };
    for (t, bytes) in fields {
        let mut r = ByteReader::new(bytes);
        match *t {
            tag::FLAGS => d.flags = r.u32(),
            tag::SHAPE => {
                d.occupancy = r.u8();
                d.shape = Some(r.u16());
            }
            tag::DENSITY => d.density = r.f32(),
            tag::FACING => d.facing = r.u8(),
            tag::CLIMB_SPEED => d.climb = r.f32(),
            t if t & tag::LOOK_BIT == 0 => d.unknown.push(t),
            _ => {}
        }
    }
    d
}

fn read_look(fields: &Fields, fluids: &[String], row: &mut StateInfo) {
    for (t, bytes) in fields {
        let mut r = ByteReader::new(bytes);
        match *t {
            tag::LOOK => {
                let s = r.str(MAX_STR);
                if r.ok() {
                    row.look = intern(&s);
                }
            }
            tag::COLOR => {
                let (c, o) = (r.u32(), r.f32());
                if r.ok() {
                    row.color = c;
                    row.opacity = o;
                }
            }
            tag::TEXTURES => {
                let (a, b, c) = (r.str(MAX_STR), r.str(MAX_STR), r.str(MAX_STR));
                if r.ok() {
                    row.textures = Some(Textures {
                        top: intern(&a),
                        side: intern(&b),
                        bottom: intern(&c),
                    });
                }
            }
            tag::GRID => {
                let (every, strength) = (r.u8(), r.f32());
                if r.ok() {
                    row.grid = Some(Grid { every, strength });
                }
            }
            tag::FLUID => {
                let i = usize::from(r.u16());
                row.fluid = fluids.get(i).and_then(|name| crate::fluid(name));
            }
            _ => {}
        }
    }
}

/// The client side: maps a server's registry onto this process's table, registering synced
/// states (ADR 0036 decision 3). Never refuses over differences; errors only when the table would
/// pass `u16`.
pub fn map(payload: &Payload, hash: u64) -> Result<Mapping, String> {
    // Server shape index → build shape index, by geometry.
    let own: Vec<Vec<u8>> = SHAPES.iter().map(shape_bytes).collect();
    let shape_map: Vec<Option<u16>> = payload
        .shapes
        .iter()
        .map(|s| own.iter().position(|o| o == s).map(|i| i as u16))
        .collect();
    let cube = STATES
        .iter()
        .find(|s| s.shape == VoxelShape::Full)
        .map_or(0, |s| s.shape_index);

    let mut differences = Differences::default();
    let mut to_client: Vec<Option<StateId>> = vec![None; payload.id_end()];
    let mut seen_build = vec![false; STATES.len()];
    let mut generated_ok = true;
    // Build states whose server definition differs: (build id, server id). Edits naming the
    // build's state (the palette's) still mean the server's state of that name.
    let mut renamed: Vec<(StateId, StateId)> = Vec::new();

    for b in &payload.blocks {
        // Each state: the server's rules (with notes on what was degraded), and whether the
        // build's state of that name already has them.
        let mut rows = Vec::with_capacity(b.states.len());
        let mut needs_copy = false;
        for (k, fields) in b.states.iter().enumerate() {
            let name = crate::canonical(&b.name, &b.properties, k);
            let d = decode_rules(fields);
            let mut notes = Vec::new();
            let known = build_state(&name);
            let mut row = match known {
                Some(id) => *info(id),
                None => StateInfo {
                    name: "",
                    block: 0,
                    density_kg_m3: 0.0,
                    solid: false,
                    indestructible: false,
                    shape: VoxelShape::Empty,
                    shape_index: 0,
                    flooded: false,
                    climbable: false,
                    facing: Facing::None,
                    climb_speed_scale: 1.0,
                    liquid: false,
                    fluid: None,
                    placeable: false,
                    look: "cube",
                    color: 0,
                    opacity: 1.0,
                    textures: None,
                    grid: None,
                },
            };
            let before = row;
            row.name = intern(&name);
            if known.is_none() {
                read_look(fields, &payload.fluids, &mut row);
            }
            let has = |bit| d.flags & bit != 0;
            row.solid = has(flag::SOLID);
            row.indestructible = has(flag::INDESTRUCTIBLE);
            row.flooded = has(flag::FLOODED);
            row.climbable = has(flag::CLIMBABLE);
            row.liquid = has(flag::LIQUID);
            row.placeable = has(flag::PLACEABLE);
            row.density_kg_m3 = d.density;
            row.climb_speed_scale = d.climb;
            row.facing = facing_of(d.facing).unwrap_or_else(|| {
                notes.push(format!("facing {} not in this version (none)", d.facing));
                Facing::None
            });
            let occupancy = match d.occupancy {
                0 => VoxelShape::Empty,
                1 => VoxelShape::Full,
                _ => VoxelShape::Shaped,
            };
            match d
                .shape
                .and_then(|i| shape_map.get(usize::from(i)).copied().flatten())
            {
                Some(i) => {
                    row.shape = occupancy;
                    row.shape_index = i;
                }
                None if occupancy == VoxelShape::Empty => {
                    row.shape = VoxelShape::Empty;
                    row.shape_index = 0;
                }
                None => {
                    notes.push("shape not in this version: a full cube here".into());
                    row.shape = VoxelShape::Full;
                    row.shape_index = cube;
                }
            }
            if d.flags & !flag::KNOWN != 0 {
                notes.push(format!(
                    "rule flags {:#x} not in this version (ignored)",
                    d.flags & !flag::KNOWN
                ));
            }
            for t in &d.unknown {
                notes.push(format!("rule field {t} not in this version (ignored)"));
            }
            match known {
                Some(id) => {
                    seen_build[usize::from(id)] = true;
                    let mut changed = changed_fields(&before, &row);
                    changed.extend(notes);
                    if changed.is_empty() {
                        rows.push((name, row, Some(id), Vec::new()));
                    } else {
                        needs_copy = true;
                        differences.different.push(Difference {
                            name: name.clone(),
                            fields: changed.clone(),
                        });
                        renamed.push((id, b.first_state + k as StateId));
                        rows.push((name, row, None, changed));
                    }
                }
                None => {
                    needs_copy = true;
                    differences.added.push(Difference {
                        name: name.clone(),
                        fields: notes.clone(),
                    });
                    rows.push((name, row, None, notes));
                }
            }
        }
        let copy_first = if needs_copy {
            let block = NewBlock {
                name: b.name.clone(),
                properties: b.properties.clone(),
                states: rows.iter().map(|(_, row, _, _)| *row).collect(),
            };
            Some(register(Origin::Synced, block)?)
        } else {
            None
        };
        for (k, (_, _, build, _)) in rows.iter().enumerate() {
            let server = usize::from(b.first_state) + k;
            to_client[server] = match (build, copy_first) {
                (Some(id), _) => Some(*id),
                (None, Some(first)) => Some(first + k as StateId),
                (None, None) => unreachable!("a copy exists for every unmatched state"),
            };
        }
    }
    if !differences.different.is_empty() {
        generated_ok = false;
    }

    // Build states the server does not list: whole blocks by name, else single states.
    for b in blocks().filter(|b| b.origin == Origin::Build) {
        let missing: Vec<usize> = b.states().filter(|&i| !seen_build[i]).collect();
        if missing.len() == b.states().len() {
            differences.client_only.push(b.name.to_owned());
        } else {
            differences
                .client_only
                .extend(missing.iter().map(|&i| STATES[i].name.to_owned()));
        }
    }

    let mut to_server: Vec<Option<StateId>> = vec![None; state_count()];
    for (server, client) in to_client.iter().enumerate() {
        if let Some(c) = client {
            to_server[usize::from(*c)] = Some(server as StateId);
        }
    }
    for (build, server) in renamed {
        to_server[usize::from(build)] = Some(server);
    }
    let identity = to_client
        .iter()
        .enumerate()
        .all(|(i, c)| *c == Some(i as StateId))
        && to_server
            .iter()
            .enumerate()
            .all(|(i, s)| s.is_none_or(|s| usize::from(s) == i));
    debug_assert_eq!(to_client.first().copied().flatten(), Some(AIR));
    Ok(Mapping {
        to_client,
        to_server,
        identity,
        generated_ok,
        hash,
        differences,
    })
}

/// The names of the rule and shape fields that differ between two rows.
fn changed_fields(a: &StateInfo, b: &StateInfo) -> Vec<String> {
    let mut out = Vec::new();
    for (bit, name) in flag::NAMES {
        let get = |s: &StateInfo| match bit {
            flag::SOLID => s.solid,
            flag::INDESTRUCTIBLE => s.indestructible,
            flag::FLOODED => s.flooded,
            flag::CLIMBABLE => s.climbable,
            flag::LIQUID => s.liquid,
            _ => s.placeable,
        };
        if get(a) != get(b) {
            out.push(name.to_owned());
        }
    }
    if a.shape != b.shape || a.shape_index != b.shape_index {
        out.push("shape".into());
    }
    if a.density_kg_m3.to_bits() != b.density_kg_m3.to_bits() {
        out.push("density".into());
    }
    if a.facing != b.facing {
        out.push("facing".into());
    }
    if a.climb_speed_scale.to_bits() != b.climb_speed_scale.to_bits() {
        out.push("climb speed".into());
    }
    out
}

impl Mapping {
    /// The `WorldgenCheck` hash of the verification chunk the client generated (`voxels`, its
    /// own ids; `hash` the job's FNV-1a of them): the same FNV-1a over the server's ids, which is
    /// what the server hashes (ADR 0036 decision 6). 0 — every chunk explicit — when a state
    /// differs on the server (generated terrain would hold the build's rules) or a cell names a
    /// state the server does not have.
    pub fn verification_hash(&self, voxels: &[u16], hash: u64) -> u64 {
        if !self.generated_ok {
            return 0;
        }
        if self.identity {
            return hash;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &v in voxels {
            let Some(s) = self.server(v) else { return 0 };
            for b in s.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
}

/// The states this process registered at run time, as a payload (build shape and fluid indices):
/// what the main thread sends its job workers — separate WASM instances with tables of their own —
/// so they read synced ids as it does ([`apply_extension`]).
pub fn extension_payload() -> Vec<u8> {
    Payload {
        format: FORMAT,
        shapes: Vec::new(),
        fluids: Vec::new(),
        blocks: blocks()
            .filter(|b| b.origin != Origin::Build)
            .map(|b| PayloadBlock {
                name: b.name.to_owned(),
                first_state: b.first_state,
                properties: b
                    .properties
                    .iter()
                    .map(|p| {
                        (
                            p.name.to_owned(),
                            p.values.iter().map(|v| (*v).to_owned()).collect(),
                        )
                    })
                    .collect(),
                states: b
                    .states()
                    .map(|id| state_fields(info(id as StateId)))
                    .collect(),
            })
            .collect(),
    }
    .encode()
}

/// Registers an [`extension_payload`]'s blocks this table lacks, at the same ids: blocks already
/// here (ids below the table's end) are skipped; a gap is an error.
pub fn apply_extension(bytes: &[u8]) -> Result<(), String> {
    let mut r = ByteReader::new(bytes);
    // Not `Payload::decode`: an extension starts past air.
    let _format = r.u16();
    r.u16();
    r.u16();
    let count = r.u16();
    let mut blocks = Vec::new();
    for _ in 0..count {
        let name = r.str(MAX_STR);
        let first_state = r.u16();
        let mut properties = Vec::new();
        for _ in 0..r.u8() {
            let p = r.str(MAX_STR);
            let values: Vec<String> = (0..r.u8()).map(|_| r.str(MAX_STR)).collect();
            properties.push((p, values));
        }
        let n: usize = properties.iter().map(|(_, v)| v.len()).product();
        let mut states = Vec::new();
        for _ in 0..n {
            let mut fields = Vec::new();
            for _ in 0..r.u8() {
                let t = r.u8();
                let len = usize::from(r.u16());
                fields.push((t, r.bytes(len)));
            }
            states.push(fields);
        }
        if !r.ok() {
            break;
        }
        blocks.push(PayloadBlock {
            name,
            first_state,
            properties,
            states,
        });
    }
    if !r.at_end() {
        return Err("malformed extension".into());
    }
    let fluids: Vec<String> = FLUIDS.iter().map(|f| f.name.to_owned()).collect();
    for b in blocks {
        let have = state_count();
        if usize::from(b.first_state) < have {
            continue;
        }
        if usize::from(b.first_state) > have {
            return Err(format!("{}: ids {have}.. missing", b.name));
        }
        let rows = b
            .states
            .iter()
            .enumerate()
            .map(|(k, fields)| {
                row_from_fields(
                    &crate::canonical(&b.name, &b.properties, k),
                    fields,
                    &fluids,
                )
            })
            .collect();
        register(
            Origin::Synced,
            NewBlock {
                name: b.name,
                properties: b.properties,
                states: rows,
            },
        )?;
    }
    Ok(())
}

/// A state's row from its fields as this build wrote them (`state_fields`: build shape and fluid
/// indices), `fluids` the names the fluid indices refer to.
pub fn row_from_fields(name: &str, fields: &Fields, fluids: &[String]) -> StateInfo {
    let d = decode_rules(fields);
    let mut row = *info(AIR);
    row.name = intern(name);
    row.textures = None;
    row.grid = None;
    row.fluid = None;
    read_look(fields, fluids, &mut row);
    let has = |bit| d.flags & bit != 0;
    row.solid = has(flag::SOLID);
    row.indestructible = has(flag::INDESTRUCTIBLE);
    row.flooded = has(flag::FLOODED);
    row.climbable = has(flag::CLIMBABLE);
    row.liquid = has(flag::LIQUID);
    row.placeable = has(flag::PLACEABLE);
    row.density_kg_m3 = d.density;
    row.climb_speed_scale = d.climb;
    row.facing = facing_of(d.facing).unwrap_or(Facing::None);
    row.shape = match d.occupancy {
        0 => VoxelShape::Empty,
        1 => VoxelShape::Full,
        _ => VoxelShape::Shaped,
    };
    row.shape_index = d.shape.unwrap_or(0);
    row
}

/// Splits a payload into `BlockRegistry` parts.
pub fn parts(payload: &[u8]) -> Vec<&[u8]> {
    if payload.is_empty() {
        return vec![payload];
    }
    payload.chunks(PART_BYTES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::require;

    #[test]
    fn the_served_registry_round_trips_and_maps_to_itself() {
        let p = Payload::served();
        let bytes = p.encode();
        assert_eq!(Payload::decode(&bytes).expect("decodes"), p);
        let m = map(&p, definitions_hash(&bytes)).expect("maps");
        assert!(m.identity, "{:?}", m.differences);
        assert!(m.generated_ok);
        assert!(m.differences.different.is_empty() && m.differences.added.is_empty());
        assert_eq!(m.client(require("dwell:sand")), Some(require("dwell:sand")));
    }

    #[test]
    fn bad_payloads_fail_with_a_reason() {
        let mut bytes = Payload::served().encode();
        bytes[0] = 9;
        assert!(Payload::decode(&bytes).unwrap_err().contains("format 9"));
        let bytes = Payload::served().encode();
        assert!(Payload::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Payload::decode(&longer).is_err());
    }

    fn set_field(fields: &mut Fields, t: u8, bytes: Vec<u8>) {
        match fields.iter_mut().find(|(have, _)| *have == t) {
            Some(f) => f.1 = bytes,
            None => fields.push((t, bytes)),
        }
    }

    /// A server that dropped `dwell:ladder` (later ids shift down), changed sand's density, added
    /// a block with an unknown look, one with a shape this build lacks and an unknown rule field.
    fn modded() -> Payload {
        let mut p = Payload::served();
        let ladder = p
            .blocks
            .iter()
            .position(|b| b.name == "dwell:ladder")
            .expect("ladder");
        let removed = p.blocks.remove(ladder).states.len() as u16;
        for b in &mut p.blocks[ladder..] {
            b.first_state -= removed;
        }
        let sand = p
            .blocks
            .iter_mut()
            .find(|b| b.name == "dwell:sand")
            .expect("sand");
        set_field(
            &mut sand.states[0],
            tag::DENSITY,
            999f32.to_le_bytes().to_vec(),
        );
        let stone = p
            .blocks
            .iter()
            .find(|b| b.name == "dwell:stone")
            .expect("stone")
            .clone();
        let mut next = p.id_end() as u16;
        let mut glow = stone.states[0].clone();
        let mut look = Vec::new();
        ByteWriter::new(&mut look).str("test:glow_look");
        set_field(&mut glow, tag::LOOK, look);
        set_field(&mut glow, 9, vec![1]);
        p.blocks.push(PayloadBlock {
            name: "test:glow".into(),
            first_state: next,
            properties: vec![("lit".into(), vec!["false".into(), "true".into()])],
            states: vec![glow.clone(), glow],
        });
        next += 2;
        p.shapes.push(vec![7; 3]);
        let mut wedge = stone.states[0].clone();
        let mut shape = vec![2];
        shape.extend_from_slice(&((p.shapes.len() - 1) as u16).to_le_bytes());
        set_field(&mut wedge, tag::SHAPE, shape);
        p.blocks.push(PayloadBlock {
            name: "test:wedge".into(),
            first_state: next,
            properties: vec![],
            states: vec![wedge],
        });
        p
    }

    fn server_id(p: &Payload, name: &str) -> StateId {
        p.states().find(|(_, n, _)| n == name).expect(name).0
    }

    #[test]
    fn differences_are_adopted_and_listed_never_refused() {
        let p = modded();
        let bytes = p.encode();
        assert_eq!(Payload::decode(&bytes).expect("decodes"), p);
        let m = map(&p, definitions_hash(&bytes)).expect("maps");
        assert!(!m.identity);
        assert!(
            !m.generated_ok,
            "sand differs: generated terrain would hold the build's sand"
        );
        let d = &m.differences;
        assert_eq!(d.different.len(), 1);
        assert_eq!(d.different[0].name, "dwell:sand");
        assert_eq!(d.different[0].fields, vec!["density".to_string()]);
        assert_eq!(d.client_only, vec!["dwell:ladder".to_string()]);
        let names: Vec<&str> = d.added.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            ["test:glow[lit=false]", "test:glow[lit=true]", "test:wedge"]
        );
        assert!(d.added[0].fields.iter().any(|f| f.contains("rule field 9")));
        assert!(d.added[2].fields.iter().any(|f| f.contains("full cube")));

        // Ids shift past the removed ladder; each maps by name, both ways.
        for (server, name, _) in p.states() {
            let client = m.client(server).expect("mapped");
            assert_eq!(info(client).name, name);
            assert_eq!(m.server(client), Some(server));
        }
        let sand_here = m.client(server_id(&p, "dwell:sand")).expect("sand");
        assert!(
            !crate::is_build_state(sand_here),
            "a synced copy with the server's rules"
        );
        assert_eq!(info(sand_here).density_kg_m3, 999.0);
        assert_eq!(
            info(sand_here).textures,
            info(require("dwell:sand")).textures,
            "its own look"
        );
        assert_eq!(
            m.server(require("dwell:sand")),
            Some(server_id(&p, "dwell:sand")),
            "the palette's sand places the server's sand"
        );
        assert_eq!(
            m.server(require("dwell:ladder")),
            None,
            "client-only: never sent"
        );
        let glow = info(
            m.client(server_id(&p, "test:glow[lit=true]"))
                .expect("glow"),
        );
        assert_eq!(glow.look, "test:glow_look");
        assert!(glow.solid && glow.shape == VoxelShape::Full);
        let glow_off = m
            .client(server_id(&p, "test:glow[lit=false]"))
            .expect("glow");
        assert_eq!(crate::state_property(glow_off, "lit"), Some("false"));
        assert_eq!(
            crate::with_property(glow_off, "lit", "true"),
            m.client(server_id(&p, "test:glow[lit=true]"))
        );

        // A second join registers nothing new.
        let before = state_count();
        let again = map(&p, m.hash).expect("maps");
        assert_eq!(again.to_client, m.to_client);
        assert_eq!(state_count(), before);
    }

    #[test]
    fn ids_translate_in_place_and_unknown_ids_fail() {
        let p = modded();
        let m = map(&p, 0).expect("maps");
        let stone = require("dwell:stone");
        let mut ids = vec![0, stone, server_id(&p, "dwell:sand")];
        assert!(m.to_client_ids(&mut ids));
        assert_eq!(ids[..2], [0, stone]);
        assert_eq!(info(ids[2]).name, "dwell:sand");
        let mut bad = vec![p.id_end() as u16];
        assert!(!m.to_client_ids(&mut bad));
    }

    #[test]
    fn rows_survive_their_fields_and_the_extension_reapplies_as_a_no_op() {
        let fluids: Vec<String> = FLUIDS.iter().map(|f| f.name.to_owned()).collect();
        for s in &STATES {
            let row = row_from_fields(s.name, &state_fields(s), &fluids);
            assert!(crate::same_definition(s, &row), "{}", s.name);
        }
        let _ = map(&modded(), 0).expect("maps");
        let before = state_count();
        apply_extension(&extension_payload()).expect("applies");
        assert_eq!(state_count(), before);
    }

    #[test]
    fn the_verification_hash_is_over_server_ids() {
        let p = modded();
        let mut m = map(&p, 0).expect("maps");
        let stone = require("dwell:stone");
        let server_stone = server_id(&p, "dwell:stone");
        assert_eq!(
            server_stone, stone,
            "ids before the removed ladder are kept"
        );
        let voxels = vec![0, stone, require("dwell:water")];
        let server: Vec<u16> = voxels.iter().map(|&v| m.server(v).unwrap()).collect();
        let mut want: u64 = 0xcbf2_9ce4_8422_2325;
        for v in &server {
            for b in v.to_le_bytes() {
                want ^= u64::from(b);
                want = want.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        assert_eq!(
            m.verification_hash(&voxels, 1),
            0,
            "sand differs: explicit chunks"
        );
        m.generated_ok = true;
        assert_eq!(m.verification_hash(&voxels, 1), want);
        assert_ne!(server[2], voxels[2], "water's id shifted");
        assert_eq!(
            m.verification_hash(&[require("dwell:ladder")], 1),
            0,
            "client-only"
        );
    }
}
