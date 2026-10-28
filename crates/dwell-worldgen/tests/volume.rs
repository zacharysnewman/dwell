//! The terrain volume (the cliffs over the height field, the terrain's default; `volume.rs`): overhangs on steep
//! ground, the height field untouched elsewhere, nothing left standing on air.

use dwell_world::{CHUNK_SIZE, Chunk, ChunkCoord, FaceFrame, M, local_index};
use dwell_worldgen::volume::{Node, VolumeDef};
use dwell_worldgen::{BifacialTerrain, TerrainGenerator};

/// The first `n` columns, at least 200 m apart, on a 16 m grid round the origin whose ground falls
/// more than `slope` per metre over ±3 m (the cliff mask's radius), from face-local generator `g`.
fn steep_columns(g: &TerrainGenerator, slope: f32, n: usize) -> Vec<(i32, i32)> {
    let h = |x: i32, z: i32| g.column_at(x, z).height;
    let mut found: Vec<(i32, i32)> = Vec::new();
    for (x, z) in (0..400).flat_map(|j| (0..400).map(move |i| ((i - 200) * 16, (j - 200) * 16))) {
        let sx = (h(x + 3, z) - h(x - 3, z)) / 6.0;
        let sz = (h(x, z + 3) - h(x, z - 3)) / 6.0;
        let apart = found
            .iter()
            .all(|&(a, b)| (a - x).abs().max((b - z).abs()) >= 200);
        if apart && !g.column_at(x, z).outside && (sx * sx + sz * sz).sqrt() > slope {
            found.push((x, z));
            if found.len() == n {
                break;
            }
        }
    }
    assert_eq!(found.len(), n, "steep ground near the origin");
    found
}

fn steep_column(g: &TerrainGenerator, slope: f32) -> (i32, i32) {
    steep_columns(g, slope, 1)[0]
}

/// The face-local chunks round column (x, z) from its ground 12 m down to 12 m up, from both
/// generators: (coordinate, plain terrain's, the volume's).
fn chunks_round(
    plain: &TerrainGenerator,
    cliffs: &TerrainGenerator,
    (x, z): (i32, i32),
) -> Vec<(ChunkCoord, Chunk, Chunk)> {
    let ground = plain.ground_y(x, z).expect("inside the disc");
    let rows = (ground - 24).div_euclid(CHUNK_SIZE)..=(ground + 24).div_euclid(CHUNK_SIZE);
    let (cx, cz) = (x.div_euclid(CHUNK_SIZE), z.div_euclid(CHUNK_SIZE));
    let mut out = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            for cy in rows.clone() {
                let c = ChunkCoord::new(cx + dx, cy, cz + dz);
                let (mut a, mut b) = (Chunk::default(), Chunk::default());
                plain.generate(c, &mut a);
                cliffs.generate(c, &mut b);
                out.push((c, a, b));
            }
        }
    }
    out
}

/// A volume with big overhangs, by data: 0.34–0.37's cliffs, walls on a 2D noise's zero line
/// leaned by a 3D noise, so they hang far out over their foot.
static OVERHANGS: VolumeDef = VolumeDef {
    name: "overhangs",
    reach: 16.0,
    material: "dwell:stone",
    soil_depth: 3.0,
    floor_grass: 3.0,
    strength: 1.0,
    nodes: &[
        Node::Depth,
        Node::Strength,
        Node::Noise {
            wavelength: 600,
            octaves: 2,
            stretch_y: 0.0,
            stream: 1,
        },
        Node::Smoothstep {
            of: 2,
            from: -0.2,
            to: 0.1,
        },
        Node::Water,
        Node::Smoothstep {
            of: 4,
            from: 8.0,
            to: 30.0,
        },
        Node::Slope { radius: 4 },
        Node::Smoothstep {
            of: 6,
            from: 0.3,
            to: 0.8,
        },
        Node::Const(14.0),
        Node::Const(2.0),
        Node::Noise {
            wavelength: 40,
            octaves: 2,
            stretch_y: 0.0,
            stream: 2,
        },
        Node::Const(0.5),
        Node::Noise {
            wavelength: 40,
            octaves: 1,
            stretch_y: 1.0,
            stream: 3,
        },
        Node::Mul(&[11, 12]),
        Node::Add(&[10, 13]),
        Node::Smoothstep {
            of: 14,
            from: -0.02,
            to: 0.02,
        },
        Node::Mul(&[9, 15]),
        Node::Const(-1.0),
        Node::Add(&[16, 17]),
        Node::Mul(&[1, 3, 5, 7, 8, 18]),
        Node::Add(&[0, 19]),
    ],
};

