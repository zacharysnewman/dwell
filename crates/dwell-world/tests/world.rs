//! The voxel grid and block edits (ported from the C++ core and block-edit tests); structures are
//! in `structures.rs`.

use std::sync::Arc;

use dwell_blocks::{StateId, require};
use dwell_protocol::{BlockEditAction, BlockEditRequest};
use dwell_world::edit::*;
use dwell_world::*;

/// ~8,000 km out (tests/test_origin.h): everything must work there as at the origin.
const FAR_X: i32 = 3906 * 2048;

fn s(name: &str) -> StateId {
    require(name)
}

fn empty() -> VoxelWorld {
    VoxelWorld::new(empty_generator(), None)
}

fn brk(cell: [i32; 3], face: u8) -> BlockEditRequest {
    BlockEditRequest {
        action: BlockEditAction::Break,
        cell,
        face,
        material: 0,
    }
}
fn place(cell: [i32; 3], face: u8, material: StateId) -> BlockEditRequest {
    BlockEditRequest {
        action: BlockEditAction::Place,
        cell,
        face,
        material,
    }
}
fn check(
    world: &mut VoxelWorld,
    r: BlockEditRequest,
    eye: [f64; 3],
    players: &[EditCapsule],
) -> EditOutcome {
    check_block_edit(world, &r, eye, players, DEFAULT_REACH)
}

#[test]
fn chunk_coordinates_use_floor_division() {
    assert_eq!(chunk_of(0, 0, 0), ChunkCoord::new(0, 0, 0));
    assert_eq!(chunk_of(31, 32, -1), ChunkCoord::new(0, 1, -1));
    assert_eq!(chunk_of(-32, -33, 63), ChunkCoord::new(-1, -2, 1));
}

#[test]
fn flat_world_layers_and_revisions() {
    let mut world = VoxelWorld::flat();
    let top = FLAT_TOP_Y;
    assert_eq!(world.get_voxel(5, top, 5), s("dwell:air"));
    assert_eq!(world.get_voxel(5, top - 1, 5), s("dwell:grass"));
    assert_eq!(world.get_voxel(5, top - 2, 5), s("dwell:dirt"));
    assert_eq!(world.get_voxel(5, top - 10, 5), s("dwell:stone"));
    assert_eq!(world.get_voxel(5, MIDPLANE_Y, 5), s("dwell:stone")); // the core: stone, no bedrock
    assert_eq!(world.get_voxel(5, MIDPLANE_Y - 1, 5), s("dwell:air")); // no face B
    let coord = chunk_of(5, top - 1, 5);
    assert_eq!(world.find(coord).expect("generated").revision(), 0);
    world.set_voxel(5, top - 1, 5, s("dwell:air"));
    assert_eq!(world.get_voxel(5, top - 1, 5), s("dwell:air"));
    assert_eq!(world.find(coord).expect("chunk").revision(), 1);
}

#[test]
fn the_palette_is_the_registrys_placeable_states() {
    let names: Vec<&str> = (0..dwell_blocks::STATES.len() as StateId)
        .filter(|&m| placeable(m))
        .map(|m| dwell_blocks::info(m).name)
        .collect();
    assert_eq!(
        names,
        [
            "dwell:stone[flooded=false]",
            "dwell:dirt[flooded=false]",
            "dwell:grass[flooded=false]",
            "dwell:ladder[facing=north,flooded=false]",
            "dwell:ladder[facing=east,flooded=false]",
            "dwell:ladder[facing=south,flooded=false]",
            "dwell:ladder[facing=west,flooded=false]",
            "dwell:sand[flooded=false]",
            "dwell:log",
            "dwell:leaves",
            "dwell:tall_grass"
        ]
    );
    assert!(!placeable(dwell_blocks::STATES.len() as StateId));
    assert!(!targetable(s("dwell:air")));
    assert!(!targetable(s("dwell:water")));
    assert!(targetable(s("dwell:ladder")));
}

