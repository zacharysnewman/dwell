//! A GLB reader for what the import writes (shared/models/blender_import.py): one skinned mesh
//! (positions, normals, a colour per vertex, four joints and weights) or one per model-detail level
//! (meshes `detail_0`, `detail_1`, …, ADR 0070 §10), its skin, the node tree and the clips
//! (translation, rotation and scale channels, linear or step).

use serde_json::Value;

use crate::math::{Mat4, Quat};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub t: [f32; 3],
    pub r: Quat,
    pub s: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Translation,
    Rotation,
    Scale,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub node: usize,
    pub path: Path,
    /// Linear (else step).
    pub linear: bool,
    pub times: Vec<f32>,
    /// 3 or 4 floats a key.
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    /// Seconds (its last key).
    pub duration: f32,
    pub channels: Vec<Channel>,
}

/// One model-detail level's mesh (ADR 0070 §10): the same skin and clips as the model's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// sRGB, 0..1.
    pub colors: Vec<[f32; 3]>,
    pub joints: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// A level's mesh, borrowed (level 0 is the model's own fields).
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub colors: &'a [[f32; 3]],
    pub joints: &'a [[u8; 4]],
    pub weights: &'a [[f32; 4]],
    pub indices: &'a [u32],
}

impl Geometry {
    fn view(&self) -> View<'_> {
        View {
            positions: &self.positions,
            normals: &self.normals,
            colors: &self.colors,
            joints: &self.joints,
            weights: &self.weights,
            indices: &self.indices,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    /// Level 0's mesh (the full one).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// sRGB, 0..1.
    pub colors: Vec<[f32; 3]>,
    pub joints: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub nodes: Vec<Node>,
    /// The skin's joints (node indices) and their inverse bind matrices.
    pub skin: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
    pub clips: Vec<Clip>,
    /// The model-detail levels after the first (detail_1, detail_2, …); empty: one mesh only.
    pub levels: Vec<Geometry>,
}

impl Model {
    /// How many detail levels it has (1: one mesh).
    pub fn level_count(&self) -> usize {
        1 + self.levels.len()
    }

    /// Level `k`'s mesh; a level past its last draws its last (the coarsest it has).
    pub fn level(&self, k: usize) -> View<'_> {
        match k.min(self.levels.len()) {
            0 => View {
                positions: &self.positions,
                normals: &self.normals,
                colors: &self.colors,
                joints: &self.joints,
                weights: &self.weights,
                indices: &self.indices,
            },
            k => self.levels[k - 1].view(),
        }
    }

    pub fn clip(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name == name)
    }
}

fn err(msg: impl Into<String>) -> String {
    msg.into()
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| err("GLB truncated"))
}

/// An accessor's elements as floats (normalised integers to 0..1, plain ones as they are).
fn floats(json: &Value, bin: &[u8], index: usize) -> Result<(Vec<f32>, usize), String> {
    let a = &json["accessors"][index];
    let count = a["count"]
        .as_u64()
        .ok_or_else(|| err("accessor without count"))? as usize;
    let width = match a["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        Some("MAT4") => 16,
        t => return Err(format!("accessor type {t:?}")),
    };
    let comp = a["componentType"].as_u64().unwrap_or(0);
    let normalized = a["normalized"].as_bool().unwrap_or(false);
    let size = match comp {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        c => return Err(format!("component type {c}")),
    };
    let view = &json["bufferViews"][a["bufferView"]
        .as_u64()
        .ok_or_else(|| err("sparse accessors aren't read"))?
        as usize];
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize
        + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let stride = view["byteStride"]
        .as_u64()
        .map_or(width * size, |s| s as usize);
    let mut out = Vec::with_capacity(count * width);
    for i in 0..count {
        for k in 0..width {
            let at = start + i * stride + k * size;
            let s = bin
                .get(at..at + size)
                .ok_or_else(|| err("accessor past the buffer"))?;
            let v = match comp {
                5126 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                5121 if normalized => f32::from(s[0]) / 255.0,
                5121 => f32::from(s[0]),
                5123 if normalized => f32::from(u16::from_le_bytes([s[0], s[1]])) / 65535.0,
                5123 => f32::from(u16::from_le_bytes([s[0], s[1]])),
                5125 => u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32,
                c => return Err(format!("component type {c} not read here")),
            };
            out.push(v);
        }
    }
    Ok((out, width))
}

