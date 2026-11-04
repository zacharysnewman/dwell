//! The basins saved with the world (docs/WATER_FLOW.md §8): every basin but the reservoirs (found
//! again when an edit meets the sea), with the flow network, the water in transit and the ways
//! drawn running, and the revisions of the chunks each basin lies in. A basin whose chunks a build
//! that doesn't save the basins changed since (another revision on load) is left out: it is found
//! again, at rest as it then is, by the next edit that touches it.

use super::*;
use dwell_world::ChunkCoord;

const MAGIC: &[u8; 4] = b"DWW1";

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
    fn cell(&mut self, c: [i32; 3]) {
        c.into_iter().for_each(|v| self.i32(v));
    }
    fn cells<'a>(&mut self, cells: impl ExactSizeIterator<Item = &'a [i32; 3]>) {
        self.len(cells.len());
        for &c in cells {
            self.cell(c);
        }
    }
    /// A set's cells in order (the same basins save the same bytes).
    fn set(&mut self, set: &HashSet<[i32; 3]>) {
        let mut v: Vec<&[i32; 3]> = set.iter().collect();
        v.sort_unstable();
        self.cells(v.into_iter());
    }
    fn weighed(&mut self, v: &[([i32; 3], f64)]) {
        self.len(v.len());
        for &(c, q) in v {
            self.cell(c);
            self.f64(q);
        }
    }
    /// A state by its canonical name (a registry change keeps it, as the chunks' names do).
    fn state(&mut self, m: StateId) {
        let name = dwell_blocks::try_info(m).map_or("dwell:air", |s| s.name);
        self.len(name.len());
        self.0.extend_from_slice(name.as_bytes());
    }
}

struct In<'a> {
    b: &'a [u8],
    at: usize,
}

impl In<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    /// A count, no more than the bytes left could hold (a cut record fails, not allocates).
    fn len(&mut self) -> Option<usize> {
        let n = self.u32()? as usize;
        (n <= self.b.len() - self.at).then_some(n)
    }
    fn cell(&mut self) -> Option<[i32; 3]> {
        Some([self.i32()?, self.i32()?, self.i32()?])
    }
    fn cells(&mut self) -> Option<Vec<[i32; 3]>> {
        (0..self.len()?).map(|_| self.cell()).collect()
    }
    fn set(&mut self) -> Option<HashSet<[i32; 3]>> {
        Some(self.cells()?.into_iter().collect())
    }
    fn weighed(&mut self) -> Option<Vec<([i32; 3], f64)>> {
        (0..self.len()?)
            .map(|_| Some((self.cell()?, self.f64()?)))
            .collect()
    }
    /// A state by name: `None` (outer `Some`) when this build doesn't know it.
    fn state(&mut self) -> Option<Option<StateId>> {
        let n = self.len()?;
        let name = std::str::from_utf8(self.take(n)?).ok()?;
        Some(dwell_blocks::parse_state(name).ok().filter(|&m| m != 0))
    }
}

fn put_outlet(o: &mut Out, x: &Outlet) {
    o.cell(x.from);
    match x.kind {
        OutKind::Face { lo, hi } => {
            o.u8(0);
            o.f64(lo);
            o.f64(hi);
        }
        OutKind::Floor { z } => {
            o.u8(1);
            o.f64(z);
        }
        OutKind::Leak { k, centre } => {
            o.u8(2);
            o.f64(k);
            o.f64(centre);
        }
        OutKind::Seep { sill } => {
            o.u8(3);
            o.f64(sill);
        }
    }
    match x.target {
        Target::Unresolved => o.u8(0),
        Target::Basin(t) => {
            o.u8(1);
            o.u32(t);
        }
        Target::Lost => o.u8(2),
    }
    o.f64(x.flow);
    o.f64(x.scale);
    o.u8(u8::from(x.contact));
}

fn get_outlet(i: &mut In<'_>) -> Option<Outlet> {
    let from = i.cell()?;
    let kind = match i.u8()? {
        0 => OutKind::Face {
            lo: i.f64()?,
            hi: i.f64()?,
        },
        1 => OutKind::Floor { z: i.f64()? },
        2 => OutKind::Leak {
            k: i.f64()?,
            centre: i.f64()?,
        },
        3 => OutKind::Seep { sill: i.f64()? },
        _ => return None,
    };
    let target = match i.u8()? {
        0 => Target::Unresolved,
        1 => Target::Basin(i.u32()?),
        2 => Target::Lost,
        _ => return None,
    };
    Some(Outlet {
        from,
        kind,
        target,
        flow: i.f64()?,
        scale: i.f64()?,
        contact: i.bool()?,
    })
}