fn solid(m: dwell_blocks::StateId) -> bool {
    m != M.air && m != M.water && !dwell_blocks::info(m).replaceable
}

dwell_world::both_faces!(
    /// The owner (2026-10-27): cliffs and overhangs. On steep ground the cliffs volume reshapes the
    /// surface in 3D: somewhere a solid cell stands over air (an overhang), and the volume both adds
    /// cells (ledges) and removes them (recesses). Over several hillsides: a 600 m field picks which
    /// part of the land has cliffs, so one hillside may have none.
    fn steep_ground_has_overhangs(f: FaceFrame) {
        let plain = BifacialTerrain::height_field(7);
        let cliffs = BifacialTerrain::new(7);
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let at = steep_columns(plain, 1.1, 6);
        let (mut overhangs, mut added, mut removed) = (0, 0, 0);
        for (_, a, b) in at.iter().flat_map(|&c| chunks_round(plain, cliffs, c)) {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        let (pa, pb) = (
                            a.voxels()[local_index(x, y, z)],
                            b.voxels()[local_index(x, y, z)],
                        );
                        added += usize::from(!solid(pa) && solid(pb));
                        removed += usize::from(solid(pa) && !solid(pb));
                        if y > 0 && solid(pb) && !solid(b.voxels()[local_index(x, y - 1, z)]) {
                            overhangs += 1;
                        }
                    }
                }
            }
        }
        assert!(
            added > 50 && removed > 50,
            "added {added}, removed {removed} round {at:?}"
        );
        assert!(
            overhangs > 10,
            "{overhangs} solid cells over air round {at:?}"
        );
    }
);

dwell_world::both_faces!(
    /// Away from steep ground the cliffs world is the plain terrain, cell for cell: the volume
    /// only reshapes where its mask is on.
    fn gentle_ground_is_the_plain_terrain(f: FaceFrame) {
        let plain = BifacialTerrain::height_field(7);
        let cliffs = BifacialTerrain::new(7);
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let spawn = plain
            .ground_y(0, 0)
            .map(|_| (0, 0))
            .expect("the origin is inside the disc");
        for (c, a, b) in chunks_round(plain, cliffs, spawn) {
            // Only chunks whose ground is gentle everywhere (the mask's lower edge, 0.6).
            let gentle = (0..CHUNK_SIZE).all(|z| {
                (0..CHUNK_SIZE).all(|x| {
                    let (wx, wz) = (c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z);
                    let h = |dx: i32, dz: i32| plain.column_at(wx + dx, wz + dz).height;
                    let sx = (h(3, 0) - h(-3, 0)) / 6.0;
                    let sz = (h(0, 3) - h(0, -3)) / 6.0;
                    (sx * sx + sz * sz).sqrt() < 0.6
                })
            });
            if gentle {
                assert!(
                    a.voxels() == b.voxels(),
                    "chunk {c:?} differs on gentle ground"
                );
            }
        }
    }
);

