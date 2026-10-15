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
    assert_eq!(world.get_voxel(5, 0, 5), s("dwell:air"));
    assert_eq!(world.get_voxel(5, -1, 5), s("dwell:grass"));
    assert_eq!(world.get_voxel(5, -2, 5), s("dwell:dirt"));
    assert_eq!(world.get_voxel(5, -10, 5), s("dwell:stone"));
    assert_eq!(world.get_voxel(5, MIDPLANE_Y, 5), s("dwell:stone")); // the core: stone, no bedrock
    assert_eq!(world.get_voxel(5, MIDPLANE_Y - 1, 5), s("dwell:air")); // the void
    let coord = chunk_of(5, -1, 5);
    assert_eq!(world.find(coord).expect("generated").revision(), 0);
    world.set_voxel(5, -1, 5, s("dwell:air"));
    assert_eq!(world.get_voxel(5, -1, 5), s("dwell:air"));
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
            "dwell:stone",
            "dwell:dirt",
            "dwell:grass",
            "dwell:ladder[facing=north,flooded=false]",
            "dwell:ladder[facing=east,flooded=false]",
            "dwell:ladder[facing=south,flooded=false]",
            "dwell:ladder[facing=west,flooded=false]",
            "dwell:sand"
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
        world.set_voxel(ox, 0, 5, s("dwell:stone"));
        world.set_voxel(ox, 0, 3, s("dwell:water")); // liquids are not targets
        let eye = [f64::from(ox) + 0.5, 0.5, 0.5];
        let hit = raycast_block(&mut world, eye, [0.0, 0.0, 1.0], 5.0).expect("hit");
        assert_eq!((hit.cell, hit.face), ([ox, 0, 5], 5)); // entered through −Z
        assert!((hit.distance - 4.5).abs() < 1e-5);
        assert!(raycast_block(&mut world, eye, [0.0, 0.0, 1.0], 4.4).is_none());
        // Looking down onto a floor: its top face.
        world.set_voxel(ox + 2, -1, 0, s("dwell:grass"));
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
        world.set_voxel(ox + 5, 0, 0, s("dwell:dirt"));
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
fn nothing_is_placed_outside_the_worlds_rows_or_disc() {
    let mut world = empty();
    let top = WORLD_MAX_Y - 1;
    world.set_voxel(0, top, 2, s("dwell:stone"));
    let r = place([0, top, 2], 2, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [0.5, f64::from(WORLD_MAX_Y) + 0.6, 2.5], &[]).check,
        EditCheck::OutOfWorld
    );
    let r = place([0, top, 2], 5, s("dwell:stone"));
    assert_eq!(
        check(&mut world, r, [0.5, f64::from(WORLD_MAX_Y) - 0.5, 0.5], &[]).check,
        EditCheck::Ok
    );
    let rim = WORLD_RADIUS - 1;
    world.set_voxel(rim, 0, 0, s("dwell:stone"));
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