fn put_basin(o: &mut Out, b: &Basin, revision: &dyn Fn(ChunkCoord) -> u32) {
    o.u32(b.id);
    o.state(b.liquid);
    o.i32(b.bottom);
    o.len(b.rows.len());
    for r in &b.rows {
        o.cells(r.iter());
    }
    o.f64(b.volume);
    o.weighed(&b.inflows);
    o.f64(b.last_volume);
    o.len(b.outlets.len());
    for x in &b.outlets {
        put_outlet(o, x);
    }
    o.set(&b.wet);
    o.u8(u8::from(b.capped));
    o.cell(b.lo);
    o.cell(b.hi);
    for v in [b.arriving, b.through, b.extra, b.linked, b.owed] {
        o.f64(v);
    }
    o.set(&b.rest);
    o.set(&b.held);
    let mut natural: Vec<([i32; 3], f64)> = b.natural.iter().map(|(&c, &w)| (c, w)).collect();
    natural.sort_unstable_by_key(|&(c, _)| c);
    o.weighed(&natural);
    match &b.steady {
        None => o.u8(0),
        Some(st) => {
            o.u8(1);
            o.f64(st.through);
            o.f64(st.law);
            o.f64(st.level);
            o.weighed(&st.feeds);
        }
    }
    o.len(b.chunks.len());
    for &c in &b.chunks {
        o.i32(c.x);
        o.i32(c.y);
        o.i32(c.z);
        o.u32(revision(c));
    }
}

/// A basin read back, and whether its chunks are at the revisions they were saved at; `None` for
/// a record cut short; a basin of a liquid this build doesn't know reads as stale.
fn get_basin(i: &mut In<'_>, revision: &dyn Fn(ChunkCoord) -> u32) -> Option<(Basin, bool)> {
    let id = i.u32()?;
    let liquid = i.state()?;
    let bottom = i.i32()?;
    let rows: Vec<Vec<[i32; 3]>> = (0..i.len()?).map(|_| i.cells()).collect::<Option<_>>()?;
    let volume = i.f64()?;
    let inflows = i.weighed()?;
    let last_volume = i.f64()?;
    let outlets: Vec<Outlet> = (0..i.len()?)
        .map(|_| get_outlet(i))
        .collect::<Option<_>>()?;
    let wet = i.set()?;
    let capped = i.bool()?;
    let lo = i.cell()?;
    let hi = i.cell()?;
    let [arriving, through, extra, linked, owed] =
        [i.f64()?, i.f64()?, i.f64()?, i.f64()?, i.f64()?];
    let rest = i.set()?;
    let held = i.set()?;
    let natural: HashMap<[i32; 3], f64> = i.weighed()?.into_iter().collect();
    let steady = match i.u8()? {
        0 => None,
        1 => Some(Steady {
            through: i.f64()?,
            law: i.f64()?,
            level: i.f64()?,
            feeds: i.weighed()?,
        }),
        _ => return None,
    };
    let mut fresh = true;
    let mut chunks = Vec::new();
    for _ in 0..i.len()? {
        let c = ChunkCoord {
            x: i.i32()?,
            y: i.i32()?,
            z: i.i32()?,
        };
        fresh &= revision(c) == i.u32()?;
        chunks.push(c);
    }
    let (liquid, weir) = match liquid.and_then(|m| Some((m, weir_of(m)?))) {
        Some(lw) => lw,
        None => {
            fresh = false;
            (M.water, 1.0)
        }
    };
    let b = Basin {
        id,
        face: FaceFrame::at_row(lo[1]),
        liquid,
        weir,
        reservoir: false,
        rows,
        bottom,
        volume,
        inflows,
        last_volume,
        outlets,
        wet,
        capped,
        fixed_level: 0.0,
        lo,
        hi,
        arriving,
        through,
        extra,
        linked,
        owed,
        rest,
        held,
        natural,
        // All unloaded now: it waits until they load, and is found again then.
        edge: chunks.clone(),
        chunks,
        steady,
    };
    Some((b, fresh))
}

