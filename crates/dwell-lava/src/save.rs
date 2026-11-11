//! A field's saved record: additive and optional (a new world table, `lava`, once wired; a build
//! without lava never reads it). Version 1: the fluid's name, the size and origin, the time owed,
//! each column's bed, depth, heat and water flag, and the vents. A record of another version, an
//! unknown fluid or a damaged one reads as `None` (the field is left as generated).

use crate::data::{LavaData, VentKind};
use crate::field::{Field, Vent};

const VERSION: u8 = 1;

/// The record of `field`.
pub fn record_of(field: &Field) -> Vec<u8> {
    let mut out = vec![VERSION];
    let name = field.fluid.name.as_bytes();
    out.push(name.len() as u8);
    out.extend_from_slice(name);
    out.extend_from_slice(&(field.w as u16).to_le_bytes());
    out.extend_from_slice(&(field.h as u16).to_le_bytes());
    out.extend_from_slice(&field.origin[0].to_le_bytes());
    out.extend_from_slice(&field.origin[1].to_le_bytes());
    out.extend_from_slice(&field.owed_s.to_le_bytes());
    for i in 0..field.w * field.h {
        out.extend_from_slice(&field.ground[i].to_le_bytes());
        out.extend_from_slice(&field.depth[i].to_le_bytes());
        out.extend_from_slice(&field.heat[i].to_le_bytes());
        out.push(u8::from(field.water[i]));
    }
    out.extend_from_slice(&(field.vents.len() as u16).to_le_bytes());
    for v in &field.vents {
        out.push(match v.kind {
            VentKind::Lake => 0,
            VentKind::River => 1,
        });
        out.extend_from_slice(&(v.x as u16).to_le_bytes());
        out.extend_from_slice(&(v.z as u16).to_le_bytes());
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
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().ok()?);
        v.is_finite().then_some(v)
    }
}

/// The field a record holds, under `rules` (`None`: another version, an unknown fluid, damaged).
pub fn field_of(b: &[u8], rules: &LavaData) -> Option<Field> {
    let mut r = Reader(b);
    if r.u8()? != VERSION {
        return None;
    }
    let n = r.u8()? as usize;
    let name = std::str::from_utf8(r.take(n)?).ok()?;
    let (w, h) = (r.u16()? as usize, r.u16()? as usize);
    let mut f = Field::new(rules, name, w, h).ok()?;
    f.origin = [r.i32()?, r.i32()?];
    f.owed_s = r.f32()?.max(0.0);
    for i in 0..w * h {
        f.ground[i] = r.f32()?;
        f.depth[i] = r.f32()?.max(0.0);
        f.heat[i] = r.f32()?.clamp(0.0, 1.0);
        f.water[i] = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
    }
    for _ in 0..r.u16()? {
        let kind = match r.u8()? {
            0 => VentKind::Lake,
            1 => VentKind::River,
            _ => return None,
        };
        let (x, z) = (r.u16()? as usize, r.u16()? as usize);
        if x >= w || z >= h {
            return None;
        }
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