dwell_world::both_faces!(
    /// Nothing the generator puts on the ground (grass, flowers) is left over air where the volume
    /// removed the cell under it.
    fn no_decoration_floats_over_a_recess(f: FaceFrame) {
        let plain = BifacialTerrain::height_field(7);
        let cliffs = BifacialTerrain::new(7);
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let at = steep_column(plain, 0.8);
        for (c, _, b) in chunks_round(plain, cliffs, at) {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 1..CHUNK_SIZE {
                        let m = b.voxels()[local_index(x, y, z)];
                        let decoration =
                            m != M.air && m != M.water && dwell_blocks::info(m).replaceable;
                        let under = b.voxels()[local_index(x, y - 1, z)];
                        assert!(
                            !decoration || under != M.air,
                            "a decoration over air at {:?} + ({x}, {y}, {z})",
                            c
                        );
                    }
                }
            }
        }
    }
);

dwell_world::both_faces!(
    /// Carving a dry hillside leaves air, not water: the water row of a column with no water is just
    /// over its ground, so every recess cut below a hillside's surface had filled with water that
    /// hung on the cliff (seen in Chrome by a lake at seed 7, 2026-10-27).
    fn a_recess_in_dry_ground_is_air(f: FaceFrame) {
        let plain = BifacialTerrain::height_field(7);
        let cliffs = BifacialTerrain::new(7);
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let mut wrong = 0;
        for at in steep_columns(plain, 1.1, 6) {
            for (c, a, b) in chunks_round(plain, cliffs, at) {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let col = plain.column_at(c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z);
                        if col.water != dwell_worldgen::terrain::Water::None {
                            continue;
                        }
                        for y in 0..CHUNK_SIZE {
                            let i = local_index(x, y, z);
                            wrong +=
                                usize::from(b.voxels()[i] == M.water && a.voxels()[i] != M.water);
                        }
                    }
                }
            }
        }
        assert_eq!(wrong, 0, "water cells carved into dry ground");
    }
);

/// Each cliffs chunk round six steep hillsides of face `f`, with its column's cells: (coordinate,
/// the cliffs chunk).
fn cliff_chunks(f: FaceFrame) -> Vec<(ChunkCoord, Chunk)> {
    let plain = BifacialTerrain::height_field(7);
    let cliffs = BifacialTerrain::new(7);
    let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
    steep_columns(plain, 1.1, 6)
        .into_iter()
        .flat_map(|c| chunks_round(plain, cliffs, c))
        .map(|(c, _, b)| (c, b))
        .collect()
}

/// The cell at chunk-local (x, y, z) of `chunks`' column stack, `y` past the chunk's bottom or top
/// read from the chunk below or above (`None` past the stack).
fn cell(
    chunks: &[(ChunkCoord, Chunk)],
    c: ChunkCoord,
    x: i32,
    y: i32,
    z: i32,
) -> Option<dwell_blocks::StateId> {
    let cy = c.y + y.div_euclid(CHUNK_SIZE);
    let ly = y.rem_euclid(CHUNK_SIZE);
    chunks
        .iter()
        .find(|(k, _)| k.x == c.x && k.z == c.z && k.y == cy)
        .map(|(_, ch)| ch.voxels()[local_index(x, ly, z)])
}

fn named(m: dwell_blocks::StateId) -> &'static str {
    dwell_blocks::info(m).name
}

