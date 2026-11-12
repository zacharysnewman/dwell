//! The streaming report (docs/DEV_TOOLS.md, BUGS.md #54): the near ring's chunks and what each
//! waits on, the job pool's queues, the streamer, the LOD and the last seconds' frames, as one
//! JSON object the shell completes (device, settings, world) and copies for a bug report.

use super::{Game, Scene};
use crate::telemetry;
use dwell_world::{ChunkCoord, MAX_CHUNK_Y, MIN_CHUNK_Y};
use serde_json::{Value, json};

/// The near ring's half-width (chunks): every chunk within ±this of the player's chunk.
pub const NEAR_RING_CHUNKS: i32 = 3;
/// How long the server's unloads are kept for the report (ms).
pub const UNLOADS_KEPT_MS: f64 = 30_000.0;
/// At most this many unloads are listed (the nearest); the rest are counted.
const UNLOADS_LISTED: usize = 64;
/// Unloads nearer than this (m) are flagged: the server took a chunk the player stands by.
pub const NEAR_UNLOAD_M: f64 = 48.0;
/// The frame window summarised (ms).
const WINDOW_MS: f64 = 10_000.0;

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

impl<S: Scene> Game<S> {
    /// Notes the server's unload of `coords` at `now_ms`, with each one's distance from the feet.
    pub(super) fn note_unloads(&mut self, coords: &[[i32; 3]], now_ms: f64) {
        let feet = self.current.feet();
        for &c in coords {
            let m = dwell_world::chunk_centre_m(ChunkCoord::from_array(c));
            let d = (0..3).map(|i| (m[i] - feet[i]).powi(2)).sum::<f64>().sqrt();
            self.unloads.push_back((now_ms, c, d));
        }
        while self
            .unloads
            .front()
            .is_some_and(|u| now_ms - u.0 > UNLOADS_KEPT_MS)
        {
            self.unloads.pop_front();
        }
    }

    /// The report at `now_ms` (`telemetry::now_ms`'s clock): see the module's doc.
    pub fn streaming_report(&self, now_ms: f64) -> Value {
        let c = &self.current;
        let feet = c.feet();
        let speed = c.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        let me = dwell_world::chunk_of_pos(feet);
        let pool = self.pool.borrow();
        let mut ring = Vec::new();
        let (mut needed, mut drawn) = (0, 0);
        for dy in -NEAR_RING_CHUNKS..=NEAR_RING_CHUNKS {
            for dz in -NEAR_RING_CHUNKS..=NEAR_RING_CHUNKS {
                for dx in -NEAR_RING_CHUNKS..=NEAR_RING_CHUNKS {
                    let k = [me.x + dx, me.y + dy, me.z + dz];
                    if !(MIN_CHUNK_Y..=MAX_CHUNK_Y).contains(&k[1]) {
                        continue;
                    }
                    let cc = ChunkCoord::from_array(k);
                    // The world's own volume (a big world's reaches past the build's 50 km).
                    let in_volume = self.world_bounds.volume().chunk_may_hold(cc);
                    let s = self.streamer.chunk_status(k);
                    let m = dwell_world::chunk_centre_m(cc);
                    let d = (0..3).map(|i| (m[i] - feet[i]).powi(2)).sum::<f64>().sqrt();
                    let full = self.chunk_detail(cc) == 0;
                    let section = self.lod.as_ref().map(|l| l.section_report(cc));
                    if in_volume {
                        needed += 1;
                        drawn += usize::from(full);
                    }
                    let flags: Vec<&str> = [
                        (s.loaded, "loaded"),
                        (s.air, "air"),
                        (s.generating, "generating"),
                        (s.lit, "lit"),
                        (s.meshed, "meshed"),
                        (s.meshing, "meshing"),
                        (s.dirty, "dirty"),
                        (s.drawable, "drawable"),
                        (full, "drawn"),
                        (s.settling, "settling"),
                        (!in_volume, "outsideVolume"),
                    ]
                    .into_iter()
                    .filter_map(|(on, name)| on.then_some(name))
                    .collect();
                    let mut e = json!({ "c": k, "d": round(d), "flags": flags });
                    if let Some(l) = section {
                        e["lod"] = json!({
                            "section": l.section,
                            "known": l.known,
                            "kind": l.kind,
                            "needsAnswer": l.needs_answer,
                            "generating": l.generating,
                            "meshing": l.meshing,
                            "meshed": l.meshed,
                            "showsChunks": l.shows_chunks,
                            "forced": l.forced,
                            "showsNothing": l.shows_nothing,
                        });
                    }
                    if !full && in_volume {
                        e["waits"] = json!({
                            "reasons": wait_reasons(&s, section.as_ref()),
                            "neighboursGeneratingOrUnlit": s.waits_on,
                            "neighboursMissing": s.missing,
                            "dirtyPosition": s.dirty_position,
                            "genJob": pool.chunk_job(k, false).map(job_position),
                            "meshJob": pool.chunk_job(k, true).map(job_position),
                        });
                    }
                    ring.push(e);
                }
            }
        }
        let stats = self.streamer.stats();
        let unloads: Vec<_> = self
            .unloads
            .iter()
            .filter(|u| now_ms - u.0 <= UNLOADS_KEPT_MS)
            .collect();
        let near_unloads = unloads.iter().filter(|u| u.2 < NEAR_UNLOAD_M).count();
        let mut listed = unloads.clone();
        listed.sort_by(|a, b| a.2.total_cmp(&b.2));
        listed.truncate(UNLOADS_LISTED);
        let lod = self.lod.as_ref().map(|l| {
            let st = l.debug_stats();
            let sel = l.last_selection();
            json!({
                "drawnByLevel": st.drawn,
                "chunkSections": st.chunk_sections,
                "nodes": st.nodes,
                "generating": st.generating,
                "meshing": st.meshing,
                "asking": st.asking,
                "cacheBytes": st.cache_bytes,
                "cacheBudget": l.cache_budget(),
                "inUseBytes": st.in_use_bytes,
                "errorScale": round(st.error_scale),
                "waiting": st.waiting,
                "wanted": st.wanted,
                "built": st.built,
                "evicted": st.evicted,
                "selection": {
                    "drawn": sel.drawn.len(),
                    "empty": sel.empty.len(),
                    "chunks": sel.chunks.len(),
                    "standIns": sel.stand_ins.len(),
                },
                "selectionAgeMs": l.selected_ms().map(|t| round(now_ms - t)),
                "selections": l.selections().0,
            })
        });
        json!({
            "player": {
                "feet": feet.map(round),
                "velocity": c.velocity.map(round),
                "speed": round(speed),
                "face": if c.face.sign() > 0 { "A" } else { "B" },
                "flying": c.state == super::PlayerState::Flying,
                "chunk": me.to_array(),
            },
            "nearRing": {
                "radiusChunks": NEAR_RING_CHUNKS,
                "needed": needed,
                "drawnFull": drawn,
                "chunks": ring,
            },
            "pool": pool.report(now_ms),
            "streamer": {
                "loaded": stats.loaded,
                "generating": stats.generating,
                "meshed": stats.meshed,
                "dirty": stats.dirty,
                "meshing": stats.meshing,
                "unlit": self.streamer.unlit_len(),
                "settling": self.streamer.settling_len(),
                "resyncs": stats.resyncs,
                "terrainReady": self.terrain_ready(),
                "viewRadiusChunks": dwell_protocol::world::VIEW_RADIUS_CHUNKS,
                "renderRadiusChunks": dwell_protocol::world::RENDER_RADIUS_CHUNKS,
                "unloads30s": unloads.len(),
                "unloadsWithin48m": near_unloads,
                "unloadsNearest": listed
                    .iter()
                    .map(|u| json!({ "c": u.1, "d": round(u.2), "agoMs": round(now_ms - u.0) }))
                    .collect::<Vec<_>>(),
            },
            "lod": lod,
            "frames": frame_window(now_ms),
            "creatures": self.creatures.len(),
        })
    }
}