#[test]
fn the_ray_finds_the_first_targetable_cell_and_its_face() {
    for ox in [0, FAR_X] {
        let mut world = empty();
        // A non-adaptive cube (a lone stone block would be a half pyramid: its slopes count as tops).
        world.set_voxel(ox, 0, 5, s("dwell:sandbox_floor"));
        world.set_voxel(ox, 0, 3, s("dwell:water")); // liquids are not targets
        let eye = [f64::from(ox) + 0.5, 0.5, 0.5];
        let hit = raycast_block(&mut world, eye, [0.0, 0.0, 1.0], 5.0).expect("hit");
        assert_eq!((hit.cell, hit.face), ([ox, 0, 5], 5)); // entered through −Z
        assert!((hit.distance - 4.5).abs() < 1e-5);
        assert!(raycast_block(&mut world, eye, [0.0, 0.0, 1.0], 4.4).is_none());
        // Looking down onto a floor: its top face.
        world.set_voxel(ox + 2, -1, 0, s("dwell:sandbox_floor"));
        let d = 0.5f32.sqrt();
        let hit = raycast_block(
            &mut world,
            [f64::from(ox) + 0.8, 1.5, 0.5],
            [d, -d, 0.0],
            5.0,
        )
        .expect("hit");
        assert_eq!((hit.cell, hit.face), ([ox + 2, -1, 0], 2));
        // The cell containing the eye is not a target.
        world.set_voxel(ox + 5, 0, 0, s("dwell:sandbox_floor")); // inside a solid cube
        assert!(
            raycast_block(
                &mut world,
                [f64::from(ox) + 5.5, 0.5, 0.5],
                [1.0, 0.0, 0.0],
                3.0
            )
            .is_none()
        );
    }
}

#[test]
fn a_ladder_is_targeted_broken_and_built_against_as_its_whole_cell() {
    for ox in [0, FAR_X] {
        let mut world = empty();
        world.set_voxel(ox, 0, 5, s("dwell:stone"));
        world.set_voxel(ox, 0, 3, s("dwell:ladder"));
        let hit = raycast_block(
            &mut world,
            [f64::from(ox) + 0.5, 0.5, 0.5],
            [0.0, 0.0, 1.0],
            5.0,
        )
        .expect("hit");
        assert_eq!((hit.cell, hit.face), ([ox, 0, 3], 5));
        assert!((hit.distance - 2.5).abs() < 1e-5);
    }
    let mut world = empty();
    world.set_voxel(0, -1, 2, s("dwell:grass"));
    world.set_voxel(0, 0, 2, s("dwell:ladder"));
    let eye = [0.5, 1.62, 0.5];
    let ok = check(&mut world, brk([0, 0, 2], 5), eye, &[]);
    assert_eq!((ok.check, ok.material), (EditCheck::Ok, s("dwell:air")));
    let ok = check(&mut world, place([0, 0, 2], 2, s("dwell:stone")), eye, &[]);
    assert_eq!((ok.check, ok.cell), (EditCheck::Ok, [0, 1, 2]));
}

fn ground() -> VoxelWorld {
    let mut world = empty();
    for x in -4..=4 {
        for z in -4..=8 {
            world.set_voxel(x, -1, z, s("dwell:grass"));
        }
    }
    world
}

