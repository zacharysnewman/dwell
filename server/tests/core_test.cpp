#include <doctest/doctest.h>

#include "dwell/core/fixed_step.h"
#include "dwell/core/physics_world.h"
#include "dwell/core/voxel.h"

using namespace dwell::core;

TEST_CASE("fixed step: carries remainders and caps backlogs") {
  FixedStep step(60, /*max_steps_per_advance=*/5);
  CHECK(step.Advance(1.0 / 120) == 0);
  CHECK(step.Advance(1.0 / 120) == 1);  // two halves make one step
  CHECK(step.Advance(3.0 / 60) == 3);
  CHECK(step.Advance(10.0) == 5);  // capped, backlog dropped
  CHECK(step.Advance(0.0) == 0);
}

TEST_CASE("voxel: chunk coordinates use floor division") {
  CHECK(ChunkOf(0, 0, 0) == ChunkCoord{0, 0, 0});
  CHECK(ChunkOf(31, 32, -1) == ChunkCoord{0, 1, -1});
  CHECK(ChunkOf(-32, -33, 63) == ChunkCoord{-1, -2, 1});
}

TEST_CASE("voxel: flat world layers and revisions") {
  VoxelWorld world;
  CHECK(world.GetVoxel(5, 0, 5) == Materials::kAir);
  CHECK(world.GetVoxel(5, -1, 5) == Materials::kGrass);
  CHECK(world.GetVoxel(5, -2, 5) == Materials::kDirt);
  CHECK(world.GetVoxel(5, -10, 5) == Materials::kStone);
  CHECK(world.GetVoxel(5, -128, 5) == Materials::kBedrock);
  CHECK(GetMaterial(Materials::kBedrock).indestructible);

  const auto coord = ChunkOf(5, -1, 5);
  CHECK(world.Find(coord)->revision() == 0);  // generated, unmodified
  world.SetVoxel(5, -1, 5, Materials::kAir);
  CHECK(world.GetVoxel(5, -1, 5) == Materials::kAir);
  CHECK(world.Find(coord)->revision() == 1);
}

TEST_CASE("physics layers: terrain never collides with terrain") {
  CHECK_FALSE(LayersCollide(ObjectLayers::kTerrain, ObjectLayers::kTerrain));
  CHECK(LayersCollide(ObjectLayers::kTerrain, ObjectLayers::kTier1));
  CHECK(LayersCollide(ObjectLayers::kTier1, ObjectLayers::kCharacter));
  CHECK(LayersCollide(ObjectLayers::kCharacter, ObjectLayers::kCharacter));
}
