//! An ecosystem's bytes (saved with the world, docs/ECOLOGY.md §5 slice 2). Species are written by
//! name, so a build whose data adds, removes or reorders species reads an older record: groups of a
//! species it doesn't know are dropped, the rest mapped to its own indices. A forage grid of
//! another size (the data's `cellM` changed) starts full.
//!
//! Layout (little-endian): `u8` version (1), `u64` seed, `f64` day, `u32` next id, `u32` cells,
//! that many `f64` forage, `u16` names, each `u16` length + UTF-8, `u32` groups, each `u32` id,
//! `u16` name index, `f64` x, z, size, hunger, energy, `u8` activity.

use crate::data::Data;
use crate::island::Island;
use crate::sim::{Activity, Ecosystem, Group};

const VERSION: u8 = 1;

const ACTIVITIES: [Activity; 7] = [
    Activity::Resting,
    Activity::Grazing,
    Activity::Moving,
    Activity::Fleeing,
    Activity::Hunting,
    Activity::Roaming,
    Activity::Dispersing,
];

impl Activity {
    /// The activity's name (the dev read-out).
    pub fn name(self) -> &'static str {
        match self {
            Activity::Resting => "resting",
            Activity::Grazing => "grazing",
            Activity::Moving => "moving",
            Activity::Fleeing => "fleeing",
            Activity::Hunting => "hunting",
            Activity::Roaming => "roaming",
            Activity::Dispersing => "dispersing",
        }
    }
    fn code(self) -> u8 {
        ACTIVITIES.iter().position(|&a| a == self).unwrap_or(0) as u8
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_bits(self.u64()?))
    }
}

impl Ecosystem {
    /// The ecosystem's bytes, its species named from `data`.
    pub fn to_bytes(&self, data: &Data) -> Vec<u8> {
        let mut b = vec![VERSION];
        b.extend(self.seed.to_le_bytes());
        b.extend(self.day.to_bits().to_le_bytes());
        b.extend(self.next_id.to_le_bytes());
        b.extend((self.grazing.len() as u32).to_le_bytes());
        for g in &self.grazing {
            b.extend(g.to_bits().to_le_bytes());
        }
        b.extend((data.species.len() as u16).to_le_bytes());
        for s in data.species {
            b.extend((s.name.len() as u16).to_le_bytes());
            b.extend(s.name.as_bytes());
        }
        b.extend((self.groups.len() as u32).to_le_bytes());
        for g in &self.groups {
            b.extend(g.id.to_le_bytes());
            b.extend((g.species as u16).to_le_bytes());
            for v in [g.x, g.z, g.size, g.hunger, g.energy] {
                b.extend(v.to_bits().to_le_bytes());
            }
            b.push(g.activity.code());
        }
        b
    }

    /// An ecosystem from [`Ecosystem::to_bytes`]'s bytes on `island` under `data`; None if they
    /// can't be read (another version, cut short).
    pub fn from_bytes(bytes: &[u8], island: &Island, data: &Data) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.u8()? != VERSION {
            return None;
        }
        let seed = r.u64()?;
        let day = r.f64()?;
        let next_id = r.u32()?;
        let cells = r.u32()? as usize;
        let mut grazing = Vec::with_capacity(cells.min(1 << 20));
        for _ in 0..cells {
            grazing.push(r.f64()?);
        }
        if grazing.len() != island.grazing_max.len() {
            grazing = island.grazing_max.clone();
        }
        let names = r.u16()?;
        let mut map = Vec::with_capacity(names as usize);
        for _ in 0..names {
            let len = r.u16()? as usize;
            let name = std::str::from_utf8(r.take(len)?).ok()?;
            map.push(data.find(name));
        }
        let count = r.u32()?;
        let mut groups = Vec::new();
        for _ in 0..count {
            let id = r.u32()?;
            let s = *map.get(r.u16()? as usize)?;
            let [x, z, size, hunger, energy] = [r.f64()?, r.f64()?, r.f64()?, r.f64()?, r.f64()?];
            let activity = *ACTIVITIES.get(r.u8()? as usize)?;
            if let Some(species) = s {
                groups.push(Group {
                    id,
                    species,
                    x,
                    z,
                    size,
                    hunger,
                    energy,
                    activity,
                });
            }
        }
        r.0.is_empty().then_some(Self {
            seed,
            day,
            grazing,
            groups,
            next_id,
        })
    }
}