#[test]
fn validation_of_reach_sight_materials_and_occupancy() {
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    let ok = check(&mut world, place([0, -1, 2], 2, s("dwell:stone")), eye, &[]);
    assert_eq!(
        (ok.check, ok.cell, ok.material),
        (EditCheck::Ok, [0, 0, 2], s("dwell:stone"))
    );
    let ok = check(&mut world, brk([0, -1, 2], 2), eye, &[]);
    assert_eq!(
        (ok.check, ok.cell, ok.material),
        (EditCheck::Ok, [0, -1, 2], s("dwell:air"))
    );
    // A face seen at a grazing angle: the top of a block at eye level, 4 m away.
    world.set_voxel(0, 0, 4, s("dwell:stone"));
    assert_eq!(
        check(
            &mut world,
            place([0, 0, 4], 2, s("dwell:stone")),
            [0.5, 1.2, 0.5],
            &[]
        )
        .check,
        EditCheck::Ok
    );
    assert_eq!(
        check(&mut world, brk([0, -1, 7], 2), eye, &[]).check,
        EditCheck::OutOfReach
    );
    assert_eq!(
        check(&mut world, brk([0, 0, 2], 2), eye, &[]).check,
        EditCheck::NothingThere
    );
    world.set_voxel(1, -1, 2, s("dwell:water"));
    assert_eq!(
        check(&mut world, brk([1, -1, 2], 2), eye, &[]).check,
        EditCheck::NothingThere
    );
    assert_eq!(
        check(&mut world, brk([0, -1, 2], 3), eye, &[]).check,
        EditCheck::NoLineOfSight
    );
    for (x, y) in [(0, 0), (0, 1), (-1, 0), (-1, 1), (1, 0), (1, 1), (0, 2)] {
        world.set_voxel(x, y, 1, s("dwell:stone"));
    }
    assert_eq!(
        check(&mut world, brk([0, -1, 3], 2), eye, &[]).check,
        EditCheck::NoLineOfSight
    );
    assert_eq!(
        check(&mut world, brk([0, 0, 1], 5), eye, &[]).check,
        EditCheck::Ok
    );
    for m in [
        s("dwell:air"),
        s("dwell:water"),
        s("dwell:ladder[facing=east,flooded=true]"),
        65000,
    ] {
        assert_eq!(
            check(&mut world, place([-2, -1, 0], 2, m), eye, &[]).check,
            EditCheck::NotPlaceable,
            "{m}"
        );
    }
}

#[test]
fn cutout_blocks_without_a_solid_are_air_at_lod() {
    // Ladders (and plants) are thin and see-through: the LOD doesn't draw them as cubes.
    let ladder = s("dwell:ladder[facing=north,flooded=false]");
    let stone = s("dwell:stone");
    assert!(!lod::lod_drawn(ladder));
    assert!(lod::lod_drawn(stone) && lod::lod_drawn(s("dwell:water")));
    // Four ladders and four air: air (before, four filled cells made a ladder-coloured cube).
    let half: [StateId; 8] = std::array::from_fn(|i| if i & 2 == 0 { ladder } else { 0 });
    assert_eq!(lod::downsample_block(&half), 0);
    // Ladders beside stone neither fill the cell nor give it their material.
    let mixed: [StateId; 8] = std::array::from_fn(|i| match i {
        0..=2 => stone,
        3 => ladder,
        _ => 0,
    });
    assert_eq!(lod::downsample_block(&mixed), 0);
    // A ladder on top of stone: the stone is what shows.
    let on_top: [StateId; 8] = std::array::from_fn(|i| if i & 2 == 0 { stone } else { ladder });
    assert_eq!(lod::downsample_block(&on_top), stone);
}

#[test]
fn tall_grass_is_placed_only_on_grass_or_dirt() {
    let grass = s("dwell:tall_grass");
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    // On grass: into the cell above it.
    let on_grass = check(&mut world, place([0, -1, 2], 2, grass), eye, &[]);
    assert_eq!((on_grass.check, on_grass.cell), (EditCheck::Ok, [0, 0, 2]));
    // On dirt too.
    world.set_voxel(1, -1, 2, s("dwell:dirt"));
    assert_eq!(
        check(&mut world, place([1, -1, 2], 2, grass), eye, &[]).check,
        EditCheck::Ok
    );
    // Not on stone, sand or the side of a block (nothing below it).
    for m in ["dwell:stone", "dwell:sand"] {
        world.set_voxel(-1, -1, 2, s(m));
        assert_eq!(
            check(&mut world, place([-1, -1, 2], 2, grass), eye, &[]).check,
            EditCheck::NoSupport,
            "{m}"
        );
    }
    // Walls of a non-adaptive cube (the sandbox floor): adaptive blocks would take shapes.
    world.set_voxel(0, 0, 3, s("dwell:sandbox_floor"));
    let side = check(&mut world, place([0, 0, 3], 5, grass), eye, &[]);
    assert_eq!(side.cell, [0, 0, 2]);
    assert_eq!(
        side.check,
        EditCheck::Ok,
        "against a side: there is grass below that cell"
    );
    world.set_voxel(0, 1, 3, s("dwell:sandbox_floor"));
    assert_eq!(
        check(&mut world, place([0, 1, 3], 5, grass), eye, &[]).check,
        EditCheck::NoSupport,
        "against a side with air below"
    );
    // Blocks without the rule place anywhere, as before.
    assert_eq!(
        check(
            &mut world,
            place([-1, -1, 2], 2, s("dwell:stone")),
            eye,
            &[]
        )
        .check,
        EditCheck::Ok
    );
}

