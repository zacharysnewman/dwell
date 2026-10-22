//! The world map's bytes (ADR 0049): what the browser keeps in IndexedDB and hands its workers, and
//! a native process keeps on disk. Little-endian, lossless (every value as stored), ending in an
//! FNV-1a checksum of everything before it, so a truncated or damaged copy is refused rather than
//! generating different terrain.
//!
//! Layout: magic `DWMAP`, format, fingerprint (the build's, `FINGERPRINT`), seed, the generation
//! settings' hash (`WorldMap::settings`), n, cell, x0;
//! the cells' heights (f32), flags (u8) and lakes (u16); the lakes (level, entry, outlet); the
//! rivers (from-lake, end, mouth, vertices); the checksum (u64).

use super::{MapLake, MapRiver, MapVertex, Mouth, RiverEnd, WorldMap};

const MAGIC: &[u8; 5] = b"DWMAP";
const FORMAT: u16 = 1;

/// FNV-1a 64 over bytes.
pub fn checksum(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn encode(m: &WorldMap) -> Vec<u8> {
    encode_with(m, super::FINGERPRINT)
}

/// The bytes with a zero fingerprint (the digest's: the same on every build of the same map).
pub fn encode_unsigned(m: &WorldMap) -> Vec<u8> {
    encode_with(m, 0)
}

fn encode_with(m: &WorldMap, fingerprint: u64) -> Vec<u8> {
    let len = m.n * m.n;
    let mut out = Vec::with_capacity(64 + len * 7 + m.rivers.len() * 64);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&fingerprint.to_le_bytes());
    out.extend_from_slice(&m.seed.to_le_bytes());
    out.extend_from_slice(&m.settings.to_le_bytes());
    out.extend_from_slice(&(m.n as u32).to_le_bytes());
    out.extend_from_slice(&m.cell.to_le_bytes());
    out.extend_from_slice(&m.x0.to_le_bytes());
    for h in &m.height {
        out.extend_from_slice(&h.to_le_bytes());
    }
    out.extend_from_slice(&m.flags);
    for l in &m.lake {
        out.extend_from_slice(&l.to_le_bytes());
    }
    out.extend_from_slice(&(m.lakes.len() as u32).to_le_bytes());
    for l in &m.lakes {
        out.extend_from_slice(&l.level.to_le_bytes());
        out.extend_from_slice(&l.entry.to_le_bytes());
        out.extend_from_slice(&l.outlet.to_le_bytes());
    }
    out.extend_from_slice(&(m.rivers.len() as u32).to_le_bytes());
    for r in &m.rivers {
        out.extend_from_slice(&r.from_lake.to_le_bytes());
        let (kind, arg) = match r.end {
            RiverEnd::Sea => (0u8, 0u32),
            RiverEnd::Lake(l) => (1, u32::from(l)),
            RiverEnd::Joins(k) => (2, k),
        };
        out.push(kind);
        out.extend_from_slice(&arg.to_le_bytes());
        out.push(match r.mouth {
            Mouth::None => 0,
            Mouth::Delta => 1,
            Mouth::Estuary => 2,
        });
        out.extend_from_slice(&(r.path.len() as u32).to_le_bytes());
        for v in &r.path {
            for f in [v.x, v.z, v.surface, v.width, v.depth, v.discharge] {
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
    }
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).ok_or("overflow")?;
        let s = self.b.get(self.at..end).ok_or("the map is truncated")?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("2")))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
}

/// The header's (fingerprint, seed, settings hash), without decoding the rest.
pub fn peek(bytes: &[u8]) -> Result<(u64, u64, u64), String> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(5)? != MAGIC {
        return Err("not a world map".into());
    }
    if r.u16()? != FORMAT {
        return Err("a world map of another format".into());
    }
    Ok((r.u64()?, r.u64()?, r.u64()?))
}

/// Checks a map's bytes (whole, unchanged, this build's) without decoding them; returns its seed
/// and settings hash.
pub fn verify(bytes: &[u8]) -> Result<(u64, u64), String> {
    if bytes.len() < 8 {
        return Err("the map is truncated".into());
    }
    let (body, sum) = bytes.split_at(bytes.len() - 8);
    if checksum(body) != u64::from_le_bytes(sum.try_into().expect("8")) {
        return Err("the map's checksum does not match".into());
    }
    let (fingerprint, seed, settings) = peek(body)?;
    if fingerprint != super::FINGERPRINT {
        return Err("a world map from another build of the generator".into());
    }
    Ok((seed, settings))
}

/// A map from its bytes, refused when damaged or made by another build.
pub fn decode(bytes: &[u8]) -> Result<WorldMap, String> {
    let (seed, settings) = verify(bytes)?;
    let body = &bytes[..bytes.len() - 8];
    let mut r = Reader {
        b: body,
        at: 5 + 2 + 8 + 8 + 8,
    };
    let n = r.u32()? as usize;
    if n == 0 || n > 8192 {
        return Err("a world map of an impossible size".into());
    }
    let cell = r.f64()?;
    let x0 = r.f64()?;
    let len = n * n;
    let height = r
        .take(len * 4)?
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().expect("4")))
        .collect();
    let flags = r.take(len)?.to_vec();
    let lake = r
        .take(len * 2)?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().expect("2")))
        .collect();
    let lakes_n = r.u32()? as usize;
    let mut lakes = Vec::with_capacity(lakes_n.min(65_536));
    for _ in 0..lakes_n {
        lakes.push(MapLake {
            level: r.f32()?,
            entry: r.u32()?,
            outlet: r.u32()?,
        });
    }
    let rivers_n = r.u32()? as usize;
    let mut rivers = Vec::with_capacity(rivers_n.min(65_536));
    for _ in 0..rivers_n {
        let from_lake = r.u16()?;
        let kind = r.u8()?;
        let arg = r.u32()?;
        let end = match kind {
            0 => RiverEnd::Sea,
            1 => RiverEnd::Lake(arg as u16),
            2 => RiverEnd::Joins(arg),
            _ => return Err("a river with an unknown end".into()),
        };
        let mouth = match r.u8()? {
            0 => Mouth::None,
            1 => Mouth::Delta,
            2 => Mouth::Estuary,
            _ => return Err("a river with an unknown mouth".into()),
        };
        let count = r.u32()? as usize;
        let mut path = Vec::with_capacity(count.min(1 << 20));
        for _ in 0..count {
            path.push(MapVertex {
                x: r.f32()?,
                z: r.f32()?,
                surface: r.f32()?,
                width: r.f32()?,
                depth: r.f32()?,
                discharge: r.f32()?,
            });
        }
        if path.len() < 2 {
            return Err("a river of fewer than two vertices".into());
        }
        rivers.push(MapRiver {
            from_lake,
            end,
            mouth,
            path,
        });
    }
    if r.at != body.len() {
        return Err("trailing bytes after the map".into());
    }
    let mut map = WorldMap::assemble(seed, n, cell, x0, height, flags, lake, lakes, rivers);
    map.settings = settings;
    Ok(map)
}
