//! Smooth LOD transitions (ARCHITECTURE.md §6.6 "Swaps"): the swap state machine. Each frame the
//! selection's leaves that draw — sections, level-1 sections shown as their chunks, stand-ins —
//! are *items*, each covering its section's box. When the set changes, the changes that overlap
//! (one's box inside the other's: a parent and its children, a level-1 section and its chunks) form
//! a *swap*: for `fade_ms` the outgoing items keep drawing as [`Fade::Out`] and the incoming draw
//! as [`Fade::In`], with the same progress `t`. The renderer keeps a pixel of an incoming draw
//! where its screen-space threshold is under `t` and of an outgoing one where it is not
//! ([`Fade::keeps`]), so every pixel shows exactly one level, and depth, fog and opacity are those
//! of an ordinary opaque draw.
//!
//! One progress per region keeps that exact. A change that overlaps a swap still fading joins it
//! (a level refining again mid-fade, the view's first seconds coarse to fine): the swaps become
//! one at the furthest of their progresses, the change's incoming items fading in and its outgoing
//! ones out with the rest, and an item that was fading in and leaves again just goes (its pixels
//! were the incoming side's). A change that is a swap exactly undone (a refine coarsened again, or
//! back) reverses it from where it is. Only a change that brings back an item fading out without
//! undoing its swap (rare) finishes the swaps it overlaps first. A change with no outgoing item
//! (something appears where nothing drew: the view's first frame, a section of the other face)
//! shows at once; one with no incoming item (the finer level is empty there) fades out to nothing.
//! With `fade_ms` 0 every change is immediate: the swaps of before.

use dwell_world::hash::{HashMap, HashSet};
use dwell_world::lod::{LodCoord, MAX_LEVEL, parent};

use crate::grid::{LodId, id_of};

/// How much of a draw shows this frame, by its per-pixel threshold `n` in [0, 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fade {
    /// Every pixel (not fading).
    Full,
    /// Fading in, `t` of the way: the pixels whose threshold is under `t`.
    In(f32),
    /// Fading out, `t` of the way: the pixels whose threshold is `t` or more.
    Out(f32),
}

impl Fade {
    /// Whether the pixel with threshold `n` shows (the shaders' `fade_discard` keeps the same).
    pub fn keeps(self, n: f32) -> bool {
        match self {
            Self::Full => true,
            Self::In(t) => n < t,
            Self::Out(t) => n >= t,
        }
    }

    /// The draw's fade as the shaders read it (`Draw.clip_hi.w`): 0 full, 1 + t fading in,
    /// −(1 + t) fading out.
    pub fn shader_value(self) -> f32 {
        match self {
            Self::Full => 0.0,
            Self::In(t) => 1.0 + t,
            Self::Out(t) => -(1.0 + t),
        }
    }

    /// Fading out: its pixels take no ink outline (the edges are found in the incoming level).
    pub fn is_out(self) -> bool {
        matches!(self, Self::Out(_))
    }

    /// Draws nothing at all (fading in, not yet begun).
    pub fn is_hidden(self) -> bool {
        matches!(self, Self::In(t) if t <= 0.0)
    }
}

/// What an item is: which draw covers its section's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
    /// The section's own mesh (with its skirts).
    Section,
    /// A level-1 section's 8 streamed chunks.
    Chunks,
    /// An ancestor's mesh clipped to the section's box (`from`).
    StandIn,
}

/// An item of a frame's selection, covering `area`'s box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item {
    pub kind: ItemKind,
    pub area: LodCoord,
    /// Sections: the sides whose skirt shows. Stand-ins: unused.
    pub mask: u8,
    /// Stand-ins: the ancestor whose mesh is drawn.
    pub from: LodId,
}

impl Item {
    pub fn section(area: LodCoord, mask: u8) -> Self {
        Self {
            kind: ItemKind::Section,
            area,
            mask,
            from: 0,
        }
    }
    pub fn chunks(area: LodCoord) -> Self {
        Self {
            kind: ItemKind::Chunks,
            area,
            mask: 0,
            from: 0,
        }
    }
    pub fn stand_in(area: LodCoord, from: LodId) -> Self {
        Self {
            kind: ItemKind::StandIn,
            area,
            mask: 0,
            from,
        }
    }
    fn key(&self) -> Key {
        (self.kind, id_of(self.area))
    }
}

type Key = (ItemKind, LodId);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Steady,
    In(u32),
    Out(u32),
}