#[test]
fn placing_into_tall_grass_replaces_it() {
    let grass = s("dwell:tall_grass");
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    world.set_voxel(0, 0, 2, grass);
    // Aimed at the grass itself: the block goes into its cell.
    let into = check(&mut world, place([0, 0, 2], 2, s("dwell:stone")), eye, &[]);
    assert_eq!((into.check, into.cell), (EditCheck::Ok, [0, 0, 2]));
    // Aimed past it, at the block beside its cell: the grass's cell is not Occupied.
    world.set_voxel(1, 0, 2, s("dwell:sandbox_floor"));
    world.set_voxel(1, 0, 3, s("dwell:sandbox_floor"));
    world.set_voxel(0, 0, 3, grass);
    world.set_voxel(0, 0, 2, 0);
    // Seen past the grass's box (over its top) from beside it.
    let side_eye = [-0.5, 0.8, 3.5];
    let beside = check(
        &mut world,
        place([1, 0, 3], 1, s("dwell:dirt")),
        side_eye,
        &[],
    );
    assert_eq!((beside.check, beside.cell), (EditCheck::Ok, [0, 0, 3]));
    assert!(
        !dwell_blocks::info(s("dwell:sand")).replaceable,
        "full blocks stay Occupied"
    );
}

#[test]
fn breaking_the_block_under_tall_grass_takes_the_grass_with_it() {
    let grass = s("dwell:tall_grass");
    // On face A, clear of the midplane (Phase 16: it is at y = 0).
    let mut world = ground();
    world.set_voxel(0, 31, 2, s("dwell:grass"));
    world.set_voxel(1, 31, 2, s("dwell:grass"));
    world.set_voxel(0, 32, 2, grass);
    // The grass block under it is broken: the plant goes too (it needs support below).
    assert_eq!(
        unsupported_after(&mut world, [0, 31, 2], s("dwell:air")),
        vec![([0, 32, 2], s("dwell:air"))]
    );
    // Turned to dirt, it still stands; on stone it would not.
    assert!(unsupported_after(&mut world, [0, 31, 2], s("dwell:dirt")).is_empty());
    assert_eq!(
        unsupported_after(&mut world, [0, 31, 2], s("dwell:stone")).len(),
        1
    );
    // A block with no rule above a broken one is left alone.
    world.set_voxel(1, 32, 2, s("dwell:stone"));
    assert!(unsupported_after(&mut world, [1, 31, 2], s("dwell:air")).is_empty());
    // On face B "below" is toward the midplane (+y).
    let y = MIDPLANE_Y - 10;
    world.set_voxel(0, y, 9, grass);
    assert_eq!(below([0, y, 9]), [0, y + 1, 9]);
    assert_eq!(
        unsupported_after(&mut world, [0, y + 1, 9], s("dwell:air")),
        vec![([0, y, 9], s("dwell:air"))]
    );
}

