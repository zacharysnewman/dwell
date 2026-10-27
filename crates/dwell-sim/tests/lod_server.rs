//! The server's side of level of detail (ARCHITECTURE.md §6.6, ADR 0012): propagation of chunk
//! changes up the octree, the LOD index and its updates, and LodRequest / LodData.
//! A port of `server/tests/lod_server_test.cpp`.

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use common::*;
use dwell_protocol::*;
use dwell_sim::lod_propagation::LodPropagation;
use dwell_sim::server::{OutKind, SessionId};
use dwell_world::lod::{
    INDEX_LEVEL, LodCells, LodCoord, MAX_LEVEL, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, VOLUME, ancestor,
    cell_index, cell_size, generate_flat_lod, lod_of_chunk, section_origin, section_size,
};
use dwell_world::{Chunk, ChunkCoord, M, chunk_hash, chunk_of, generate_flat_chunk};

/// The chunk row holding the flat world's surface row (`FLAT_TOP_Y`): 2 on ½ m cells.
const GC: i32 = dwell_world::FLAT_TOP_Y / dwell_world::CHUNK_SIZE;

/// A chunk's ancestor at `level`.
fn above(c: ChunkCoord, level: i32) -> LodCoord {
    ancestor(lod_of_chunk(c), level)
}

fn ask(c: LodCoord, known: u32) -> LodSectionRequest {
    LodSectionRequest {
        level: c.level as u8,
        section: [c.i, c.j, c.k],
        known_revision: known,
    }
}

/// The client side of the verification check (§6.3) on the flat world.
fn verify(f: &mut Fixture, id: SessionId, w: &Welcome, full: bool) {
    let mut v = Chunk::default();
    let vc = w.verification_chunk;
    generate_flat_chunk(ChunkCoord::new(vc[0], vc[1], vc[2]), &mut v);
    f.send(
        id,
        Message::WorldgenCheck(if full { 0 } else { chunk_hash(&v) }),
    );
}

/// The flat world's surface (feet height on its ground).
const G: f64 = dwell_world::FLAT_TOP_M;

fn flat_lod(c: LodCoord) -> LodCells {
    let mut cells = LodCells::new();
    generate_flat_lod(c, &mut cells);
    cells
}

fn flat_generator() -> dwell_world::lod::LodGenerator {
    std::sync::Arc::new(generate_flat_lod)
}

/// What a client received on the lod stream.
#[derive(Default)]
struct LodInbox {
    index: Vec<LodIndex>,
    updates: Vec<Vec<LodIndexEntry>>,
    data: Vec<LodData>,
}

impl LodInbox {
    fn add(&mut self, messages: Vec<Message>) {
        for m in messages {
            match m {
                Message::LodIndex(i) => self.index.push(i),
                Message::LodIndexUpdate(u) => self.updates.push(u),
                Message::LodData(d) => self.data.push(d),
                _ => {}
            }
        }
    }
    fn find(&self, c: LodCoord) -> Option<&LodData> {
        self.data
            .iter()
            .rev()
            .find(|d| i32::from(d.level) == c.level && d.section == [c.i, c.j, c.k])
    }
}

type Boxes = BTreeMap<SessionId, LodInbox>;

/// Steps once and files each session's messages.
fn step_into(f: &mut Fixture, boxes: &mut Boxes) {
    f.server.step();
    for (id, messages) in f.take_all() {
        boxes.entry(id).or_default().add(messages);
    }
}

/// A chunk filled with stone.
fn stone() -> Chunk {
    let mut c = Chunk::default();
    c.voxels_mut().fill(M.stone);
    c.set_revision(1);
    c
}

// ---- lod: propagation ----