fn arr<const N: usize>(v: &Value, fallback: [f32; N]) -> [f32; N] {
    match v.as_array() {
        Some(a) if a.len() == N => std::array::from_fn(|k| a[k].as_f64().unwrap_or(0.0) as f32),
        _ => fallback,
    }
}

fn chunks<const N: usize>(v: &[f32]) -> Vec<[f32; N]> {
    v.chunks_exact(N)
        .map(|c| std::array::from_fn(|k| c[k]))
        .collect()
}

/// Reads a GLB. Fails with a message on anything it doesn't read (more than one mesh or skin, a
/// missing attribute).
pub fn parse(bytes: &[u8]) -> Result<Model, String> {
    read(bytes, true)
}

/// Reads a static GLB (a prop, shared/models/props.json): one mesh with a colour per vertex, no
/// skin or clips (every vertex on joint 0 at weight 1, so the skinned paths can draw it too).
pub fn parse_static(bytes: &[u8]) -> Result<Model, String> {
    read(bytes, false)
}

fn read(bytes: &[u8], skinned: bool) -> Result<Model, String> {
    if u32_at(bytes, 0)? != 0x4654_6c67 {
        return Err(err("not a GLB"));
    }
    let json_len = u32_at(bytes, 12)? as usize;
    if u32_at(bytes, 16)? != 0x4e4f_534a {
        return Err(err("GLB: first chunk is not JSON"));
    }
    let json: Value = serde_json::from_slice(
        bytes
            .get(20..20 + json_len)
            .ok_or_else(|| err("GLB truncated"))?,
    )
    .map_err(|e| format!("GLB JSON: {e}"))?;
    let bin_at = 20 + json_len;
    let bin_len = u32_at(bytes, bin_at)? as usize;
    let bin = bytes
        .get(bin_at + 8..bin_at + 8 + bin_len)
        .ok_or_else(|| err("GLB truncated"))?;

    let meshes = json["meshes"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    // The levels: meshes detail_0, detail_1, … in order; else the one mesh.
    let mut order: Vec<&Value> = Vec::new();
    while let Some(m) = meshes
        .iter()
        .find(|m| m["name"].as_str() == Some(&format!("detail_{}", order.len())))
    {
        order.push(m);
    }
    if order.is_empty() {
        if meshes.len() != 1 {
            return Err(format!("{} meshes, not one", meshes.len()));
        }
        order.push(&meshes[0]);
    } else if order.len() != meshes.len() {
        return Err(format!(
            "{} meshes, {} of them detail_0.. in order",
            meshes.len(),
            order.len()
        ));
    }
    let mut geoms = order
        .iter()
        .map(|mesh| geometry(&json, bin, mesh, skinned))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let g = geoms.next().ok_or_else(|| err("no mesh"))?;
    let mut m = Model {
        positions: g.positions,
        normals: g.normals,
        colors: g.colors,
        joints: g.joints,
        weights: g.weights,
        indices: g.indices,
        levels: geoms.collect(),
        ..Model::default()
    };

    let nodes = json["nodes"].as_array().ok_or_else(|| err("no nodes"))?;
    m.nodes = nodes
        .iter()
        .map(|n| Node {
            name: n["name"].as_str().unwrap_or("").to_string(),
            parent: None,
            t: arr(&n["translation"], [0.0; 3]),
            r: arr(&n["rotation"], [0.0, 0.0, 0.0, 1.0]),
            s: arr(&n["scale"], [1.0; 3]),
        })
        .collect();
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().into_iter().flatten() {
            let c = c.as_u64().ok_or_else(|| err("bad child"))? as usize;
            m.nodes
                .get_mut(c)
                .ok_or_else(|| err("child past the nodes"))?
                .parent = Some(i);
        }
    }

    if !skinned {
        if json["skins"].as_array().is_some_and(|s| !s.is_empty()) {
            return Err(err("a static model with a skin"));
        }
        return Ok(m);
    }
    let skin = &json["skins"][0];
    m.skin = skin["joints"]
        .as_array()
        .ok_or_else(|| err("no skin"))?
        .iter()
        .map(|j| j.as_u64().unwrap_or(0) as usize)
        .collect();
    if m.skin.iter().any(|&j| j >= m.nodes.len()) {
        return Err(err("a joint past the nodes"));
    }
    m.inverse_bind = match skin["inverseBindMatrices"].as_u64() {
        Some(i) => chunks::<16>(&floats(&json, bin, i as usize)?.0)
            .into_iter()
            .map(|f| std::array::from_fn(|c| std::array::from_fn(|r| f[c * 4 + r])))
            .collect(),
        None => vec![crate::math::IDENTITY; m.skin.len()],
    };
    if (0..m.level_count()).any(|k| {
        m.level(k)
            .joints
            .iter()
            .flatten()
            .any(|&j| usize::from(j) >= m.skin.len())
    }) {
        return Err(err("a vertex on a joint past the skin"));
    }

    for a in json["animations"].as_array().into_iter().flatten() {
        let samplers = a["samplers"].as_array().ok_or_else(|| err("no samplers"))?;
        let mut clip = Clip {
            name: a["name"].as_str().unwrap_or("").to_string(),
            duration: 0.0,
            channels: Vec::new(),
        };
        for c in a["channels"].as_array().into_iter().flatten() {
            let path = match c["target"]["path"].as_str() {
                Some("translation") => Path::Translation,
                Some("rotation") => Path::Rotation,
                Some("scale") => Path::Scale,
                _ => continue,
            };
            let Some(node) = c["target"]["node"].as_u64() else {
                continue;
            };
            let s = &samplers[c["sampler"].as_u64().unwrap_or(0) as usize];
            let times = floats(&json, bin, s["input"].as_u64().unwrap_or(0) as usize)?.0;
            let (values, _) = floats(&json, bin, s["output"].as_u64().unwrap_or(0) as usize)?;
            let linear = match s["interpolation"].as_str().unwrap_or("LINEAR") {
                "LINEAR" => true,
                "STEP" => false,
                i => return Err(format!("clip {}: {i} keys aren't read", clip.name)),
            };
            clip.duration = clip.duration.max(times.last().copied().unwrap_or(0.0));
            clip.channels.push(Channel {
                node: node as usize,
                path,
                linear,
                times,
                values,
            });
        }
        m.clips.push(clip);
    }
    Ok(m)
}