dwell_world::both_faces!(
    /// The owner (2026-10-27): the cliffs were applied after the grass, the dirt and the trees, so
    /// the old surface showed as a stripe of grass and dirt inside the rock. No grass lies buried:
    /// every grass cell of the reshaped ground has air, a plant, a tree or the upper half of its own
    /// steep ramp over it.
    fn no_grass_is_buried_in_the_rock(f: FaceFrame) {
        let chunks = cliff_chunks(f);
        let mut buried = 0;
        for (c, ch) in &chunks {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        if named(ch.voxels()[local_index(x, y, z)]).starts_with("dwell:grass")
                            && cell(&chunks, *c, x, y + 1, z).is_some_and(|m| {
                                let n = named(m);
                                // Its own steep ramp's upper half, or a tree standing on it.
                                solid(m)
                                    && !n.starts_with("dwell:grass")
                                    && !n.starts_with("dwell:log")
                                    && n != "dwell:leaves"
                            })
                        {
                            buried += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(buried, 0, "grass cells under solid ground");
    }
);

dwell_world::both_faces!(
    /// The owner: "mostly stone at the surface now, instead of grass". The ground the volume
    /// raises is topped as the land round it: most tops open to the sky over a raised or lowered
    /// column are grass, as on the plain terrain there.
    fn the_reshaped_ground_is_grassed_like_the_land(f: FaceFrame) {
        let plain = BifacialTerrain::height_field(7);
        let cliffs = BifacialTerrain::new(7);
        let (plain, cliffs) = (plain.local(f.face), cliffs.local(f.face));
        let (mut tops, mut grass) = (0, 0);
        for at in steep_columns(plain, 1.1, 6) {
            for dz in -24..=24 {
                for dx in -24..=24 {
                    let (x, z) = (at.0 + dx, at.1 + dz);
                    let (Some(a), Some(b)) = (plain.ground_y(x, z), cliffs.ground_y(x, z)) else {
                        continue;
                    };
                    if a == b || !named(plain.state_at(x, a, z)).starts_with("dwell:grass") {
                        continue;
                    }
                    tops += 1;
                    grass +=
                        usize::from(named(cliffs.state_at(x, b, z)).starts_with("dwell:grass"));
                }
            }
        }
        assert!(tops > 200, "reshaped grass columns: {tops}");
        assert!(
            grass * 10 >= tops * 9,
            "{grass} of {tops} reshaped tops are grass"
        );
    }
);

dwell_world::both_faces!(
    /// The owner: floating trees. A tree's trunk stands on solid ground: no log cell has air
    /// under it unless another log or the tree's own block is there.
    fn trees_stand_on_the_reshaped_ground(f: FaceFrame) {
        let chunks = cliff_chunks(f);
        let (mut trunks, mut floating) = (0, Vec::new());
        for (c, ch) in &chunks {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 0..CHUNK_SIZE {
                        if !named(ch.voxels()[local_index(x, y, z)]).starts_with("dwell:log") {
                            continue;
                        }
                        let below = cell(&chunks, *c, x, y - 1, z);
                        if below.is_some_and(|m| !named(m).starts_with("dwell:log")) {
                            trunks += 1;
                            if !below.is_some_and(solid) {
                                floating.push((*c, x, y, z));
                            }
                        }
                    }
                }
            }
        }
        assert!(trunks > 5, "trunks round the hillsides: {trunks}");
        assert!(
            floating.is_empty(),
            "{} of {trunks} trunks over air, e.g. {:?}",
            floating.len(),
            floating.first()
        );
    }
);

