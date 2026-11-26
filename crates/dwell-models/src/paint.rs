//! A player's paint (the Character painter, docs/CHARACTERS.md §9.4; owner, 2026-11-26): seven colour
//! slots, each a palette ramp ([`crate::PAINT_RAMPS`]), and a slot per face of the player model's
//! painted level ([`PAINT_LEVEL`], the 60 % mesh every view draws). Changing a slot recolours every face
//! painted with it. Other levels take each face's slot from the painted level's nearest face.

use crate::glb::{Model, View};
use crate::{ModelData, PAINT_RAMPS, ramp_base};

/// The colour slots a paint has.
pub const PAINT_SLOTS: usize = 7;
/// The detail level faces are painted on (the 60 % mesh: other players, your shadow, the helm's view).
pub const PAINT_LEVEL: usize = crate::DETAIL_DEFAULT;

/// A player's paint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paint {
    /// Each slot's ramp (a name of [`PAINT_RAMPS`]).
    pub slots: [String; PAINT_SLOTS],
    /// Each face's slot (0..PAINT_SLOTS) at [`PAINT_LEVEL`], in index order.
    pub faces: Vec<u8>,
}

/// Each triangle's centre at a level.
pub fn centres(m: &View<'_>) -> Vec<[f32; 3]> {
    m.indices
        .chunks_exact(3)
        .map(|t| {
            let p: [[f32; 3]; 3] = std::array::from_fn(|k| m.positions[t[k] as usize]);
            std::array::from_fn(|a| (p[0][a] + p[1][a] + p[2][a]) / 3.0)
        })
        .collect()
}

/// The faces at the painted level.
pub fn face_count(model: &Model) -> usize {
    model.level(PAINT_LEVEL).indices.len() / 3
}