#[test]
fn tall_grass_is_aimed_at_by_its_smaller_box() {
    let grass = s("dwell:tall_grass");
    let b = dwell_blocks::info(grass)
        .selection
        .expect("the data gives a box");
    let mut world = empty();
    world.set_voxel(0, 0, 3, grass);
    let (lo, hi) = target_box(&mut world, [0, 0, 3]);
    assert_eq!((lo, hi), ([b[0], b[1], b[2]], [b[3], b[4], b[5]]));
    // Through the box's middle: hit on its near face, inside the cell.
    let hit = raycast_block(&mut world, [0.5, 0.3, 0.5], [0.0, 0.0, 1.0], 5.0).expect("hit");
    assert_eq!((hit.cell, hit.face), ([0, 0, 3], 5));
    assert!(
        (hit.distance - (2.5 + b[2])).abs() < 1e-4,
        "{}",
        hit.distance
    );
    // Above the box (but in the cell): the ray passes; past the box's side, too.
    assert!(raycast_block(&mut world, [0.5, 0.9, 0.5], [0.0, 0.0, 1.0], 5.0).is_none());
    assert!(raycast_block(&mut world, [0.05, 0.3, 0.5], [0.0, 0.0, 1.0], 5.0).is_none());
    // Its box is what the server's line of sight and a full block's outline use.
    world.set_voxel(4, 0, 3, s("dwell:sandbox_floor"));
    assert_eq!(target_box(&mut world, [4, 0, 3]), ([0.0; 3], [1.0; 3]));
    let mut world = ground();
    world.set_voxel(0, 0, 2, grass);
    let eye = [0.5, 1.62, 0.5];
    assert_eq!(
        check(&mut world, brk([0, 0, 2], 2), eye, &[]).check,
        EditCheck::Ok
    );
}

#[test]
fn the_sandbox_floor_cannot_be_broken_or_replaced_but_can_be_built_on() {
    let floor = s("dwell:sandbox_floor");
    assert!(
        dwell_blocks::info(floor).indestructible,
        "the block data marks it"
    );
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    world.set_voxel(0, -1, 2, floor);
    assert_eq!(
        check(&mut world, brk([0, -1, 2], 2), eye, &[]).check,
        EditCheck::Unbreakable
    );
    // Built on: a block placed against it lands next to it and leaves it be.
    let on = check(&mut world, place([0, -1, 2], 2, s("dwell:stone")), eye, &[]);
    assert_eq!((on.check, on.cell), (EditCheck::Ok, [0, 0, 2]));
    // Never replaced by a placement into its cell.
    world.set_voxel(0, 0, 3, s("dwell:stone"));
    world.set_voxel(0, 0, 2, floor);
    let into = check(&mut world, place([0, 0, 3], 5, s("dwell:stone")), eye, &[]);
    assert_ne!(into.check, EditCheck::Ok);
    // Every indestructible state in the data is refused the same way.
    for m in
        (0..dwell_blocks::STATES.len() as StateId).filter(|&m| dwell_blocks::info(m).indestructible)
    {
        let mut world = ground();
        world.set_voxel(0, -1, 2, m);
        if targetable(m) {
            assert_eq!(
                check(&mut world, brk([0, -1, 2], 2), eye, &[]).check,
                EditCheck::Unbreakable
            );
        }
    }
    // Breakable blocks still break.
    let mut world = ground();
    assert_eq!(
        check(&mut world, brk([0, -1, 2], 2), eye, &[]).check,
        EditCheck::Ok
    );
}

#[test]
fn plants_go_only_into_their_medium() {
    // Tall grass is an air plant (block data `medium: air`): into water, refused.
    let tall = s("dwell:tall_grass");
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    world.set_voxel(-2, 0, 0, s("dwell:water"));
    assert_eq!(
        check(&mut world, place([-2, -1, 0], 2, tall), eye, &[]).check,
        EditCheck::WrongMedium
    );
    // Blocks without the rule still go into water.
    assert_eq!(
        check(&mut world, place([-2, -1, 0], 2, s("dwell:dirt")), eye, &[]).check,
        EditCheck::Ok
    );
    // A water plant added by data alone (shared/blocks/fixtures: `test:kelp`, `medium: water`)
    // goes only into water, and holds it (its flooded state).
    use dwell_blocks::fixture::STATES;
    let row = |name: &str| STATES.iter().find(|r| r.name == name).expect(name);
    let kelp = row("test:kelp[flooded=true]");
    assert!(medium_allows(kelp, row("dwell:water")));
    assert!(!medium_allows(kelp, row("dwell:air")));
    let grass = row("dwell:tall_grass");
    assert!(medium_allows(grass, row("dwell:air")));
    assert!(!medium_allows(grass, row("dwell:water")));
    assert!(medium_allows(
        row("dwell:stone[flooded=false]"),
        row("dwell:water")
    ));
    // An air plant never stays flooded; anything else is kept as it is.
    assert_eq!(medium_kept(tall), tall);
    assert_eq!(
        medium_kept(s("dwell:ladder[flooded=true]")),
        s("dwell:ladder[flooded=true]")
    );
}