#[derive(Clone, Debug)]
struct Entry {
    item: Item,
    phase: Phase,
    /// The last update whose selection held it.
    seen: u64,
}

#[derive(Clone, Debug)]
struct Swap {
    start_ms: f64,
    ins: Vec<Key>,
    outs: Vec<Key>,
}

/// Counts for the F3 line and the bench.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FadeStats {
    /// Swaps fading now, and the items they draw on top of the selection (the outgoing ones).
    pub swaps: usize,
    pub retiring: usize,
    /// Swaps begun, changes joined to swaps fading, swaps finished early (a change brought back
    /// an item fading out) and swaps reversed, since the start.
    pub begun: u64,
    pub merged: u64,
    pub cut_short: u64,
    pub reversed: u64,
}

/// The swap state machine (module docs).
#[derive(Clone, Debug, Default)]
pub struct SwapFade {
    fade_ms: f64,
    entries: HashMap<Key, Entry>,
    swaps: HashMap<u32, Swap>,
    next: u32,
    frame: u64,
    stats: FadeStats,
}

/// The box `c` and every box holding it, smallest first.
fn with_ancestors(c: LodCoord) -> impl Iterator<Item = LodId> {
    let mut at = Some(c);
    std::iter::from_fn(move || {
        let c = at?;
        at = (c.level < MAX_LEVEL).then(|| parent(c));
        Some(id_of(c))
    })
}

fn find(up: &mut [usize], mut i: usize) -> usize {
    while up[i] != i {
        up[i] = up[up[i]];
        i = up[i];
    }
    i
}

impl SwapFade {
    pub fn new(fade_ms: f64) -> Self {
        Self {
            fade_ms: fade_ms.max(0.0),
            ..Self::default()
        }
    }

    pub fn fade_ms(&self) -> f64 {
        self.fade_ms
    }

    /// A new fade time; swaps under way finish at once.
    pub fn set_fade_ms(&mut self, fade_ms: f64) {
        let ids: Vec<u32> = self.swaps.keys().copied().collect();
        for g in ids {
            self.finish(g);
        }
        self.fade_ms = fade_ms.max(0.0);
    }

    pub fn stats(&self) -> FadeStats {
        FadeStats {
            swaps: self.swaps.len(),
            retiring: self
                .entries
                .values()
                .filter(|e| matches!(e.phase, Phase::Out(_)))
                .count(),
            ..self.stats
        }
    }

    fn progress(&self, g: u32, now_ms: f64) -> f32 {
        let s = &self.swaps[&g];
        if self.fade_ms <= 0.0 {
            return 1.0;
        }
        ((now_ms - s.start_ms) / self.fade_ms).clamp(0.0, 1.0) as f32
    }

    /// Ends swap `g` now: its outgoing items go, its incoming draw in full.
    fn finish(&mut self, g: u32) {
        let Some(s) = self.swaps.remove(&g) else {
            return;
        };
        for k in s.outs {
            self.entries.remove(&k);
        }
        for k in s.ins {
            if let Some(e) = self.entries.get_mut(&k) {
                e.phase = Phase::Steady;
            }
        }
    }