impl Water {
    /// The basins as saved with the world (docs/WATER_FLOW.md §8); `revision` is each chunk's
    /// revision now (as the save that carries this writes it).
    pub fn save(&self, revision: impl Fn(ChunkCoord) -> u32) -> Vec<u8> {
        let mut o = Out::default();
        o.0.extend_from_slice(MAGIC);
        o.u32(self.next);
        o.f64(self.brought);
        o.f64(self.lost);
        let mut ids: Vec<u32> = self
            .basins
            .values()
            .filter(|b| !b.reservoir)
            .map(|b| b.id)
            .collect();
        ids.sort_unstable();
        let saved: HashSet<u32> = ids.iter().copied().collect();
        o.len(ids.len());
        for id in &ids {
            put_basin(&mut o, &self.basins[id], &revision);
        }
        let mut links: Vec<_> = self
            .links
            .iter()
            .filter(|((f, _), (t, _))| saved.contains(f) && saved.contains(t))
            .collect();
        links.sort_unstable_by_key(|((f, c), _)| (*f, *c));
        o.len(links.len());
        for (&(f, c), &(t, w)) in links {
            o.u32(f);
            o.cell(c);
            o.u32(t);
            o.f64(w);
        }
        let mut pending: Vec<_> = self
            .pending
            .iter()
            .filter(|((b, _), _)| saved.contains(b))
            .collect();
        pending.sort_unstable_by_key(|(k, _)| **k);
        o.len(pending.len());
        for (&(b, c), &v) in pending {
            o.u32(b);
            o.cell(c);
            o.f64(v);
        }
        let mut paths: Vec<_> = self
            .paths
            .iter()
            .filter(|((b, _), _)| saved.contains(b))
            .collect();
        paths.sort_unstable_by_key(|(k, _)| **k);
        o.len(paths.len());
        for (&(b, c), cells) in paths {
            o.u32(b);
            o.cell(c);
            o.cells(cells.iter());
        }
        o.set(&self.drawn);
        o.set(&self.edited);
        let mut lands: Vec<_> = self.lands.iter().collect();
        lands.sort_unstable_by_key(|(c, _)| **c);
        o.len(lands.len());
        for (&c, v) in lands {
            o.cell(c);
            let v: Vec<&Land> = v.iter().filter(|(b, _, _)| saved.contains(b)).collect();
            o.len(v.len());
            for &&(b, from, w) in &v {
                o.u32(b);
                o.cell(from);
                o.f64(w);
            }
        }
        o.len(self.touched.len());
        for &(c, m) in &self.touched {
            o.cell(c);
            o.state(m);
        }
        o.0
    }

    /// Takes in the basins [`Water::save`] wrote (none before: a world from before them is found
    /// again by its edits, as it was), `revision` each chunk's saved revision; a basin whose
    /// chunks changed since is left out, and what pointed at it with it. Returns how many were
    /// kept, or `None` for a record this build can't read (then nothing is taken in).
    pub fn restore(&mut self, bytes: &[u8], revision: impl Fn(ChunkCoord) -> u32) -> Option<usize> {
        let mut i = In { b: bytes, at: 0 };
        if i.take(4)? != MAGIC {
            return None;
        }
        let next = i.u32()?;
        let brought = i.f64()?;
        let lost = i.f64()?;
        let mut basins: HashMap<u32, Basin> = HashMap::default();
        for _ in 0..i.len()? {
            let (b, fresh) = get_basin(&mut i, &revision)?;
            if fresh {
                basins.insert(b.id, b);
            }
        }
        let kept = |b: &u32| basins.contains_key(b);
        let mut links = HashMap::default();
        for _ in 0..i.len()? {
            let (f, c, t, w) = (i.u32()?, i.cell()?, i.u32()?, i.f64()?);
            if kept(&f) && kept(&t) {
                links.insert((f, c), (t, w));
            }
        }
        let mut pending = HashMap::default();
        let mut gone = 0.0;
        for _ in 0..i.len()? {
            let (b, c, v) = (i.u32()?, i.cell()?, i.f64()?);
            if kept(&b) {
                pending.insert((b, c), v);
            } else {
                gone += v;
            }
        }
        let mut paths = HashMap::default();
        for _ in 0..i.len()? {
            let (b, c, cells) = (i.u32()?, i.cell()?, i.cells()?);
            if kept(&b) {
                paths.insert((b, c), cells);
            }
        }
        let drawn = i.set()?;
        let edited = i.set()?;
        let mut lands: HashMap<[i32; 3], Vec<Land>> = HashMap::default();
        for _ in 0..i.len()? {
            let c = i.cell()?;
            let mut v = Vec::new();
            for _ in 0..i.len()? {
                let land = (i.u32()?, i.cell()?, i.f64()?);
                if kept(&land.0) {
                    v.push(land);
                }
            }
            if !v.is_empty() {
                lands.insert(c, v);
            }
        }
        let mut touched = Vec::new();
        for _ in 0..i.len()? {
            let c = i.cell()?;
            if let Some(m) = i.state()? {
                touched.push((c, m));
            }
        }
        // Ways into a basin left out (or a reservoir, not saved) are found again.
        let ids: HashSet<u32> = basins.keys().copied().collect();
        for b in basins.values_mut() {
            for o in &mut b.outlets {
                if let Target::Basin(t) = o.target
                    && !ids.contains(&t)
                {
                    o.target = Target::Unresolved;
                    o.contact = false;
                }
            }
        }
        let n = basins.len();
        self.owner.clear();
        for b in basins.values() {
            for c in b.cells() {
                self.owner.insert(c, b.id);
            }
        }
        self.basins = basins;
        self.next = next.max(self.next);
        self.brought = brought;
        self.lost = lost + gone;
        self.links = links;
        self.pending = pending;
        self.paths = paths;
        self.drawn = drawn;
        self.edited = edited;
        self.lands = lands;
        self.touched = touched;
        Some(n)
    }
}