#[test]
fn a_chunk_change_is_downsampled_into_its_sections_up_to_the_root_each_a_new_revision() {
    let mut lod = LodPropagation::new(flat_generator(), 0);
    let edited = ChunkCoord::new(0, GC, 0); // above the flat ground: air when generated
    let stone = stone();
    let mut modified = |c: ChunkCoord| (c == edited).then(|| stone.voxels().to_vec());
    lod.mark_chunk(edited);
    lod.drain(&[], &mut modified);
    let mut previous = 0;
    for level in 1..=MAX_LEVEL {
        let s = lod
            .find(above(edited, level))
            .unwrap_or_else(|| panic!("level {level}"));
        assert!(s.revision > previous, "level {level}"); // children are written before their parents
        previous = s.revision;
    }
    assert_eq!(lod.sections().len(), MAX_LEVEL as usize);
    // The stone cube shows where it fills cells; the rest of the section is as generated. (From
    // level 6 a cell holding the chunk also holds the flat ground below it: solid either way.)
    for level in 1..=5 {
        let c = above(edited, level);
        let cells = lod.cells_for_client(c);
        let generated = flat_lod(c);
        let o = section_origin(c);
        let size = cell_size(level);
        // The cell holding the chunk's lowest corner.
        let (x, y, z) = (
            (-o.x / size) as i32,
            ((i64::from(edited.y) * 32 - o.y) / size) as i32,
            (-o.z / size) as i32,
        );
        let at = cell_index(x, y, z);
        assert_eq!(generated[at], M.air, "level {level}");
        assert_eq!(cells[at], M.stone, "level {level}"); // 32 m of stone: one level-5 cell
        let differ = cells.iter().zip(&generated).filter(|(a, b)| a != b).count();
        assert!(differ > 0, "level {level}");
    }
}

#[test]
fn propagation_is_budgeted_per_tick_and_runs_on_threads_with_the_same_result() {
    let stone = stone();
    let mut modified = |c: ChunkCoord| (c.y == 0).then(|| stone.voxels().to_vec());
    // 40 changed chunks in a row: 40 level-1 sections... fewer (2 chunks per section along x).
    let mut inline_lod = LodPropagation::new(flat_generator(), 0);
    for x in 0..40 {
        inline_lod.mark_chunk(ChunkCoord::new(x * 2, GC - 1, 0));
    }
    inline_lod.step(
        &[[0.0, 0.0, 0.0]],
        &mut modified,
        8,
        Duration::from_secs(3600),
    );
    let first = inline_lod.take_written();
    assert_eq!(first.len(), 8);
    // Nearest to the player first, lowest level first.
    for c in &first {
        assert_eq!(c.level, 1);
        assert!(section_origin(*c).x < 8 * 64 + 64, "{c:?}");
    }
    inline_lod.drain(&[[0.0, 0.0, 0.0]], &mut modified);

    let mut threaded = LodPropagation::new(flat_generator(), 2);
    for x in 0..40 {
        threaded.mark_chunk(ChunkCoord::new(x * 2, GC - 1, 0));
    }
    threaded.drain(&[[0.0, 0.0, 0.0]], &mut modified);
    assert_eq!(threaded.sections().len(), inline_lod.sections().len());
    for (c, s) in inline_lod.sections() {
        let t = threaded.find(*c).unwrap_or_else(|| panic!("{c:?}"));
        assert_eq!(t.encoded, s.encoded, "{c:?}");
    }
}

// ---- lod: streaming ----

#[test]
fn the_index_follows_the_worldgen_check_untouched_worlds_cost_no_lod_data() {
    let mut f = Fixture::new(flat());
    let mut boxes = Boxes::new();
    let w = f.join(1, &Client::new(1));
    for _ in 0..5 {
        step_into(&mut f, &mut boxes);
    }
    assert!(boxes.entry(1).or_default().index.is_empty()); // nothing before the check
    verify(&mut f, 1, &w, false);
    for _ in 0..120 {
        step_into(&mut f, &mut boxes);
    }
    let b = &boxes[&1];
    assert_eq!(b.index.len(), 1);
    assert!(b.index[0].last);
    assert!(b.index[0].entries.is_empty());
    assert!(b.updates.is_empty());
    assert!(b.data.is_empty());
}