    /// The frame's selection (`selected`: each drawn leaf once) at `now_ms`.
    pub fn update(&mut self, selected: &[Item], now_ms: f64) {
        // Swaps whose time is up are done.
        let done: Vec<u32> = self
            .swaps
            .iter()
            .filter(|(_, s)| now_ms - s.start_ms >= self.fade_ms)
            .map(|(&g, _)| g)
            .collect();
        for g in done {
            self.finish(g);
        }
        // What changed since the last frame's selection (the items drawn steady or fading in).
        // (One lookup per selected item: this runs for every drawn leaf, every frame.)
        self.frame += 1;
        let frame = self.frame;
        let mut changes: Vec<(Item, bool)> = Vec::new(); // (item, incoming)
        for it in selected {
            match self.entries.get_mut(&it.key()) {
                Some(e) if !matches!(e.phase, Phase::Out(_)) => {
                    e.item = *it;
                    e.seen = frame;
                }
                _ => changes.push((*it, true)),
            }
        }
        for e in self.entries.values() {
            if !matches!(e.phase, Phase::Out(_)) && e.seen != frame {
                changes.push((e.item, false));
            }
        }
        if changes.is_empty() {
            return;
        }
        if self.fade_ms <= 0.0 {
            for (it, incoming) in changes {
                if incoming {
                    self.entries.insert(
                        it.key(),
                        Entry {
                            item: it,
                            phase: Phase::Steady,
                            seen: 0,
                        },
                    );
                } else {
                    self.entries.remove(&it.key());
                }
            }
            return;
        }
        // Changes that overlap are one swap (union-find over the boxes holding each other).
        let mut at: HashMap<LodId, Vec<usize>> = HashMap::default();
        for (i, (it, _)) in changes.iter().enumerate() {
            at.entry(id_of(it.area)).or_default().push(i);
        }
        let mut up: Vec<usize> = (0..changes.len()).collect();
        for (i, (it, _)) in changes.iter().enumerate() {
            for a in with_ancestors(it.area) {
                if let Some(js) = at.get(&a) {
                    for &j in js {
                        let (ri, rj) = (find(&mut up, i), find(&mut up, j));
                        if ri != rj {
                            up[ri] = rj;
                        }
                    }
                }
            }
        }
        let mut clusters: HashMap<usize, Vec<usize>> = HashMap::default();
        for i in 0..changes.len() {
            let r = find(&mut up, i);
            clusters.entry(r).or_default().push(i);
        }
        // The swaps fading now, by the boxes of their items (exactly, and every box above them).
        let mut fading_at: HashMap<LodId, Vec<u32>> = HashMap::default();
        let mut fading_below: HashMap<LodId, Vec<u32>> = HashMap::default();
        for (g, s) in &self.swaps {
            for k in s.ins.iter().chain(&s.outs) {
                let Some(e) = self.entries.get(k) else {
                    continue;
                };
                fading_at.entry(id_of(e.item.area)).or_default().push(*g);
                for a in with_ancestors(e.item.area) {
                    fading_below.entry(a).or_default().push(*g);
                }
            }
        }
        let mut order: Vec<Vec<usize>> = clusters.into_values().collect();
        order.sort_unstable_by_key(|c| c[0]);
        for members in order {
            let mut touched: Vec<u32> = Vec::new();
            for &i in &members {
                let area = changes[i].0.area;
                let own = id_of(area);
                touched.extend(fading_below.get(&own).into_iter().flatten());
                for a in with_ancestors(area).skip(1) {
                    touched.extend(fading_at.get(&a).into_iter().flatten());
                }
            }
            touched.sort_unstable();
            touched.dedup();
            touched.retain(|g| self.swaps.contains_key(g));
            let mut added: Vec<Item> = Vec::new();
            let mut removed: Vec<Item> = Vec::new();
            for &i in &members {
                let (it, incoming) = changes[i];
                if incoming {
                    added.push(it);
                } else {
                    removed.push(it);
                }
            }
            if let [g] = touched[..]
                && self.is_undone(g, &added, &removed)
            {
                self.reverse(g, &added, now_ms);
                continue;
            }
            let revived = added.iter().any(|it| {
                self.entries
                    .get(&it.key())
                    .is_some_and(|e| matches!(e.phase, Phase::Out(_)))
            });
            if !touched.is_empty() && !revived {
                self.merge(&touched, added, removed, now_ms);
                continue;
            }
            for g in touched {
                self.stats.cut_short += 1;
                self.finish(g);
            }
            if added.is_empty() && removed.is_empty() {
                continue;
            }
            if removed.is_empty() {
                for it in added {
                    self.entries.insert(
                        it.key(),
                        Entry {
                            item: it,
                            phase: Phase::Steady,
                            seen: 0,
                        },
                    );
                }
                continue;
            }
            let g = self.next;
            self.next = self.next.wrapping_add(1);
            self.stats.begun += 1;
            let mut swap = Swap {
                start_ms: now_ms,
                ins: Vec::with_capacity(added.len()),
                outs: Vec::with_capacity(removed.len()),
            };
            for it in removed {
                let k = it.key();
                swap.outs.push(k);
                self.entries.insert(
                    k,
                    Entry {
                        item: it,
                        phase: Phase::Out(g),
                        seen: 0,
                    },
                );
            }
            for it in added {
                let k = it.key();
                swap.ins.push(k);
                self.entries.insert(
                    k,
                    Entry {
                        item: it,
                        phase: Phase::In(g),
                        seen: 0,
                    },
                );
            }
            self.swaps.insert(g, swap);
        }
    }

