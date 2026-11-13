//! The big map's debug layers (docs/proposals/REALISTIC_WORLD.md R6, §7 q8: "also on the big map"):
//! a face's generator sampled on a grid for one of a small fixed vocabulary of fields, which the
//! layers in `shared/ui/map.json` `bigMap.layers` name and colour (data: the layers, their labels,
//! colours and ramps; code: the fields). The client's jobs worker runs it (`Job::MapLayer`), the
//! big map draws it over the revealed ground.
//!
//! Each sample is a category (an index into the grid's `names`), an id (outlines are drawn where
//! a neighbour's differs) and a value (m or mm). A field the world doesn't have (plates in a world
//! without them, rain without a climate) samples to nothing.

use crate::terrain::TerrainGenerator;

/// A field a layer can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapField {
    /// The face's plates: the category is the plate's kind (`oceanic`, `continental`), the id the
    /// plate (outlines at the boundaries). A world whose tectonics come from plates only.
    Plates,
    /// The boundaries' uplift (m: ranges and cones up, trenches and rifts down), from the plates.
    Uplift,
    /// Annual rain (mm) from the climate (the rain shadow). A world with a climate only.
    Rain,
    /// The rock at the surface (the category, by the geology's rock names, each with its
    /// `rocks.json` colour; the id the same): in a Realistic world the plate history's (R5).
    Rocks,
}

impl MapField {
    /// Every field, in the wire's order.
    pub const ALL: [Self; 4] = [Self::Plates, Self::Uplift, Self::Rain, Self::Rocks];

    /// Its name in the data (`bigMap.layers[].field`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Plates => "plates",
            Self::Uplift => "uplift",
            Self::Rain => "rain",
            Self::Rocks => "rocks",
        }
    }

    /// The field named `name`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }

    /// Its index on the wire.
    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|&f| f == self).expect("in ALL") as u8
    }

    /// The field of wire index `i`.
    pub fn of_index(i: u8) -> Option<Self> {
        Self::ALL.get(usize::from(i)).copied()
    }
}

/// A category without a colour of its own.
pub const NO_COLOR: u32 = u32::MAX;

/// A field sampled on an n × n grid (row-major, +x along a row, +z down the rows).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerGrid {
    pub n: usize,
    /// The categories' names (empty for a value field).
    pub names: Vec<String>,
    /// Each category's colour from its own data (0xRRGGBB; `NO_COLOR`: the layer's data colours it).
    pub colors: Vec<u32>,
    pub cats: Vec<u8>,
    pub ids: Vec<u8>,
    pub values: Vec<f32>,
}

impl LayerGrid {
    /// The wire form: `u8` name count, each name (`u8` length, UTF-8), each name's `u32`
    /// colour, `u32` n, then per sample
    /// its category (`u8`), id (`u8`) and value (`f32`), all little-endian.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.n * self.n * 6);
        out.push(self.names.len() as u8);
        for s in &self.names {
            let b = &s.as_bytes()[..s.len().min(255)];
            out.push(b.len() as u8);
            out.extend_from_slice(b);
        }
        for k in 0..self.names.len() {
            let c = self.colors.get(k).copied().unwrap_or(NO_COLOR);
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&(self.n as u32).to_le_bytes());
        for k in 0..self.n * self.n {
            out.push(self.cats[k]);
            out.push(self.ids[k]);
            out.extend_from_slice(&self.values[k].to_le_bytes());
        }
        out
    }
}