#[test]
fn an_edit_reaches_a_far_client_index_update_then_explicit_only_where_it_changed() {
    // A builds near the spawn; B is 40 km away and holds sections over A's area; C, 45 km the
    // other way, has no modification in view and requests nothing (Phase 16: the 50 km disc).
    let mut f = Fixture::new(flat());
    let mut boxes = Boxes::new();
    let a = f.join(1, &Client::new(1));
    let b = f.join(2, &Client::new(2));
    let c = f.join(3, &Client::new(3));
    f.move_to(b.player_id, 40000.5, G, 0.5);
    f.move_to(c.player_id, -45000.5, G, 0.5);
    for (id, w) in [(1, &a), (2, &b), (3, &c)] {
        verify(&mut f, id, w, false);
    }
    for _ in 0..5 {
        step_into(&mut f, &mut boxes);
    }

    // B holds these sections over the spawn, all generated so far (the index is empty).
    let site1 = ChunkCoord::new(-1, GC, 0); // cell (-1, G, 2)
    let site2 = chunk_of(-5001, dwell_world::FLAT_TOP_Y, 2); // another level-7 section, same level 8
    let index_section = above(site1, INDEX_LEVEL);
    assert_eq!(above(site2, INDEX_LEVEL), index_section);
    assert_ne!(above(site2, 7), above(site1, 7));

    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [-1, dwell_world::FLAT_TOP_Y - 1, 2],
            face: 2,
            material: M.sand,
            rows: 1,
            down: false,
        }),
    );
    let edit_tick = f.server.tick();
    // The server's sections above the edit change within a bounded time, and B hears of it.
    let updated = |boxes: &Boxes| {
        boxes.get(&2).is_some_and(|b| {
            b.updates
                .iter()
                .flatten()
                .any(|e| e.i == index_section.i && e.k == index_section.k)
        })
    };
    let mut ticks = 0;
    while !updated(&boxes) && ticks < 600 {
        step_into(&mut f, &mut boxes);
        ticks += 1;
    }
    assert!(updated(&boxes));
    println!("index update reached a far client {ticks} ticks after the edit");
    assert!(ticks <= 60, "{ticks}"); // ≤ 1 s
    let root = LodCoord::new(MAX_LEVEL, 0, 0, 0);
    while f.server.lod().find(root).is_none() && f.server.tick() < edit_tick + 600 {
        step_into(&mut f, &mut boxes);
    }
    assert!(f.server.lod().find(root).is_some());
    assert!(f.server.tick() - edit_tick <= 60); // up to the root within a second

    // B re-requests what it holds under the entry, and the entry's ancestors.
    let mut held = vec![
        ask(index_section, 0),
        ask(above(site1, 7), 0),
        ask(above(site2, 7), 0),
        ask(above(site1, 3), 0),
    ];
    for level in INDEX_LEVEL + 1..=MAX_LEVEL {
        held.push(ask(above(site1, level), 0));
    }
    f.send(2, Message::LodRequest(held));
    for _ in 0..10 {
        step_into(&mut f, &mut boxes);
    }
    let (known_entry, known_path7) = {
        let inbox = &boxes[&2];
        let entry = inbox.find(index_section);
        let path7 = inbox.find(above(site1, 7));
        let other7 = inbox.find(above(site2, 7));
        let path3 = inbox.find(above(site1, 3));
        let (Some(entry), Some(path7), Some(other7), Some(path3)) = (entry, path7, other7, path3)
        else {
            panic!(
                "missing: entry {} path7 {} other7 {} path3 {}",
                entry.is_some(),
                path7.is_some(),
                other7.is_some(),
                path3.is_some()
            );
        };
        assert_eq!(entry.form, LodForm::Explicit);
        assert_eq!(entry.cells.len(), LOD_CELL_COUNT);
        assert_eq!(path7.form, LodForm::Explicit);
        assert_eq!(path3.form, LodForm::Explicit);
        assert_eq!(other7.form, LodForm::Generated); // nothing modified there: B generates it
        for level in INDEX_LEVEL + 1..=MAX_LEVEL {
            let d = inbox
                .find(above(site1, level))
                .unwrap_or_else(|| panic!("level {level}"));
            assert_eq!(d.form, LodForm::Explicit, "level {level}");
        }
        (entry.revision, path7.revision)
    };

    // A second edit elsewhere under the same entry: B asks again with what it holds. Only the
    // changed path comes back Explicit; the first path's section is Unchanged.
    let updates_before = boxes[&2].updates.len();
    f.move_to(a.player_id, -4999.5, G, 0.5);
    for _ in 0..3 {
        step_into(&mut f, &mut boxes);
    }
    f.send(
        1,
        Message::BlockEditRequest(BlockEditRequest {
            action: BlockEditAction::Place,
            cell: [-5001, dwell_world::FLAT_TOP_Y - 1, 2],
            face: 2,
            material: M.sand,
            rows: 1,
            down: false,
        }),
    );
    for _ in 0..120 {
        if boxes[&2].updates.len() != updates_before {
            break;
        }
        step_into(&mut f, &mut boxes);
    }
    assert!(boxes[&2].updates.len() > updates_before);
    let bytes_before = f.server.lod_stats_of(b.player_id).unwrap().bytes;
    let explicit_before = f.server.lod_stats_of(b.player_id).unwrap().explicit_sent;
    f.send(
        2,
        Message::LodRequest(vec![
            ask(index_section, known_entry),
            ask(above(site1, 7), known_path7),
            ask(above(site2, 7), 0),
        ]),
    );
    for _ in 0..10 {
        step_into(&mut f, &mut boxes);
    }
    let inbox = &boxes[&2];
    assert_eq!(inbox.find(index_section).unwrap().form, LodForm::Explicit);
    assert!(inbox.find(index_section).unwrap().revision > known_entry);
    assert_eq!(
        inbox.find(above(site1, 7)).unwrap().form,
        LodForm::Unchanged
    );
    assert_eq!(inbox.find(above(site2, 7)).unwrap().form, LodForm::Explicit);
    assert_eq!(
        f.server.lod_stats_of(b.player_id).unwrap().explicit_sent,
        explicit_before + 2
    );
    println!(
        "second round: {} bytes for 3 sections",
        f.server.lod_stats_of(b.player_id).unwrap().bytes - bytes_before
    );

    // C saw the index and its updates, and no section data.
    let inbox_c = &boxes[&3];
    assert_eq!(inbox_c.index.len(), 1);
    assert!(!inbox_c.updates.is_empty());
    assert!(inbox_c.data.is_empty());
    assert_eq!(f.server.lod_stats_of(c.player_id).unwrap().explicit_sent, 0);
}

