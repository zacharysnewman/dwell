//! A process builds a many-biome world's biome set once per face, however many generators it
//! makes of the world (BUGS.md: the starter world reloaded iOS Safari's tab while loading). A job
//! worker makes four (`dwell-client` `Runner::world_with`: its terrain, the chunk generator, the
//! LOD generator and the LOD bounds) and each built both faces' sets, twice (the build's, then the
//! worldgen's): 16 sets of whole-map grids (~60 MB a face natively) in each worker and the server.
//! Its own test binary: the count is the process's.

use dwell_world::{Face, GENERATOR_TERRAIN};
use dwell_worldgen::bifacial::{BifacialTerrain, face_seed};
use dwell_worldgen::config::worldgen_map_bytes;
use dwell_worldgen::terrain::biome_sets_built;

#[test]
fn a_worlds_generators_share_one_biome_set_per_face() {
    // both-faces: installs and generates the world's two faces together.
    let seed = 7;
    for f in [Face::A, Face::B] {
        let bytes =
            worldgen_map_bytes(face_seed(seed, f), "test:starter").expect("the starter's map");
        dwell_worldgen::worldmap::install(&bytes).expect("installs");
    }
    let before = biome_sets_built();
    // What a job worker makes of its world (`Runner::world_with`), and the spawn search.
    let _terrain = BifacialTerrain::for_generator(GENERATOR_TERRAIN, seed);
    let _chunks = dwell_worldgen::generator_for(GENERATOR_TERRAIN, seed);
    let _lod = dwell_worldgen::lod_generator_for(GENERATOR_TERRAIN, seed);
    let _bounds = dwell_worldgen::lod_bounds_for(GENERATOR_TERRAIN, seed);
    let _spawn = dwell_worldgen::spawn_point_for(GENERATOR_TERRAIN, seed);
    let built = biome_sets_built() - before;
    assert!(
        built <= 2,
        "five generators of one world built {built} biome sets (one per face expected)"
    );
}