/// `field` of the face generator `g` on an n × n grid from face-local (x0, z0), `step` m apart;
/// None where the world doesn't have the field.
pub fn sample(
    g: &TerrainGenerator,
    field: MapField,
    x0: i64,
    z0: i64,
    step: i64,
    n: usize,
) -> Option<LayerGrid> {
    let mut grid = LayerGrid {
        n,
        names: Vec::new(),
        colors: Vec::new(),
        cats: vec![0; n * n],
        ids: vec![0; n * n],
        values: vec![0.0; n * n],
    };
    let at = |k: usize| (x0 + (k % n) as i64 * step, z0 + (k / n) as i64 * step);
    match field {
        MapField::Plates | MapField::Uplift => {
            let t = g.plate_tectonics()?;
            grid.names = match field {
                MapField::Plates => vec!["oceanic".into(), "continental".into()],
                _ => Vec::new(),
            };
            for k in 0..n * n {
                let (x, z) = at(k);
                let (x, z) = (x as f64, z as f64);
                if field == MapField::Plates {
                    let p = t.layout.plate_at(x, z);
                    grid.cats[k] = match t.layout.plates[p].kind {
                        crate::plates::PlateKind::Oceanic => 0,
                        crate::plates::PlateKind::Continental => 1,
                    };
                    grid.ids[k] = (p % 255) as u8 + 1;
                } else {
                    grid.values[k] = t.landform_exact(x, z) as f32;
                }
            }
        }
        MapField::Rain => {
            for k in 0..n * n {
                let (x, z) = at(k);
                grid.values[k] = g.biome_geography(x, z)?.rain? as f32;
            }
        }
        MapField::Rocks => {
            let rocks = g.geology().rocks;
            grid.names = rocks.iter().map(|r| r.name.to_owned()).collect();
            grid.colors = rocks.iter().map(|r| r.color).collect();
            for k in 0..n * n {
                let (x, z) = at(k);
                let rock = g.rock_at_surface(x, z).0;
                let i = rocks.iter().position(|r| r.name == rock.name).unwrap_or(0);
                grid.cats[k] = i.min(255) as u8;
                grid.ids[k] = grid.cats[k];
            }
        }
    }
    Some(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_round_trips_its_name_and_index() {
        for f in MapField::ALL {
            assert_eq!(MapField::parse(f.name()), Some(f));
            assert_eq!(MapField::of_index(f.index()), Some(f));
        }
        assert_eq!(MapField::parse("nope"), None);
    }

    #[test]
    fn the_wire_form_holds_names_and_samples() {
        let g = LayerGrid {
            n: 2,
            names: vec!["a".into(), "bc".into()],
            colors: vec![0x123456],
            cats: vec![0, 1, 1, 0],
            ids: vec![1, 2, 2, 1],
            values: vec![0.5, -1.0, 2.0, 3.0],
        };
        let b = g.encode();
        assert_eq!(&b[..6], &[2, 1, b'a', 2, b'b', b'c']);
        assert_eq!(u32::from_le_bytes(b[6..10].try_into().unwrap()), 0x123456);
        assert_eq!(u32::from_le_bytes(b[10..14].try_into().unwrap()), NO_COLOR);
        assert_eq!(u32::from_le_bytes(b[14..18].try_into().unwrap()), 2);
        assert_eq!(b.len(), 18 + 4 * 6);
        assert_eq!((b[18], b[19]), (0, 1));
        assert_eq!(f32::from_le_bytes(b[20..24].try_into().unwrap()), 0.5);
    }

    #[test]
    fn every_layer_in_the_data_names_a_field() {
        // shared/ui/mapLayers.json (validated by shared/ui/gen.mjs, which lists the same names).
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../../shared/ui/mapLayers.json")).unwrap();
        let layers = data["layers"].as_array().expect("layers");
        assert!(!layers.is_empty());
        for l in layers {
            let f = l["field"].as_str().expect("a field");
            assert!(MapField::parse(f).is_some(), "mapLayers.json: no field {f}");
        }
    }

    #[test]
    fn the_default_world_has_rocks_but_no_plates_or_rain() {
        // face-a-only: which fields a world has doesn't depend on the face (its generator's data).
        let g = TerrainGenerator::new(1);
        assert!(sample(&g, MapField::Plates, 0, 0, 1000, 4).is_none());
        assert!(sample(&g, MapField::Uplift, 0, 0, 1000, 4).is_none());
        assert!(sample(&g, MapField::Rain, 0, 0, 1000, 4).is_none());
        let r = sample(&g, MapField::Rocks, -8000, -8000, 4000, 4).expect("rocks everywhere");
        assert!(!r.names.is_empty());
        assert!(r.cats.iter().all(|&c| usize::from(c) < r.names.len()));
    }
}