#[test]
fn placing_into_water_is_allowed_into_players_rejected() {
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    world.set_voxel(-2, 0, 0, s("dwell:water"));
    assert_eq!(
        check(&mut world, place([-2, -1, 0], 2, s("dwell:dirt")), eye, &[]).check,
        EditCheck::Ok
    );
    let player = [EditCapsule {
        center: [-0.5, 0.9, 2.5],
        radius: 0.3,
        half_cylinder: 0.6,
    }];
    assert_eq!(
        check(
            &mut world,
            place([-1, -1, 2], 2, s("dwell:stone")),
            eye,
            &player
        )
        .check,
        EditCheck::IntoPlayer
    );
    assert_eq!(
        check(
            &mut world,
            place([-1, -1, 2], 2, s("dwell:ladder")),
            eye,
            &player
        )
        .check,
        EditCheck::Ok
    );
    let beside = [EditCapsule {
        center: [-2.5, 0.9, 2.5],
        radius: 0.3,
        half_cylinder: 0.6,
    }];
    assert_eq!(
        check(
            &mut world,
            place([-1, -1, 2], 2, s("dwell:stone")),
            eye,
            &beside
        )
        .check,
        EditCheck::Ok
    );
}

#[test]
fn flooded_blocks_follow_their_cell_and_leave_water() {
    let mut world = ground();
    let eye = [0.5, 1.62, 0.5];
    let dry = s("dwell:ladder[facing=south]");
    let wet = s("dwell:ladder[facing=south,flooded=true]");
    let r = check(&mut world, place([0, -1, 2], 2, dry), eye, &[]);
    assert_eq!((r.check, r.material), (EditCheck::Ok, dry));
    assert_eq!(
        check(&mut world, place([0, -1, 2], 2, wet), eye, &[]).check,
        EditCheck::NotPlaceable
    );
    world.set_voxel(-2, 0, 0, s("dwell:water"));
    let r = check(&mut world, place([-2, -1, 0], 2, dry), eye, &[]);
    assert_eq!((r.check, r.material), (EditCheck::Ok, wet));
    world.set_voxel(1, 0, 2, wet);
    world.set_voxel(-1, 0, 2, dry);
    let r = check(&mut world, brk([1, 0, 2], 2), eye, &[]);
    assert_eq!((r.check, r.material), (EditCheck::Ok, s("dwell:water")));
    let r = check(&mut world, brk([-1, 0, 2], 2), eye, &[]);
    assert_eq!((r.check, r.material), (EditCheck::Ok, s("dwell:air")));
}

#[test]
fn nothing_is_placed_outside_the_build_volume_or_disc() {
    // The build sphere (ADR 0025): over its top, under its bottom (face B), and beside it high
    // over the rim, where the sphere is low.
    let mut world = empty();
    let sky = BUILD_VOLUME.y_range().1 as i32;
    let top = sky - 1;
    world.set_voxel(0, top, 2, s("dwell:sandbox_floor")); // a cube: its −Z face is there to aim at
    let r = place([0, top, 2], 2, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [0.5, f64::from(sky) + 0.6, 2.5], &[]).check,
        EditCheck::OutOfWorld
    );
    let r = place([0, top, 2], 5, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [0.5, f64::from(sky) - 0.5, 0.5], &[]).check,
        EditCheck::Ok
    );
    let bottom = BUILD_VOLUME.y_range().0 as i32;
    world.set_voxel(0, bottom, 2, s("dwell:sandbox_floor"));
    let r = place([0, bottom, 2], 3, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [0.5, f64::from(bottom) - 0.6, 2.5], &[]).check,
        EditCheck::OutOfWorld
    );
    // 45 km out the sphere is 21.8 km tall: inside below, outside above.
    let (x, high) = (45_000, 21_793);
    world.set_voxel(x, high - 1, 0, s("dwell:sandbox_floor"));
    let r = place([x, high - 1, 0], 2, s("dwell:stone"));
    assert_eq!(
        check(
            &mut world,
            r,
            [f64::from(x) + 0.5, f64::from(high) + 0.6, 0.5],
            &[]
        )
        .check,
        EditCheck::OutOfWorld
    );
    assert!(inside_build_volume(i64::from(x), i64::from(high) - 1, 0));
    assert!(!inside_build_volume(i64::from(x), i64::from(high), 0));
    let rim = WORLD_RADIUS - 1;
    world.set_voxel(rim, 0, 0, s("dwell:sandbox_floor"));
    let r = place([rim, 0, 0], 0, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [f64::from(rim) + 1.5, 0.5, 0.5], &[]).check,
        EditCheck::OutOfWorld
    );
}