fn rgb(c: u32) -> [f32; 3] {
    [16, 8, 0].map(|s| ((c >> s) & 0xff) as f32 / 255.0)
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

/// The paint ramp nearest an sRGB colour (0..1).
pub fn nearest_ramp(c: [f32; 3]) -> &'static str {
    PAINT_RAMPS
        .iter()
        .copied()
        .min_by(|a, b| {
            let d = |n: &str| dist2(rgb(ramp_base(n).unwrap_or(0)), c);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or("stone")
}

/// The paint a player starts from: the model as it is drawn. Its skin, hair, hat, shirt and
/// trousers parts take the first five slots (each the ramp nearest its commonest colour); the
/// last two take the two commonest colours of every other face (the bands' pattern, the boots, the
/// faces of no part), each such face then the slot nearest its own colour.
pub fn default_paint(data: &ModelData, model: &Model) -> Paint {
    const PARTS: [&str; 5] = ["skin", "hair", "hat", "shirt", "trousers"];
    const FALLBACK: [&str; PAINT_SLOTS] = [
        "skin_fair",
        "hair_brown",
        "heartwood",
        "wool_green",
        "flint",
        "leather",
        "wool_red",
    ];
    let m = model.level(PAINT_LEVEL);
    let parts = data.parts.map(|p| (p, p.level(PAINT_LEVEL)));
    let colour = |v: usize| m.colors[v].map(crate::linear_to_srgb);
    let face_part = |t: &[u32]| -> Option<usize> {
        let (p, levels) = parts.as_ref()?;
        let n = *levels.get(t[0] as usize)?;
        PARTS
            .iter()
            .position(|name| n > 0 && p.number(name) == Some(n))
    };
    let tris: Vec<&[u32]> = m.indices.chunks_exact(3).collect();
    // The commonest ramp (each face's colour snapped) among `faces`, the `skip`-th most.
    let commonest =
        |faces: &mut dyn Iterator<Item = &&[u32]>, taken: &[String]| -> Option<&'static str> {
            let mut counts: Vec<(&'static str, usize)> = Vec::new();
            for t in faces {
                let r = nearest_ramp(colour(t[0] as usize));
                match counts.iter_mut().find(|(x, _)| *x == r) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((r, 1)),
                }
            }
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            counts
                .iter()
                .map(|(r, _)| *r)
                .find(|r| !taken.iter().any(|t| t == r))
        };
    let mut slots: [String; PAINT_SLOTS] = std::array::from_fn(|k| FALLBACK[k].to_string());
    for (k, slot) in slots.iter_mut().enumerate().take(PARTS.len()) {
        if let Some(r) = commonest(&mut tris.iter().filter(|t| face_part(t) == Some(k)), &[]) {
            *slot = r.to_string();
        }
    }
    // The last two: the leftover faces' commonest colours that no slot has yet.
    for k in PARTS.len()..PAINT_SLOTS {
        let taken = slots[..k].to_vec();
        if let Some(r) = commonest(&mut tris.iter().filter(|t| face_part(t).is_none()), &taken) {
            slots[k] = r.to_string();
        }
    }
    let slot_rgb: Vec<[f32; 3]> = slots
        .iter()
        .map(|s| rgb(ramp_base(s).unwrap_or(0)))
        .collect();
    let faces = tris
        .iter()
        .map(|t| {
            face_part(t).unwrap_or_else(|| {
                let c = colour(t[0] as usize);
                (0..PAINT_SLOTS)
                    .min_by(|&a, &b| dist2(slot_rgb[a], c).total_cmp(&dist2(slot_rgb[b], c)))
                    .unwrap_or(0)
            }) as u8
        })
        .collect();
    Paint { slots, faces }
}

impl Paint {
    /// Whether it fits the model: a slot per painted face, each slot a paint ramp.
    pub fn fits(&self, model: &Model) -> bool {
        self.faces.len() == face_count(model)
            && self.faces.iter().all(|&s| usize::from(s) < PAINT_SLOTS)
            && self.slots.iter().all(|s| PAINT_RAMPS.contains(&s.as_str()))
    }

    /// Each face's slot at `level`: the painted level's own, else its nearest face's.
    pub fn level_faces(&self, model: &Model, level: usize) -> Vec<u8> {
        if level == PAINT_LEVEL {
            return self.faces.clone();
        }
        let from = centres(&model.level(PAINT_LEVEL));
        centres(&model.level(level))
            .iter()
            .map(|c| {
                (0..from.len())
                    .min_by(|&a, &b| dist2(from[a], *c).total_cmp(&dist2(from[b], *c)))
                    .and_then(|i| self.faces.get(i).copied())
                    .unwrap_or(0)
            })
            .collect()
    }

    /// Each slot's colour (sRGB bytes).
    pub fn slot_bytes(&self) -> [[u8; 3]; PAINT_SLOTS] {
        std::array::from_fn(|k| {
            let c = ramp_base(&self.slots[k]).unwrap_or(0);
            [(c >> 16) as u8, (c >> 8) as u8, c as u8]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_paint_draws_the_player_as_the_model_does() {
        // face-a-only: model space, no face frame. Owner, 2026-11-26: paint "starts from the model's
        // current colours".
        let (data, model) = crate::model(crate::PLAYER).unwrap();
        let p = default_paint(data, model);
        assert!(p.fits(model), "{:?}", p.slots);
        assert_eq!(p.faces.len(), face_count(model));
        assert_eq!(p.slots[0], "skin_fair");
        assert_eq!(p.slots[1], "hair_brown");
        // Every slot is used, and each painted face's slot colour is near its modelled colour.
        let m = model.level(PAINT_LEVEL);
        let bytes = p.slot_bytes();
        let mut used = [0usize; PAINT_SLOTS];
        let mut far = 0;
        for (t, &s) in m.indices.chunks_exact(3).zip(&p.faces) {
            used[usize::from(s)] += 1;
            let c = m.colors[t[0] as usize].map(crate::linear_to_srgb);
            let d = dist2(bytes[usize::from(s)].map(|v| f32::from(v) / 255.0), c).sqrt();
            far += usize::from(d > 0.35);
        }
        assert!(used.iter().all(|&n| n > 0), "{used:?} {:?}", p.slots);
        assert!(
            far * 20 < p.faces.len(),
            "{far} of {} faces far from their colour",
            p.faces.len()
        );
    }

    #[test]
    fn paint_carries_to_every_level() {
        // face-a-only: model space.
        let (data, model) = crate::model(crate::PLAYER).unwrap();
        let mut p = default_paint(data, model);
        p.faces.iter_mut().for_each(|f| *f = 3);
        for level in 0..crate::DETAIL_LEVELS.len() {
            let faces = p.level_faces(model, level);
            assert_eq!(faces.len(), model.level(level).indices.len() / 3);
            assert!(faces.iter().all(|&f| f == 3));
        }
    }
}