dwell_world::both_faces!(
    /// A cliff face shows rock, not a wall of soil: no dirt cell lies more than the volume's
    /// `soilDepth` under its column's top (the soil mantle followed the lowest neighbour down a
    /// 14 m face), or the height field's own mantle there where that is deeper.
    fn cliff_faces_show_rock_below_the_soil(f: FaceFrame) {
        let cliffs = BifacialTerrain::new(7);
        let g = cliffs.local(f.face);
        let soil = (f64::from(
            dwell_worldgen::volume::find("cliffs")
                .expect("cliffs")
                .soil_depth,
        ) / dwell_world::CELL_H) as i32;
        let plain = BifacialTerrain::height_field(7);
        let p = plain.local(f.face);
        // The height field's own soil under a top: down to its lowest neighbour's top.
        let mantle = |x: i32, z: i32| {
            let top = p.ground_y(x, z).expect("disc");
            let lowest = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter_map(|(dx, dz)| p.ground_y(x + dx, z + dz))
                .min()
                .unwrap_or(top);
            top - lowest
        };
        let chunks = cliff_chunks(f);
        let mut deep = 0;
        for (c, ch) in &chunks {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let (wx, wz) = (c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z);
                    let top = g.ground_y(wx, wz).expect("disc");
                    let allowed = soil.max(mantle(wx, wz));
                    for y in 0..CHUNK_SIZE {
                        let wy = c.y * CHUNK_SIZE + y;
                        if named(ch.voxels()[local_index(x, y, z)]).starts_with("dwell:dirt")
                            && top - wy > allowed
                        {
                            deep += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(deep, 0, "dirt cells deeper than the soil under a top");
    }
);

dwell_world::both_faces!(
    /// The owner (2026-10-27): "the overhangs are forming shallow caves and tunnels". A 3D noise
    /// varying faster up than across layered rock and air through the cliffs' band; walls come from
    /// a 2D switch now, leaned by a slow 3D term, so a column under its top has at most one gap of
    /// air (an overhang over its foot), never rock, air, rock and air again (a tunnel in the face).
    fn cliffs_are_walls_not_tunnels(f: FaceFrame) {
        let chunks = cliff_chunks(f);
        let mut stacks = std::collections::HashMap::<(i32, i32), Vec<(i32, bool)>>::new();
        for (c, ch) in &chunks {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let s = stacks
                        .entry((c.x * CHUNK_SIZE + x, c.z * CHUNK_SIZE + z))
                        .or_default();
                    for y in 0..CHUNK_SIZE {
                        let m = ch.voxels()[local_index(x, y, z)];
                        // The ground: rock, soil and water, not a tree's canopy over the air.
                        let ground = (solid(m)
                            && !(named(m).starts_with("dwell:log") || named(m) == "dwell:leaves"))
                            || m == M.water;
                        s.push((c.y * CHUNK_SIZE + y, ground));
                    }
                }
            }
        }
        let (mut columns, mut tunnels) = (0usize, Vec::new());
        for (at, mut s) in stacks {
            s.sort_by_key(|&(y, _)| std::cmp::Reverse(y));
            // Gaps of air under the first solid cell, going down.
            let mut gaps = 0;
            let mut seen_solid = false;
            for w in s.windows(2) {
                let ((_, a), (_, b)) = (w[0], w[1]);
                seen_solid |= a;
                gaps += usize::from(seen_solid && a && !b);
            }
            columns += 1;
            if gaps > 1 {
                tunnels.push(at);
            }
        }
        assert!(columns > 1000, "columns checked: {columns}");
        assert!(
            tunnels.len() * 1000 <= columns,
            "{} of {columns} columns hold more than one gap of air, e.g. {:?}",
            tunnels.len(),
            &tunnels[..tunnels.len().min(3)]
        );
    }
);

/// Columns round seed `seed`'s rivers on face `f` (every 6th course point of the first 60
/// courses, out to `out` m past the channel), with the plain and the cliffs generators.
fn river_columns(
    seed: u64,
    f: FaceFrame,
    out: i32,
    mut each: impl FnMut(&TerrainGenerator, &TerrainGenerator, i32, i32),
) {
    let plain = BifacialTerrain::height_field(seed);
    let cliffs = BifacialTerrain::new(seed);
    let (p, c) = (plain.local(f.face), cliffs.local(f.face));
    for course in p.map().channels.courses.iter().take(60) {
        for pt in course.iter().step_by(6) {
            let r = (pt.w * 0.5) as i32 + out;
            for z in pt.z as i32 - r..=pt.z as i32 + r {
                for x in pt.x as i32 - r..=pt.x as i32 + r {
                    each(p, c, x, z);
                }
            }
        }
    }
}

dwell_world::both_faces!(
    /// The owner (2026-10-27, 0.34.0): "big edges rising up on sides of river, and drop off edges
    /// without filling low areas". A river valley's banks are steep, so the cliffs' slope mask put
    /// walls along them, raised above the water or sunk below it. The cliffs keep off the water:
    /// within 8 m of a river channel's edge or a lake's shore the ground is the plain terrain's.
    fn cliffs_keep_off_the_rivers(f: FaceFrame) {
        let (mut near, mut moved) = (0usize, Vec::new());
        river_columns(7, f, 10, |p, c, x, z| {
            let col = p.column_at(x, z);
            if col.outside || col.shore >= 8.0 {
                return;
            }
            near += 1;
            if p.ground_y(x, z) != c.ground_y(x, z) {
                moved.push((x, z));
            }
        });
        assert!(near > 5000, "columns by the water: {near}");
        assert!(
            moved.is_empty(),
            "{} of {near} columns by the water reshaped, e.g. {:?}",
            moved.len(),
            moved.first()
        );
    }
);

dwell_world::both_faces!(
    /// No water stands open beside ground the cliffs lowered: round the rivers, the cliffs world
    /// has no more water cells beside air (sideways) than the plain terrain.
    fn no_water_stands_open_beside_lowered_ground(f: FaceFrame) {
        let open = |g: &TerrainGenerator, x: i32, z: i32| {
            let col = g.column_at(x, z);
            if col.outside || col.water == dwell_worldgen::terrain::Water::None {
                return 0;
            }
            let surface = col.water_row();
            [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter(|(dx, dz)| {
                    let n = g.column_at(x + dx, z + dz);
                    !n.outside
                        && n.water == dwell_worldgen::terrain::Water::None
                        && g.ground_y(x + dx, z + dz).is_some_and(|t| t < surface - 1)
                })
                .count()
        };
        let (mut plain, mut cliffs) = (0usize, 0usize);
        river_columns(7, f, 6, |p, c, x, z| {
            plain += open(p, x, z);
            cliffs += open(c, x, z);
        });
        assert!(
            cliffs <= plain,
            "open water faces: cliffs {cliffs}, plain {plain}"
        );
    }
);

#[test]
fn cliffs_keep_off_the_river_at_the_report() {
    // face-a-only: the owner's report, seed 3764472973 face A near (-3794, 240, -5614) (0.34.0).
    let plain = BifacialTerrain::height_field(3_764_472_973);
    let cliffs = BifacialTerrain::new(3_764_472_973);
    let (p, c) = (
        plain.local(FaceFrame::A.face),
        cliffs.local(FaceFrame::A.face),
    );
    let (mut near, mut moved) = (0, 0);
    for z in -5714..=-5514 {
        for x in -3894..=-3694 {
            let col = p.column_at(x, z);
            if col.outside || col.shore >= 8.0 {
                continue;
            }
            near += 1;
            moved += usize::from(p.ground_y(x, z) != c.ground_y(x, z));
        }
    }
    assert!(near > 100, "columns by the water: {near}");
    assert_eq!(moved, 0, "of {near} columns by the water");
}

dwell_world::both_faces!(
    /// The owner (2026-10-27): the cliffs are the default terrain, configurable ("this will most
    /// likely be a slider" with biomes). At strength 0 the volume leaves the height field as it is,
    /// cell for cell, by data alone.
    fn at_strength_zero_the_terrain_is_the_height_field(f: FaceFrame) {
        let mut def = *dwell_worldgen::volume::find("cliffs").expect("cliffs");
        def.strength = 0.0;
        let none = BifacialTerrain::with_volume(7, Box::leak(Box::new(def)));
        let plain = BifacialTerrain::height_field(7);
        let (p, z) = (plain.local(f.face), none.local(f.face));
        for at in steep_columns(p, 1.1, 2) {
            for (c, a, b) in chunks_round(p, z, at) {
                assert!(
                    a.voxels() == b.voxels(),
                    "chunk {c:?} differs at strength 0"
                );
            }
        }
    }
);

dwell_world::both_faces!(
    /// The owner (2026-10-27): "did we work in rendering undersides in cliffs LODs? If not that
    /// should be the priority." The LOD's sections filled each column solid up to its top, so an
    /// overhang read as a solid bank from afar. They take the volume's carving too: round steep
    /// hillsides, sections at the near levels hold rock over air (an underside to draw). The
    /// cliffs' own overhangs are slight since 0.38.0 (under a metre, thinner than these levels'
    /// cells), so a volume with big ones is given by data here.
    fn lod_sections_hold_the_overhangs_undersides(f: FaceFrame) {
        use dwell_world::lod::{
            LodCells, LodCoord, LodKind, ORIGIN_X, ORIGIN_Y, ORIGIN_Z, SECTION_CELLS, cell_index,
            section_size,
        };
        let plain = BifacialTerrain::height_field(7);
        let terrain = BifacialTerrain::with_volume(7, &OVERHANGS);
        let (p, g) = (plain.local(f.face), terrain.local(f.face));
        let section = |level: i32, x: i64, y: i64, z: i64| {
            let s = section_size(level);
            let at = |v: i64, o: i64| (v - o).div_euclid(s) as i32;
            LodCoord::new(level, at(x, ORIGIN_X), at(y, ORIGIN_Y), at(z, ORIGIN_Z))
        };
        let mut under = [0usize; 3];
        for at in steep_columns(p, 1.1, 6) {
            let ground = i64::from(g.ground_y(at.0, at.1).expect("disc"));
            for (k, level) in [1, 2, 3].into_iter().enumerate() {
                let s = section_size(level);
                for dy in -1..=1 {
                    let c = section(level, i64::from(at.0), ground + dy * s, i64::from(at.1));
                    let mut cells = LodCells::new();
                    if g.generate_lod(c, &mut cells, None) != LodKind::Content {
                        continue;
                    }
                    for z in 0..SECTION_CELLS {
                        for x in 0..SECTION_CELLS {
                            for y in 1..SECTION_CELLS {
                                let m = cells[cell_index(x, y, z)];
                                let below = cells[cell_index(x, y - 1, z)];
                                let tree =
                                    named(m).starts_with("dwell:log") || named(m) == "dwell:leaves";
                                if solid(m) && !tree && below == M.air {
                                    under[k] += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            under[0] > 20 && under[1] > 5,
            "undersides at levels 1, 2, 3: {under:?}"
        );
    }
);

/// Over 64 m squares round six steep hillsides of `cliffs` (face-local, either face): the steps of
/// 4 m or more between neighbouring columns, and how many of them have the uphill side (on the
/// height field) high. A wall with its downhill side high is a trench's or a pit's side.
fn walls_facing_downhill(f: FaceFrame, cliffs: &BifacialTerrain) -> (usize, usize) {
    let plain = BifacialTerrain::height_field(7);
    let (p, g) = (plain.local(f.face), cliffs.local(f.face));
    let (mut walls, mut downhill) = (0, 0);
    for (cx, cz) in steep_columns(p, 1.1, 6) {
        for z in cz - 32..cz + 32 {
            for x in cx - 32..cx + 32 {
                let top = |x, z| g.column_at(x, z).height;
                let base = |x, z| p.column_at(x, z).height;
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let (a, b) = (top(x, z), top(x + dx, z + dz));
                    if (a - b).abs() >= 4.0 {
                        walls += 1;
                        if (b > a) == (base(x + dx, z + dz) > base(x, z)) {
                            downhill += 1;
                        }
                    }
                }
            }
        }
    }
    (walls, downhill)
}

dwell_world::both_faces!(
    /// The owner (2026-10-27): the cliffs "kind of look like a huge worm dug around the surface of
    /// the terrain or like an ant farm from the side"; "redesign to look more like real cliffs".
    /// 0.34–0.37's walls stood on a noise's zero line, winding across the hill and sinking
    /// trenches (56–64% of their walls had the uphill side high, about chance); the terraces step
    /// the hill down along its contours. Red on 0.37's cliffs (`OVERHANGS`, the same data).
    fn cliffs_step_down_the_hill(f: FaceFrame) {
        let (walls, downhill) = walls_facing_downhill(f, &BifacialTerrain::new(7));
        assert!(walls > 200, "walls: {walls}");
        assert!(
            downhill * 100 >= walls * 98,
            "{downhill} of {walls} walls face downhill"
        );
        let (walls, downhill) =
            walls_facing_downhill(f, &BifacialTerrain::with_volume(7, &OVERHANGS));
        assert!(
            downhill * 100 < walls * 80,
            "0.37's cliffs: {downhill} of {walls}"
        );
    }
);