/// A pool job's place: its queue position, or "running".
fn job_position(p: Option<usize>) -> Value {
    p.map_or_else(|| json!("running"), |at| json!(at))
}

/// Why a chunk is not drawn at full detail, most decisive first.
pub fn wait_reasons(
    s: &dwell_game::chunk_stream::ChunkStatus,
    lod: Option<&dwell_lod::SectionReport>,
) -> Vec<&'static str> {
    let mut r = Vec::new();
    if !s.loaded {
        r.push(if s.generating {
            "generating"
        } else {
            "notLoaded"
        });
    }
    if s.loaded && !s.lit {
        r.push("unlit");
    }
    if !s.waits_on.is_empty() {
        r.push("neighbourGeneratingOrUnlit");
    }
    if s.meshing {
        r.push("meshing");
    }
    if s.dirty {
        r.push("dirty");
    }
    if s.loaded && !s.drawable && !s.dirty && !s.meshing {
        r.push("notMeshed");
    }
    if s.drawable
        && let Some(l) = lod
        && !l.shows_chunks
    {
        r.push(if l.shows_nothing {
            "lodShowsNothing"
        } else {
            "lodNotShowingChunks"
        });
    }
    r
}

/// The last [`WINDOW_MS`] of frames: the interval's p50/p95/max, frames a second, uploads a
/// second and their time, jobs applied a second.
fn frame_window(now_ms: f64) -> Value {
    telemetry::with(|t| {
        let names = t.names().to_vec();
        let col = |n: &str| names.iter().position(|x| *x == n);
        let frames: Vec<_> = t
            .frames()
            .filter(|f| now_ms - f.at_ms <= WINDOW_MS)
            .cloned()
            .collect();
        let gaps: Vec<f64> = frames.windows(2).map(|w| w[1].at_ms - w[0].at_ms).collect();
        let span_s = match (frames.first(), frames.last()) {
            (Some(a), Some(b)) if b.at_ms > a.at_ms => (b.at_ms - a.at_ms) / 1000.0,
            _ => 0.0,
        };
        let sum = |n: &str| {
            col(n).map_or(0.0, |i| {
                frames
                    .iter()
                    .map(|f| f.values.get(i).copied().unwrap_or(0.0))
                    .sum()
            })
        };
        let per_s = |v: f64| if span_s > 0.0 { round(v / span_s) } else { 0.0 };
        let st = telemetry::Stats::of(&gaps);
        json!({
            "windowS": round(span_s),
            "frameMs": { "p50": round(st.p50), "p95": round(st.p95), "max": round(st.max) },
            "framesPerS": per_s(gaps.len() as f64),
            "chunkUploadsPerS": per_s(sum("upload.chunks")),
            "chunkUploadMs": round(sum("upload.chunk")),
            "sectionUploadsPerS": per_s(sum("upload.sections")),
            "sectionUploadMs": round(sum("upload.section")),
            "jobsAppliedPerS": per_s(sum("jobs.done")),
        })
    })
}
