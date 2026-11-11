//! A field's saved record: additive and optional (the world's `lava` table; a build without lava
//! never reads it). Version 2 (V2, tiles): the fluid's name, the origin, the time owed, whether it
//! grows, then each tile (its x, z, and each of its `TILE × TILE` columns' bed, depth, heat, water's
//! surface, crust and flags), then the vents. Version 1 (V1's fixed rectangle: its size, each column's bed,
//! depth, heat and water flag, the vents) still reads, as a fixed field. A record of another
//! version, an unknown fluid or a damaged one reads as `None` (the field is left as generated).

use crate::data::{LavaData, VentKind};
use crate::field::{DRY, Field, TILE, Vent};

const VERSION: u8 = 2;
const TN: usize = TILE * TILE;

/// The record of `field` (version 2).
pub fn record_of(field: &Field) -> Vec<u8> {
    let mut out = vec![VERSION];
    let name = field.fluid.name.as_bytes();
    out.push(name.len() as u8);
    out.extend_from_slice(name);
    out.extend_from_slice(&field.origin[0].to_le_bytes());
    out.extend_from_slice(&field.origin[1].to_le_bytes());
    out.extend_from_slice(&field.owed_s.to_le_bytes());
    out.push(u8::from(field.grows));
    out.extend_from_slice(&(field.tiles().len() as u32).to_le_bytes());
    for (k, t) in field.tiles().iter().enumerate() {
        out.extend_from_slice(&t[0].to_le_bytes());
        out.extend_from_slice(&t[1].to_le_bytes());
        for i in k * TN..(k + 1) * TN {
            out.extend_from_slice(&field.ground[i].to_le_bytes());
            out.extend_from_slice(&field.depth[i].to_le_bytes());
            out.extend_from_slice(&field.heat[i].to_le_bytes());
            out.extend_from_slice(&field.water_top[i].to_le_bytes());
            out.extend_from_slice(&field.crust[i].to_le_bytes());
            out.push(u8::from(field.water[i]) | (u8::from(field.open[i]) << 1));
        }
    }
    out.extend_from_slice(&(field.vents.len() as u16).to_le_bytes());
    for v in &field.vents {
        out.push(match v.kind {
            VentKind::Lake => 0,
            VentKind::River => 1,
        });
        out.extend_from_slice(&v.x.to_le_bytes());
        out.extend_from_slice(&v.z.to_le_bytes());
        out.extend_from_slice(&v.rate_m3s.to_le_bytes());
        out.extend_from_slice(&v.level_m.to_le_bytes());
    }
    out
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
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
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().ok()?);
        v.is_finite().then_some(v)
    }
    fn kind(&mut self) -> Option<VentKind> {
        match self.u8()? {
            0 => Some(VentKind::Lake),
            1 => Some(VentKind::River),
            _ => None,
        }
    }
}

/// The field a record holds, under `rules` (`None`: another version, an unknown fluid, damaged).
pub fn field_of(b: &[u8], rules: &LavaData) -> Option<Field> {
    let mut r = Reader(b);
    match r.u8()? {
        1 => field_v1(r, rules),
        2 => field_v2(r, rules),
        _ => None,
    }
}

fn name<'a>(r: &mut Reader<'a>) -> Option<&'a str> {
    let n = r.u8()? as usize;
    std::str::from_utf8(r.take(n)?).ok()
}

/// Version 1: a fixed `w × h` rectangle.
fn field_v1(mut r: Reader<'_>, rules: &LavaData) -> Option<Field> {
    let name = name(&mut r)?;
    let (w, h) = (r.u16()? as usize, r.u16()? as usize);
    let mut f = Field::new(rules, name, w, h).ok()?;
    f.origin = [r.i32()?, r.i32()?];
    f.owed_s = r.f32()?.max(0.0);
    for z in 0..h {
        for x in 0..w {
            let i = f.at(x, z);
            f.ground[i] = r.f32()?;
            f.depth[i] = r.f32()?.max(0.0);
            f.heat[i] = r.f32()?.clamp(0.0, 1.0);
            f.water[i] = match r.u8()? {
                0 => false,
                1 => true,
                _ => return None,
            };
            f.water_top[i] = DRY;
        }
    }
    for i in f.columns().collect::<Vec<_>>() {
        f.written[i] = f.lava_rows(i);
    }
    f.settle();
    for _ in 0..r.u16()? {
        let kind = r.kind()?;
        let (x, z) = (r.u16()? as usize, r.u16()? as usize);
        if x >= w || z >= h {
            return None;
        }
        f.vents.push(Vent {
            kind,
            x: x as i32,
            z: z as i32,
            rate_m3s: r.f32()?,
            level_m: r.f32()?,
        });
    }
    r.0.is_empty().then_some(f)
}

/// Version 2: tiles.
fn field_v2(mut r: Reader<'_>, rules: &LavaData) -> Option<Field> {
    let name = name(&mut r)?;
    let origin = [r.i32()?, r.i32()?];
    let owed = r.f32()?.max(0.0);
    let grows = match r.u8()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let mut f = if grows {
        Field::growing(rules, name).ok()?
    } else {
        // An empty fixed field: its tiles come from the record.
        Field::new(rules, name, 0, 0).ok()?
    };
    f.origin = origin;
    f.owed_s = owed;
    let n = r.u32()? as usize;
    let mut cols = Vec::with_capacity(TN);
    for _ in 0..n {
        let t = [r.i32()?, r.i32()?];
        if f.has_tile(t) {
            return None;
        }
        cols.clear();
        for _ in 0..TN {
            let (g, d, h, top) = (
                r.f32()?,
                r.f32()?.max(0.0),
                r.f32()?.clamp(0.0, 1.0),
                r.f32()?,
            );
            let crust = r.f32()?.max(0.0);
            let flags = r.u8()?;
            if flags > 3 {
                return None;
            }
            cols.push((g, d, h, top, crust, flags & 1 != 0, flags & 2 != 0));
        }
        f.load_tile(t, &cols);
    }
    for _ in 0..r.u16()? {
        let kind = r.kind()?;
        let (x, z) = (r.i32()?, r.i32()?);
        f.slot(x, z)?;
        f.vents.push(Vent {
            kind,
            x,
            z,
            rate_m3s: r.f32()?,
            level_m: r.f32()?,
        });
    }
    r.0.is_empty().then_some(f)
}