    /// Folds a change into the swaps it overlaps (`touched`), as one swap at the furthest of their
    /// progresses: its incoming items join fading in, its outgoing ones fading out, and an item
    /// that was fading in and leaves again just goes (the pixels it had, under `t`, are the
    /// incoming level's). Every item of the swap keeps the threshold rule with the same `t`, so
    /// each pixel still shows exactly one level; what jumps is at most the share a swap's
    /// progress moved up to the furthest.
    fn merge(&mut self, touched: &[u32], added: Vec<Item>, removed: Vec<Item>, now_ms: f64) {
        self.stats.merged += 1;
        let t = touched
            .iter()
            .map(|&g| f64::from(self.progress(g, now_ms)))
            .fold(0.0, f64::max);
        let g0 = touched[0];
        let mut ins = Vec::new();
        let mut outs = Vec::new();
        for &g in touched {
            let s = self.swaps.remove(&g).expect("swap");
            ins.extend(s.ins);
            outs.extend(s.outs);
        }
        for k in &ins {
            if let Some(e) = self.entries.get_mut(k) {
                e.phase = Phase::In(g0);
            }
        }
        for k in &outs {
            if let Some(e) = self.entries.get_mut(k) {
                e.phase = Phase::Out(g0);
            }
        }
        let mut gone: HashSet<Key> = HashSet::default();
        for it in removed {
            let k = it.key();
            match self.entries.get(&k).map(|e| e.phase) {
                Some(Phase::In(_)) => {
                    self.entries.remove(&k);
                    gone.insert(k);
                }
                _ => {
                    outs.push(k);
                    self.entries.insert(
                        k,
                        Entry {
                            item: it,
                            phase: Phase::Out(g0),
                            seen: 0,
                        },
                    );
                }
            }
        }
        ins.retain(|k| !gone.contains(k));
        for it in added {
            let k = it.key();
            ins.push(k);
            self.entries.insert(
                k,
                Entry {
                    item: it,
                    phase: Phase::In(g0),
                    seen: 0,
                },
            );
        }
        self.swaps.insert(
            g0,
            Swap {
                start_ms: now_ms - t * self.fade_ms,
                ins,
                outs,
            },
        );
    }

    /// Whether a change is exactly swap `g` undone: what it brought in leaves and what it took out
    /// comes back.
    fn is_undone(&self, g: u32, added: &[Item], removed: &[Item]) -> bool {
        let s = &self.swaps[&g];
        let same = |keys: &[Key], items: &[Item]| {
            keys.len() == items.len() && {
                let set: HashSet<Key> = items.iter().map(Item::key).collect();
                keys.iter().all(|k| set.contains(k))
            }
        };
        same(&s.ins, removed) && same(&s.outs, added)
    }

    /// Runs swap `g` backwards from where it is: each item keeps the share of pixels it has.
    fn reverse(&mut self, g: u32, added: &[Item], now_ms: f64) {
        self.stats.reversed += 1;
        let t = f64::from(self.progress(g, now_ms));
        let s = self.swaps.get_mut(&g).expect("swap");
        std::mem::swap(&mut s.ins, &mut s.outs);
        s.start_ms = now_ms - (1.0 - t) * self.fade_ms;
        let (ins, outs) = (s.ins.clone(), s.outs.clone());
        for k in ins {
            if let Some(e) = self.entries.get_mut(&k) {
                e.phase = Phase::In(g);
            }
        }
        for k in outs {
            if let Some(e) = self.entries.get_mut(&k) {
                e.phase = Phase::Out(g);
            }
        }
        for it in added {
            if let Some(e) = self.entries.get_mut(&it.key()) {
                e.item = *it;
            }
        }
    }

    /// Every item drawn this frame with its fade (in no particular order).
    pub fn shown(&self, now_ms: f64) -> impl Iterator<Item = (Item, Fade)> + '_ {
        self.entries.values().map(move |e| {
            let fade = match e.phase {
                Phase::Steady => Fade::Full,
                Phase::In(g) => Fade::In(self.progress(g, now_ms)),
                Phase::Out(g) => Fade::Out(self.progress(g, now_ms)),
            };
            (e.item, fade)
        })
    }

    /// Items still drawing that left the selection (their meshes must stay in the cache).
    pub fn retiring(&self) -> impl Iterator<Item = &Item> + '_ {
        self.entries
            .values()
            .filter(|e| matches!(e.phase, Phase::Out(_)))
            .map(|e| &e.item)
    }

    /// Drops everything (the view starts over).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.swaps.clear();
    }
}

#[cfg(test)]
mod tests;