/// One mesh's primitives, joined.
fn geometry(json: &Value, bin: &[u8], mesh: &Value, skinned: bool) -> Result<Geometry, String> {
    let prims = mesh["primitives"]
        .as_array()
        .ok_or_else(|| err("no primitives"))?;
    let mut m = Geometry::default();
    for p in prims {
        let at = &p["attributes"];
        let get = |name: &str| -> Result<Vec<f32>, String> {
            let i = at[name].as_u64().ok_or_else(|| format!("no {name}"))?;
            Ok(floats(json, bin, i as usize)?.0)
        };
        let base = m.positions.len() as u32;
        let pos = chunks::<3>(&get("POSITION")?);
        let n = pos.len();
        m.positions.extend(pos);
        m.normals.extend(chunks::<3>(&get("NORMAL")?));
        match at["COLOR_0"].as_u64() {
            Some(i) => {
                let (c, w) = floats(json, bin, i as usize)?;
                m.colors
                    .extend(c.chunks_exact(w).map(|c| [c[0], c[1], c[2]]));
            }
            None => m.colors.extend(std::iter::repeat_n([1.0; 3], n)),
        }
        if skinned {
            m.joints.extend(
                chunks::<4>(&get("JOINTS_0")?)
                    .into_iter()
                    .map(|j| j.map(|v| v as u8)),
            );
            m.weights
                .extend(chunks::<4>(&get("WEIGHTS_0")?).into_iter().map(|w| {
                    let s: f32 = w.iter().sum();
                    if s > 0.0 {
                        w.map(|v| v / s)
                    } else {
                        [1.0, 0.0, 0.0, 0.0]
                    }
                }));
        } else {
            m.joints.extend(std::iter::repeat_n([0; 4], n));
            m.weights
                .extend(std::iter::repeat_n([1.0, 0.0, 0.0, 0.0], n));
        }
        let idx = p["indices"].as_u64().ok_or_else(|| err("no indices"))?;
        m.indices.extend(
            floats(json, bin, idx as usize)?
                .0
                .into_iter()
                .map(|i| base + i as u32),
        );
    }
    Ok(m)
}