#[test]
fn a_block_placed_into_a_chunk_read_as_sky_is_seen() {
    let mut world = VoxelWorld::new(empty_generator(), Some(Arc::new(|c: ChunkCoord| c.y > 0)));
    assert_eq!(world.get_voxel(5, 40, 5), s("dwell:air"));
    world.set_voxel(5, 40, 5, s("dwell:stone"));
    assert_eq!(world.get_voxel(5, 40, 5), s("dwell:stone"));
}

#[test]
fn a_box_reads_in_a_structures_layer_order_across_chunk_borders() {
    // Saving a structure reads the client's world through `region::read_box` (STRUCTURES.md §4):
    // its order is a structure file's layers (y-slices, z-rows, x fastest).
    let mut world = VoxelWorld::streamed();
    // The box straddles chunk borders on every axis (chunks are 32 cells), at negative coordinates.
    let min = [-2, 30, -1];
    let size = [4, 3, 2];
    for (x, y, z) in [
        (-2, 30, -1),
        (1, 33, 1),
        (-2, 30, 1),
        (1, 30, -1),
        (-2, 33, -1),
        (1, 33, -1),
        (-2, 33, 1),
        (1, 30, 1),
    ] {
        // Every chunk the box covers is held (a missing one reads as None, not air).
        world.set_voxel(x, y, z, s("dwell:air"));
    }
    world.set_voxel(-2, 30, -1, s("dwell:stone")); // (0, 0, 0)
    world.set_voxel(1, 30, -1, s("dwell:dirt")); // (3, 0, 0): x is fastest
    world.set_voxel(-2, 30, 0, s("dwell:sand")); // (0, 0, 1): then z
    world.set_voxel(-1, 32, 0, s("dwell:grass")); // (1, 2, 1): y slowest
    world.set_voxel(2, 30, -1, s("dwell:stone")); // just outside the box
    let cells = dwell_world::region::read_box(&mut world, min, size).expect("loaded");
    assert_eq!(cells.len(), 4 * 3 * 2);
    let at = |x: usize, y: usize, z: usize| cells[(y * 2 + z) * 4 + x];
    assert_eq!(at(0, 0, 0), s("dwell:stone"));
    assert_eq!(at(3, 0, 0), s("dwell:dirt"));
    assert_eq!(at(0, 0, 1), s("dwell:sand"));
    assert_eq!(at(1, 2, 1), s("dwell:grass"));
    let filled = cells.iter().filter(|&&c| c != s("dwell:air")).count();
    assert_eq!(filled, 4, "the cell outside the box is not read");
    // A box reaching into a chunk the client doesn't hold is not read as air.
    assert_eq!(
        dwell_world::region::read_box(&mut world, min, [4, 3, 40]),
        None
    );
}

#[test]
fn chunk_changes_apply_with_the_modifications_revision() {
    let mut world = VoxelWorld::streamed();
    apply_chunk_changes(
        &mut world,
        &dwell_protocol::ChunkChanges {
            coord: [0, 0, 0],
            revision: 4,
            changes: vec![dwell_protocol::VoxelChange {
                index: 33,
                material: s("dwell:sand"),
            }],
        },
    );
    assert_eq!(world.get_voxel(1, 1, 0), s("dwell:sand"));
    assert_eq!(
        world
            .find(ChunkCoord::new(0, 0, 0))
            .expect("chunk")
            .revision(),
        4
    );
}
