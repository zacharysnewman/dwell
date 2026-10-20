//! Jobs and results as bytes, for `postMessage` between the main thread and the workers (one
//! transferable buffer each way). Little-endian; every message starts with the job id (u32).

use dwell_mesh::{ChunkMeshes, FlatMesh, MeshArrays, SectionMeshes};
use dwell_world::ChunkCoord;
use dwell_world::lod::{LodBounds, LodCoord};

use super::{Job, Output};
use crate::render::march::{ChunkBricks, SectionBricks};
use crate::render::seam::{ChunkTerrain, Payloads, SectionTerrain};

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u16s(&mut self, v: &[u16]) {
        self.u32(v.len() as u32);
        v.iter()
            .for_each(|x| self.0.extend_from_slice(&x.to_le_bytes()));
    }
    fn u32s(&mut self, v: &[u32]) {
        self.u32(v.len() as u32);
        v.iter()
            .for_each(|x| self.0.extend_from_slice(&x.to_le_bytes()));
    }
    fn f32s(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        v.iter()
            .for_each(|x| self.0.extend_from_slice(&x.to_le_bytes()));
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    fn opt_f32s(&mut self, v: &Option<Vec<f32>>) {
        match v {
            Some(v) => {
                self.u8(1);
                self.f32s(v);
            }
            None => self.u8(0),
        }
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn arrays(&mut self, m: &MeshArrays) {
        self.f32s(&m.positions);
        self.f32s(&m.normals);
        self.f32s(&m.colors);
        self.f32s(&m.uvs);
        self.f32s(&m.tiles);
        self.f32s(&m.fluids);
        self.f32s(&m.plants);
        self.f32s(&m.bare);
        self.u32s(&m.indices);
    }
    /// The marcher's bricks: a node build as words (`dwell_march::NodeBuild::to_words`).
    fn chunk_bricks(&mut self, b: &ChunkBricks) {
        self.u32s(&b.0.to_words());
    }
    fn section_bricks(&mut self, b: &SectionBricks) {
        self.u32s(&b.0.to_words());
    }
    fn flat(&mut self, m: &FlatMesh) {
        self.f32s(&m.positions);
        self.f32s(&m.normals);
        self.f32s(&m.colors);
        self.f32s(&m.fluids);
        self.u32s(&m.indices);
    }
}

struct R<'a>(&'a [u8]);

/// A truncated or malformed message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError;

type Res<T> = Result<T, DecodeError>;

