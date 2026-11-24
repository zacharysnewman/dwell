//! Probes the flood light down seed 0's first portal shaft, as the client lights it.
use dwell_world::{
    ChunkCoord, VoxelWorld,
    light::{ShaftSky, SkyTest, sky_of},
};
fn main() {
    let size = dwell_protocol::world_size::WorldSize::DEFAULT;
    let v = dwell_world::GENERATOR_TERRAIN;
    let shafts = dwell_worldgen::portal_shafts_for_sized(v, 0, size);
    let s = shafts.shafts[0];
    let air = dwell_worldgen::AirColumns::for_generator_sized(v, 0, size);
    let test: SkyTest = std::sync::Arc::new(move |c| air.known(c).unwrap_or(true));
    let sh = shafts.clone();
    let sky: ShaftSky = std::sync::Arc::new(move |c| sh.sky_cells(c));
    let mut w = VoxelWorld::new(dwell_worldgen::generator_for(v, 0), None);
    w.enable_light();
    let use_shaft = std::env::args().nth(1).as_deref() != Some("plain");
    {
        let l = w.light_mut().unwrap();
        l.set_sky_test(Some(test));
        if use_shaft {
            l.set_shaft_sky(Some(sky));
        }
    }
    let (ax, az) = (s.axis[0].floor() as i32, s.axis[1].floor() as i32);
    let cy = dwell_world::row_of_m(482.5).div_euclid(32);
    for dy in -1..=1 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let c = ChunkCoord::new(ax.div_euclid(32) + dx, cy + dy, az.div_euclid(32) + dz);
                w.get_voxel(c.x * 32, c.y * 32, c.z * 32);
                w.light_load(c);
            }
        }
    }
    let l = w.light().unwrap();
    for y in [960, 965, 970] {
        let row: Vec<String> = (-4..=4)
            .map(|d| format!("{:2}", l.get([ax + d, y, az]).map_or(99, sky_of)))
            .collect();
        println!("row {y} sky across x: {}", row.join(" "));
    }
}
