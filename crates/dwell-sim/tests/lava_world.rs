//! A world's own lava vents (docs/proposals/VOLCANOES.md §6.2): the starter world
//! (`test:starter`, `shared/worldgen/worlds/starter.json`) lists a lake vent on its youngest
//! hotspot island's summit (`tectonics.lavaVents`), placed by the server on each face when the
//! world is created, and lava stands round the vent after a while with a player beside it. Its own
//! test binary: the starter's maps are installed process-wide.

mod common;

use std::sync::OnceLock;

use common::*;
use dwell_blocks::require;
use dwell_lava::field::row_at;
use dwell_protocol::*;
use dwell_sim::server::ServerConfig;
use dwell_world::{Face, FaceFrame, GENERATOR_TERRAIN};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};

const SEED: u64 = 0x1a7a_5eed;
const NAME: &str = "test:starter";

/// The starter's maps on both faces, installed once.
fn install() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for face in [Face::A, Face::B] {
            let bytes =
                dwell_worldgen::config::worldgen_map_bytes(face_seed(SEED, face), NAME).unwrap();
            dwell_worldgen::worldmap::install(&bytes).unwrap();
        }
    });
}

/// The starter's vent site (face-local column) and the ground's height there on face `f`.
fn vent(f: FaceFrame) -> ([i32; 2], f64) {
    let sites = dwell_worldgen::config::lava_vent_sites(NAME).unwrap();
    assert_eq!(sites.len(), 1, "the starter lists one vent");
    let [x, z] = sites[0].1;
    let c = [x.floor() as i32, z.floor() as i32];
    let t = BifacialTerrain::for_generator(GENERATOR_TERRAIN, SEED).unwrap();
    let y = t
        .local(f.face)
        .surface_y_at(c[0], c[1])
        .expect("on the disc");
    (c, y)
}

#[test]
fn the_starter_world_lists_a_lake_vent_on_its_youngest_hotspot_and_the_default_none() {
    // face-a-only: the data, no terrain.
    use dwell_worldgen::tectonics::{Cone, LavaVentKind};
    let sites = dwell_worldgen::config::lava_vent_sites(NAME).unwrap();
    assert_eq!(sites.len(), 1);
    let (v, [x, z]) = &sites[0];
    assert_eq!((v.cone, v.kind), (Cone::Hotspot(0), LavaVentKind::Lake));
    assert_eq!(v.fluid, "dwell:lava");
    // The youngest hotspot island's centre (starter.json hotspotX, hotspotZ).
    assert_eq!([*x, *z], [30000.0, -8000.0]);
    assert!(
        dwell_worldgen::config::lava_vent_sites("")
            .unwrap()
            .is_empty()
    );
    assert!(
        dwell_worldgen::config::lava_vent_sites("test:tall_peak")
            .unwrap()
            .is_empty()
    );
}

dwell_world::both_faces!(
    fn the_starter_world_gets_its_vent_and_lava_stands_on_the_summit(f: FaceFrame) {
        install();
        let (c, y) = vent(f);
        assert!(y > 50.0, "the island stands out of the sea: {y} m");
        let mut fx = Fixture::new(ServerConfig {
            generator_version: GENERATOR_TERRAIN,
            world_seed: SEED,
            // Beside the vent, a few metres off, on the summit.
            spawn: Some(f.point([f64::from(c[0]) + 6.5, y + 1.0, f64::from(c[1]) + 0.5])),
            mode: "creative".into(),
            ..flat()
        });
        assert_eq!(fx.server.worldgen_text(), NAME);
        let fields = &fx.server.lava().fields;
        assert_eq!(fields.len(), 2, "a vent on each face");
        let i = fields
            .iter()
            .position(|p| p.face == f)
            .expect("this face's");
        let r = fields[i].field.w / 2;
        let o = fields[i].field.origin;
        assert_eq!([o[0] + r as i32, o[1] + r as i32], c, "centred on the vent");
        fx.join(1, &Client::new(1));
        for _ in 0..20 * SIM_HZ {
            fx.server.step();
        }
        let p = &fx.server.lava().fields[i];
        let at = p.field.at(r, r);
        assert!(p.field.depth[at] > 0.0, "lava over the vent");
        let bed = row_at(p.field.ground[at]);
        let [wx, wy, wz] = f.cell([c[0], bed, c[1]]);
        assert_eq!(
            f.state(fx.server.world().get_voxel(wx, wy, wz)),
            require("dwell:lava"),
            "and in the chunks"
        );
    }
);