impl R<'_> {
    fn take(&mut self, n: usize) -> Res<&[u8]> {
        if self.0.len() < n {
            return Err(DecodeError);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn arr<const N: usize>(&mut self) -> Res<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| DecodeError)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.arr::<1>()?[0])
    }
    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    fn f64(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.arr()?))
    }
    fn len(&mut self, size: usize) -> Res<usize> {
        let n = self.u32()? as usize;
        if self.0.len() < n.saturating_mul(size) {
            return Err(DecodeError);
        }
        Ok(n)
    }
    fn u16s(&mut self) -> Res<Vec<u16>> {
        let n = self.len(2)?;
        Ok(self
            .take(n * 2)?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect())
    }
    fn u32s(&mut self) -> Res<Vec<u32>> {
        let n = self.len(4)?;
        Ok(self
            .take(n * 4)?
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
    fn f32s(&mut self) -> Res<Vec<f32>> {
        let n = self.len(4)?;
        Ok(self
            .take(n * 4)?
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
    fn bytes(&mut self) -> Res<Vec<u8>> {
        let n = self.len(1)?;
        Ok(self.take(n)?.to_vec())
    }
    fn opt_f32s(&mut self) -> Res<Option<Vec<f32>>> {
        Ok(if self.u8()? == 1 {
            Some(self.f32s()?)
        } else {
            None
        })
    }
    fn arrays(&mut self) -> Res<MeshArrays> {
        Ok(MeshArrays {
            positions: self.f32s()?,
            normals: self.f32s()?,
            colors: self.f32s()?,
            uvs: self.f32s()?,
            tiles: self.f32s()?,
            fluids: self.f32s()?,
            plants: self.f32s()?,
            bare: self.f32s()?,
            indices: self.u32s()?,
        })
    }
    fn chunk_bricks(&mut self) -> Res<ChunkBricks> {
        Ok(ChunkBricks(
            dwell_march::NodeBuild::from_words(&self.u32s()?).ok_or(DecodeError)?,
        ))
    }
    fn section_bricks(&mut self) -> Res<SectionBricks> {
        Ok(SectionBricks(
            dwell_march::NodeBuild::from_words(&self.u32s()?).ok_or(DecodeError)?,
        ))
    }
    fn flat(&mut self) -> Res<FlatMesh> {
        Ok(FlatMesh {
            positions: self.f32s()?,
            normals: self.f32s()?,
            colors: self.f32s()?,
            fluids: self.f32s()?,
            indices: self.u32s()?,
        })
    }
}

pub fn encode_job(id: u32, job: &Job) -> Vec<u8> {
    let mut w = W::default();
    w.u32(id);
    match job {
        Job::Generate { coord } => {
            w.u8(0);
            [coord.x, coord.y, coord.z]
                .into_iter()
                .for_each(|v| w.i32(v));
        }
        Job::LodBounds { level, i, k } => {
            w.u8(1);
            [*level, *i, *k].into_iter().for_each(|v| w.i32(v));
        }
        Job::Lod { coord } => {
            w.u8(2);
            [coord.level, coord.i, coord.j, coord.k]
                .into_iter()
                .for_each(|v| w.i32(v));
        }
        Job::Map {
            x0,
            z0,
            step,
            n,
            face,
        } => {
            w.u8(3);
            [*x0, *z0, *step, *n].into_iter().for_each(|v| w.i32(v));
            w.u8(*face);
        }
        Job::MeshChunk {
            padded,
            mirror,
            payloads,
            origin,
        } => {
            w.u8(4);
            w.u8(u8::from(*mirror));
            w.u8(payloads.0);
            origin.iter().for_each(|v| w.i32(*v));
            w.u16s(padded);
        }
        Job::MeshSection {
            cells,
            surface,
            water_drop,
            mirror,
            payloads,
        } => {
            w.u8(5);
            w.u8(u8::from(*mirror));
            w.u8(payloads.0);
            w.f64(*water_drop);
            w.u16s(cells);
            w.opt_f32s(surface);
        }
        Job::Registry { payload } => {
            w.u8(6);
            w.bytes(payload);
        }
    }
    w.0
}

pub fn decode_job(bytes: &[u8]) -> Result<(u32, Job), DecodeError> {
    let mut r = R(bytes);
    let id = r.u32()?;
    let job = match r.u8()? {
        0 => Job::Generate {
            coord: ChunkCoord {
                x: r.i32()?,
                y: r.i32()?,
                z: r.i32()?,
            },
        },
        1 => Job::LodBounds {
            level: r.i32()?,
            i: r.i32()?,
            k: r.i32()?,
        },
        2 => Job::Lod {
            coord: LodCoord::new(r.i32()?, r.i32()?, r.i32()?, r.i32()?),
        },
        3 => Job::Map {
            x0: r.i32()?,
            z0: r.i32()?,
            step: r.i32()?,
            n: r.i32()?,
            face: r.u8()?,
        },
        4 => {
            let mirror = r.u8()? != 0;
            let payloads = Payloads(r.u8()?);
            let origin = [r.i32()?, r.i32()?, r.i32()?];
            Job::MeshChunk {
                mirror,
                payloads,
                origin,
                padded: r.u16s()?,
            }
        }
        5 => {
            let mirror = r.u8()? != 0;
            let payloads = Payloads(r.u8()?);
            let water_drop = r.f64()?;
            Job::MeshSection {
                mirror,
                payloads,
                water_drop,
                cells: r.u16s()?,
                surface: r.opt_f32s()?,
            }
        }
        6 => Job::Registry {
            payload: r.bytes()?,
        },
        _ => return Err(DecodeError),
    };
    Ok((id, job))
}

pub fn encode_output(id: u32, out: &Output) -> Vec<u8> {
    let mut w = W::default();
    w.u32(id);
    match out {
        Output::Chunk { voxels, hash } => {
            w.u8(0);
            w.u64(*hash);
            w.u16s(voxels);
        }
        Output::Bounds(b) => {
            w.u8(1);
            [b.lo, b.hi, b.lo_b, b.hi_b]
                .into_iter()
                .for_each(|v| w.f64(v));
            w.u8(u8::from(b.any_inside) | (u8::from(b.bifacial) << 1));
        }
        Output::Lod {
            cells,
            surface,
            surface_b,
        } => {
            w.u8(2);
            w.u16s(cells);
            w.opt_f32s(surface);
            w.opt_f32s(surface_b);
        }
        Output::Map(bytes) => {
            w.u8(3);
            match bytes {
                Some(b) => {
                    w.u8(1);
                    w.bytes(b);
                }
                None => w.u8(0),
            }
        }
        Output::ChunkMesh(t) => {
            w.u8(4);
            w.u8(u8::from(t.raster.is_some()) | (u8::from(t.march.is_some()) << 1));
            if let Some(m) = &t.raster {
                w.arrays(&m.opaque);
                w.arrays(&m.transparent);
                w.arrays(&m.cutout);
            }
            if let Some(b) = &t.march {
                w.chunk_bricks(b);
            }
        }
        Output::SectionMesh(t) => {
            w.u8(5);
            w.u8(u8::from(t.raster.is_some()) | (u8::from(t.march.is_some()) << 1));
            if let Some(m) = &t.raster {
                w.flat(&m.opaque);
                w.flat(&m.water);
                m.skirts.iter().for_each(|s| w.flat(s));
            }
            if let Some(b) = &t.march {
                w.section_bricks(b);
            }
        }
        Output::Failed(e) => {
            w.u8(6);
            w.str(e);
        }
        Output::Done => w.u8(7),
    }
    w.0
}

pub fn decode_output(bytes: &[u8]) -> Result<(u32, Output), DecodeError> {
    let mut r = R(bytes);
    let id = r.u32()?;
    let out = match r.u8()? {
        0 => {
            let hash = r.u64()?;
            Output::Chunk {
                hash,
                voxels: r.u16s()?,
            }
        }
        1 => {
            let (lo, hi, lo_b, hi_b) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?);
            let f = r.u8()?;
            Output::Bounds(LodBounds {
                lo,
                hi,
                lo_b,
                hi_b,
                any_inside: f & 1 != 0,
                bifacial: f & 2 != 0,
            })
        }
        2 => Output::Lod {
            cells: r.u16s()?,
            surface: r.opt_f32s()?,
            surface_b: r.opt_f32s()?,
        },
        3 => Output::Map(if r.u8()? == 1 { Some(r.bytes()?) } else { None }),
        4 => {
            let parts = Payloads(r.u8()?);
            let raster = if parts.mesh() {
                Some(ChunkMeshes {
                    opaque: r.arrays()?,
                    transparent: r.arrays()?,
                    cutout: r.arrays()?,
                })
            } else {
                None
            };
            let march = if parts.bricks() {
                Some(r.chunk_bricks()?)
            } else {
                None
            };
            Output::ChunkMesh(Box::new(ChunkTerrain { raster, march }))
        }
        5 => {
            let parts = Payloads(r.u8()?);
            let raster = if parts.mesh() {
                let (opaque, water) = (r.flat()?, r.flat()?);
                let mut skirts: [FlatMesh; 6] = Default::default();
                for s in &mut skirts {
                    *s = r.flat()?;
                }
                Some(SectionMeshes {
                    opaque,
                    water,
                    skirts,
                })
            } else {
                None
            };
            let march = if parts.bricks() {
                Some(r.section_bricks()?)
            } else {
                None
            };
            Output::SectionMesh(Box::new(SectionTerrain { raster, march }))
        }
        6 => Output::Failed(String::from_utf8_lossy(&r.bytes()?).into_owned()),
        7 => Output::Done,
        _ => return Err(DecodeError),
    };
    Ok((id, out))
}

#[cfg(test)]
mod tests {
    use super::super::Runner;
    use super::*;
    use dwell_world::GENERATOR_TERRAIN;

    #[test]
    fn every_job_and_result_round_trips() {
        let r = Runner::world(GENERATOR_TERRAIN, 7);
        let jobs = [
            Job::Generate {
                coord: ChunkCoord { x: 0, y: -1, z: 0 },
            },
            Job::LodBounds {
                level: 3,
                i: -2,
                k: 5,
            },
            Job::Lod {
                coord: dwell_lod::grid::section_at(3, [0.0, 0.0, 0.0]),
            },
            Job::Map {
                x0: -10,
                z0: 4,
                step: 8,
                n: 4,
                face: 1,
            },
            Job::Registry {
                payload: dwell_blocks::sync::extension_payload(),
            },
        ];
        let mut jobs = jobs.to_vec();
        for payloads in [Payloads::MESH, Payloads::BRICKS, Payloads::BOTH] {
            jobs.push(Job::MeshChunk {
                padded: vec![1; dwell_mesh::PADDED_VOLUME],
                mirror: true,
                payloads,
                origin: [-32, 64, 96],
            });
            jobs.push(Job::MeshSection {
                cells: vec![1; dwell_world::lod::VOLUME],
                surface: None,
                water_drop: 0.125,
                mirror: false,
                payloads,
            });
        }
        for (n, job) in jobs.into_iter().enumerate() {
            let id = 1000 + n as u32;
            assert_eq!(
                decode_job(&encode_job(id, &job)).unwrap(),
                (id, job.clone())
            );
            let out = r.run(job);
            assert!(!matches!(out, Output::Failed(_)), "{out:?}");
            assert_eq!(decode_output(&encode_output(id, &out)).unwrap(), (id, out));
        }
        let failed = Output::Failed("nope".into());
        assert_eq!(
            decode_output(&encode_output(3, &failed)).unwrap(),
            (3, failed)
        );
    }

    #[test]
    fn truncated_messages_are_errors_not_panics() {
        let bytes = encode_job(
            1,
            &Job::MeshChunk {
                padded: vec![0; 100],
                mirror: false,
                payloads: Payloads::BOTH,
                origin: [0; 3],
            },
        );
        for n in 0..bytes.len() {
            assert_eq!(decode_job(&bytes[..n]), Err(DecodeError));
        }
    }
}