#[test]
fn requests_are_rate_limited_and_answered_within_the_byte_budget() {
    let config = dwell_sim::server::ServerConfig {
        lod_bytes_per_second: 60 * 2000,
        ..flat()
    }; // 2 KB per tick
    let sleep = config.lod_threads != Some(0);
    let mut f = Fixture::new(config);
    let mut boxes = Boxes::new();
    let w = f.join(1, &Client::new(1));
    verify(&mut f, 1, &w, true); // full-chunk mode: every answer Explicit (~1–6 KB)
    step_into(&mut f, &mut boxes);
    // 3 × 32 sections at once: the bucket holds LOD_REQUESTS_PER_SECOND.
    let base = above(ChunkCoord::new(0, GC - 1, 0), 2);
    for m in 0..3 {
        let r = (0..32)
            .map(|i| ask(LodCoord::new(2, base.i + m * 32 + i, base.j, base.k), 0))
            .collect();
        f.send(1, Message::LodRequest(r));
    }
    let after = f.server.lod_stats_of(w.player_id).unwrap();
    assert_eq!(after.requests, lod::REQUESTS_PER_SECOND as u32);
    assert_eq!(after.requests_dropped, 96 - lod::REQUESTS_PER_SECOND as u32);
    // Bytes per tick stay within the budget (one message may overdraw it).
    let mut max_tick = 0;
    let inbox = boxes.entry(1).or_default();
    for _ in 0..600 {
        if inbox.data.len() >= 64 {
            break;
        }
        let mut bytes = 0;
        f.server.step();
        for o in f.server.take_outbox() {
            if o.kind != OutKind::Reliable(Channel::Lod) {
                continue;
            }
            bytes += o.bytes.len();
            let m = Message::decode(&o.bytes).expect("decodes");
            inbox.add(vec![m]);
        }
        max_tick = max_tick.max(bytes);
        if sleep {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(inbox.data.len(), 64);
    assert!(max_tick <= 2000 + 8000, "{max_tick}");
    // Full mode: unmodified sections come Explicit, as generated.
    let d = &inbox.data[0];
    assert_eq!(d.form, LodForm::Explicit);
    let generated = flat_lod(LodCoord::new(
        i32::from(d.level),
        d.section[0],
        d.section[1],
        d.section[2],
    ));
    assert_eq!(d.cells, generated);
}

#[test]
fn explicit_sections_carry_their_modified_neighbours_borders_in_the_apron() {
    let mut lod = LodPropagation::new(flat_generator(), 0);
    let stone = stone();
    // Two chunks in neighbouring level-1 sections along x.
    let (left, right) = (ChunkCoord::new(-1, GC, 0), ChunkCoord::new(0, GC, 0));
    assert_ne!(above(left, 1), above(right, 1));
    lod.mark_chunk(left);
    lod.mark_chunk(right);
    lod.drain(&[], &mut |c: ChunkCoord| {
        (c == left || c == right).then(|| stone.voxels().to_vec())
    });
    let cells = lod.cells_for_client(above(right, 1));
    // The right section's −x apron is the left section's stone border; a generated apron would
    // be air there (above the ground).
    let y = ((i64::from(right.y) * 32 - section_origin(above(right, 1)).y) / 2) as i32;
    assert_eq!(cells[cell_index(-1, y, 0)], M.stone);
    let generated = flat_lod(above(right, 1));
    assert_eq!(generated[cell_index(-1, y, 0)], M.air);
}

// ---- lod: builds from afar ----

#[test]
fn a_large_build_is_in_the_sections_drawn_from_50_km_away_and_from_altitude() {
    // The level the client draws at distance d (LOD_PIXEL_ERROR 4 px, 1080 px at 75°: ~703 px
    // per radian): the coarsest whose cells still project within 4 px.
    let drawn_level = |d: f64| {
        let mut level = 1;
        while (2f64.powi(level + 1) / d) * 703.0 <= 4.0 {
            level += 1;
        }
        level
    };
    let (at_50km, at_100km) = (drawn_level(50_000.0), drawn_level(100_000.0));
    assert_eq!(at_50km, 8);
    assert_eq!(at_100km, 9);
    // A wall 512 m wide, 512 m tall and 1 m thick floating 512 m over the flat ground (x = 1000,
    // z 0..511): coarse cells near the ground hold the ground too (Phase 16: it lies at 0..32).
    const X: i32 = 1000;
    let mut built: BTreeMap<ChunkCoord, Chunk> = BTreeMap::new();
    for cy in 16..32 {
        for cz in 0..16 {
            let coord = ChunkCoord::new(X / 32, cy, cz);
            let mut chunk = Chunk::default();
            generate_flat_chunk(coord, &mut chunk);
            for y in 0..32 {
                for z in 0..32 {
                    chunk.set(X % 32, y, z, M.stone);
                }
            }
            built.insert(coord, chunk);
        }
    }
    let mut lod = LodPropagation::new(flat_generator(), 0);
    for c in built.keys() {
        lod.mark_chunk(*c);
    }
    lod.drain(&[], &mut |c: ChunkCoord| {
        built.get(&c).map(|chunk| chunk.voxels().to_vec())
    });
    for level in [at_50km, at_100km] {
        // The section holding the wall's middle, 768 m up.
        let (s, cell) = (section_size(level), cell_size(level));
        let c = LodCoord::new(
            level,
            ((i64::from(X) - ORIGIN_X) / s) as i32,
            ((768 - ORIGIN_Y) / s) as i32,
            ((256 - ORIGIN_Z) / s) as i32,
        );
        let cells = lod.cells_for_client(c);
        assert_eq!(cells.len(), VOLUME, "level {level}");
        let generated = flat_lod(c);
        let o = section_origin(c);
        let at = |x: i64, y: i64, z: i64| {
            cell_index(
                ((x - o.x) / cell) as i32,
                ((y - o.y) / cell) as i32,
                ((z - o.z) / cell) as i32,
            )
        };
        let x = i64::from(X);
        assert_eq!(generated[at(x, 768, 256)], M.air, "level {level}");
        assert_eq!(cells[at(x, 768, 256)], M.stone, "level {level}"); // the wall, in mid air above ground
        assert_eq!(cells[at(x + 4 * cell, 768, 256)], M.air, "level {level}");
    }
}
