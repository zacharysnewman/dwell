//! Prints the spawn search points `existing_seeds_spawn_where_they_did` pins (seeds 0 and 7, both
//! faces, face B's height face-local), to re-pin them after a terrain change.
fn main() {
    for seed in [0u64, 7] {
        let t = dwell_worldgen::BifacialTerrain::new(seed);
        let a = t.local(dwell_world::Face::A).spawn_search_point();
        let [x, y, z] = t.local(dwell_world::Face::B).spawn_search_point();
        println!("({seed}, {a:?}, {:?}),", [x, dwell_world::FaceFrame::B.y(y), z]);
    }
}
