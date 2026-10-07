// Block interaction (ARCHITECTURE.md §6.5, Phase 3d): the targeting ray, the server's validation of
// edit requests (§11), VoxelModification broadcasts with per-chunk revisions, and resync.
#include <doctest/doctest.h>

#include "dwell/core/block_edit.h"
#include "dwell/core/block_registry.h"
#include "dwell/core/physics_world.h"
#include "dwell/player/voxel_query.h"
#include "server_fixture.h"
#include "test_origin.h"

using namespace dwell::test;

namespace {

using Cell = std::array<std::int32_t, 3>;

VoxelWorld Empty() { return VoxelWorld(GenerateEmptyChunk); }

BlockEditRequest Break(Cell cell, int face) {
  return {BlockEditAction::kBreak, cell, static_cast<std::uint8_t>(face), 0};
}
BlockEditRequest Place(Cell cell, int face, MaterialId m) {
  return {BlockEditAction::kPlace, cell, static_cast<std::uint8_t>(face), m};
}

// Joins a streaming client (its worldgen check passes) to a flat world.
void JoinStreaming(Fixture& f, SessionId id) {
  const Welcome w = f.Join(id, Client(static_cast<std::uint8_t>(id)));
  Chunk v;
  GenerateFlatChunk({w.verification_chunk[0], w.verification_chunk[1], w.verification_chunk[2]}, v);
  f.Send(id, WorldgenCheck{ChunkHash(v)});
}

std::vector<VoxelModification> Modifications(const std::vector<Message>& messages) {
  std::vector<VoxelModification> out;
  for (const auto& m : messages) {
    if (const auto* v = std::get_if<VoxelModification>(&m)) out.push_back(*v);
  }
  return out;
}

}  // namespace

TEST_CASE("block edit: the palette (the registry's placeable states)") {
  std::vector<std::string_view> placeable;
  for (MaterialId m = 0; m < Materials::kCount; ++m) {
    if (Placeable(m)) placeable.push_back(GetMaterial(m).name);
  }
  // The shaped families (slopes and slabs) are placed through the shape selector, not slots: every
  // state of them is placeable, and the hotbar draws one slot per material.
  std::erase_if(placeable, [](std::string_view name) {
    return name.find("_slope[") != std::string_view::npos ||
           name.find("_slab[") != std::string_view::npos;
  });
  CHECK(placeable ==
        std::vector<std::string_view>{
            "dwell:stone", "dwell:dirt", "dwell:grass", "dwell:ladder[facing=north,flooded=false]",
            "dwell:ladder[facing=east,flooded=false]", "dwell:ladder[facing=south,flooded=false]",
            "dwell:ladder[facing=west,flooded=false]", "dwell:sand", "dwell:sandstone",
            "dwell:gravel", "dwell:snow", "dwell:ice", "dwell:log", "dwell:leaves", "dwell:coal_ore",
            "dwell:iron_ore", "dwell:gold_ore"});
  CHECK_FALSE(Placeable(Materials::kCount));
  CHECK_FALSE(Targetable(Materials::kAir));
  CHECK_FALSE(Targetable(Materials::kWater));
  CHECK(Targetable(Materials::kLadderN));
}

TEST_CASE("block edit: the targeting ray finds the first targetable cell and the face it enters") {
  for (const std::int32_t ox : {0, kFarOriginX}) {
    CAPTURE(ox);
    VoxelWorld world = Empty();
    world.SetVoxel(ox, 0, 5, Materials::kStone);
    world.SetVoxel(ox, 0, 3, Materials::kWater);  // liquids are not targets
    const std::array<double, 3> eye{ox + 0.5, 0.5, 0.5};
    auto hit = RaycastBlock(world, eye, {0, 0, 1}, 5.0f);
    REQUIRE(hit);
    CHECK(hit->cell == Cell{ox, 0, 5});
    CHECK(hit->face == 5);  // entered through −Z
    CHECK(hit->distance == doctest::Approx(4.5f));
    CHECK_FALSE(RaycastBlock(world, eye, {0, 0, 1}, 4.4f));

    // A slab is a half-height box: a ray above it passes, one through its lower half hits.
    world.SetVoxel(ox, 0, 2, Materials::kStoneSlab);
    CHECK(RaycastBlock(world, {ox + 0.5, 0.75, 0.5}, {0, 0, 1}, 5.0f)->cell == Cell{ox, 0, 5});
    CHECK(RaycastBlock(world, {ox + 0.5, 0.25, 0.5}, {0, 0, 1}, 5.0f)->cell == Cell{ox, 0, 2});

    // Looking down onto a floor: its top face.
    world.SetVoxel(ox + 2, -1, 0, Materials::kGrass);
    const float s = std::sqrt(0.5f);
    hit = RaycastBlock(world, {ox + 0.8, 1.5, 0.5}, {s, -s, 0}, 5.0f);
    REQUIRE(hit);
    CHECK(hit->cell == Cell{ox + 2, -1, 0});
    CHECK(hit->face == 2);

    // The cell containing the eye is not a target.
    world.SetVoxel(ox + 5, 0, 0, Materials::kLeaves);
    CHECK_FALSE(RaycastBlock(world, {ox + 5.5, 0.5, 0.5}, {1, 0, 0}, 3.0f));
  }
}

// Regression (Phase 9a): the ray and the server's line of sight took every block's exact shape, and
// a ladder has none (it is not solid): the crosshair passed through ladders, and an edit of one was
// refused, so ladders could not be broken or built against.
TEST_CASE("block edit: a ladder is targeted, broken and built against as its whole cell") {
  for (const std::int32_t ox : {0, kFarOriginX}) {
    CAPTURE(ox);
    VoxelWorld world = Empty();
    world.SetVoxel(ox, 0, 5, Materials::kStone);
    world.SetVoxel(ox, 0, 3, Materials::kLadder);
    const auto hit = RaycastBlock(world, {ox + 0.5, 0.5, 0.5}, {0, 0, 1}, 5.0f);
    REQUIRE(hit);
    CHECK(hit->cell == Cell{ox, 0, 3});
    CHECK(hit->face == 5);
    CHECK(hit->distance == doctest::Approx(2.5f));
  }
  VoxelWorld world = Empty();
  world.SetVoxel(0, -1, 2, Materials::kGrass);
  world.SetVoxel(0, 0, 2, Materials::kLadder);
  const std::array<double, 3> eye{0.5, 1.62, 0.5};
  const std::vector<EditCapsule> nobody;
  auto ok = CheckBlockEdit(world, Break({0, 0, 2}, 5), eye, nobody);
  CHECK(ok.check == EditCheck::kOk);
  CHECK(ok.material == Materials::kAir);
  ok = CheckBlockEdit(world, Place({0, 0, 2}, 2, Materials::kStone), eye, nobody);
  CHECK(ok.check == EditCheck::kOk);
  CHECK(ok.cell == Cell{0, 1, 2});
}

TEST_CASE("block edit: validation of reach, line of sight, materials, occupancy and players") {
  VoxelWorld world = Empty();
  for (int x = -4; x <= 4; ++x)
    for (int z = -4; z <= 8; ++z) world.SetVoxel(x, -1, z, Materials::kGrass);
  const std::array<double, 3> eye{0.5, 1.62, 0.5};
  const std::vector<EditCapsule> nobody;

  // Place on the ground in front: the cell above the targeted one.
  auto ok = CheckBlockEdit(world, Place({0, -1, 2}, 2, Materials::kStone), eye, nobody);
  CHECK(ok.check == EditCheck::kOk);
  CHECK(ok.cell == Cell{0, 0, 2});
  CHECK(ok.material == Materials::kStone);
  // Break it.
  ok = CheckBlockEdit(world, Break({0, -1, 2}, 2), eye, nobody);
  CHECK(ok.check == EditCheck::kOk);
  CHECK(ok.cell == Cell{0, -1, 2});
  CHECK(ok.material == Materials::kAir);

  // A face seen at a grazing angle: the top of a block at eye level, 4 m away.
  world.SetVoxel(0, 0, 4, Materials::kStone);
  CHECK(CheckBlockEdit(world, Place({0, 0, 4}, 2, Materials::kStone), {0.5, 1.2, 0.5}, nobody)
            .check == EditCheck::kOk);

  // Reach: REACH_DISTANCE plus the latency slack, to the nearest point of the cell.
  CHECK(CheckBlockEdit(world, Break({0, -1, 7}, 2), eye, nobody).check == EditCheck::kOutOfReach);
  // Nothing to break, and air or water to place against.
  CHECK(CheckBlockEdit(world, Break({0, 0, 2}, 2), eye, nobody).check == EditCheck::kNothingThere);
  world.SetVoxel(1, -1, 2, Materials::kWater);
  CHECK(CheckBlockEdit(world, Break({1, -1, 2}, 2), eye, nobody).check == EditCheck::kNothingThere);
  // The eye must be in front of the targeted face, with a clear view of it.
  CHECK(CheckBlockEdit(world, Break({0, -1, 2}, 3), eye, nobody).check ==
        EditCheck::kNoLineOfSight);
  world.SetVoxel(0, 0, 1, Materials::kStone);
  world.SetVoxel(0, 1, 1, Materials::kStone);
  world.SetVoxel(-1, 0, 1, Materials::kStone);
  world.SetVoxel(-1, 1, 1, Materials::kStone);
  world.SetVoxel(1, 0, 1, Materials::kStone);
  world.SetVoxel(1, 1, 1, Materials::kStone);
  world.SetVoxel(0, 2, 1, Materials::kStone);
  CHECK(CheckBlockEdit(world, Break({0, -1, 3}, 2), eye, nobody).check ==
        EditCheck::kNoLineOfSight);
  CHECK(CheckBlockEdit(world, Break({0, 0, 1}, 5), eye, nobody).check == EditCheck::kOk);

  // Bedrock is unbreakable.
  world.SetVoxel(3, 0, 0, Materials::kBedrock);
  CHECK(CheckBlockEdit(world, Break({3, 0, 0}, 1), eye, nobody).check == EditCheck::kUnbreakable);
  // Only the palette can be placed.
  for (const MaterialId m : {Materials::kAir, Materials::kWater, Materials::kBedrock,
                             Materials::kLaunchPad, MaterialId{65000}}) {
    CAPTURE(m);
    CHECK(CheckBlockEdit(world, Place({-2, -1, 0}, 2, m), eye, nobody).check ==
          EditCheck::kNotPlaceable);
  }
}

TEST_CASE("block edit: placing into blocks or players is rejected") {
  VoxelWorld world = Empty();
  for (int x = -4; x <= 4; ++x)
    for (int z = -4; z <= 8; ++z) world.SetVoxel(x, -1, z, Materials::kGrass);
  const std::array<double, 3> eye{0.5, 1.62, 0.5};
  const std::vector<EditCapsule> nobody;
  // Against a block's side whose neighbour is a slab: the face's upper half shows above the slab,
  // but the cell to place into is taken.
  world.SetVoxel(0, 0, 3, Materials::kStone);
  world.SetVoxel(0, 0, 2, Materials::kStoneSlab);
  CHECK(CheckBlockEdit(world, Place({0, 0, 3}, 5, Materials::kDirt), eye, nobody).check ==
        EditCheck::kOccupied);
  // Into a cell holding water: allowed (the block replaces it).
  world.SetVoxel(-2, 0, 0, Materials::kWater);
  CHECK(CheckBlockEdit(world, Place({-2, -1, 0}, 2, Materials::kDirt), eye, nobody).check ==
        EditCheck::kOk);

  // Into a player capsule: rejected for solid blocks (any player), allowed for ladders.
  const std::vector<EditCapsule> player{{{-0.5, 0.9, 2.5}, 0.3f, 0.6f}};
  CHECK(CheckBlockEdit(world, Place({-1, -1, 2}, 2, Materials::kStone), eye, player).check ==
        EditCheck::kIntoPlayer);
  CHECK(CheckBlockEdit(world, Place({-1, -1, 2}, 2, Materials::kLadderN), eye, player).check ==
        EditCheck::kOk);
  const std::vector<EditCapsule> beside{{{-2.5, 0.9, 2.5}, 0.3f, 0.6f}};
  CHECK(CheckBlockEdit(world, Place({-1, -1, 2}, 2, Materials::kStone), eye, beside).check ==
        EditCheck::kOk);
}

TEST_CASE("block edit: every slope and slab state of a shapeable material is placeable") {
  std::size_t shaped = 0;
  for (MaterialId m = 0; m < Materials::kCount; ++m) {
    const std::string name(StateString(m));
    const bool family =
        name.find("_slope[") != std::string::npos || name.find("_slab[") != std::string::npos;
    if (family) {
      CAPTURE(name);
      CHECK(Placeable(m));
      ++shaped;
    }
  }
  CHECK(shaped == 8 * (144 + 4));
  CHECK_FALSE(Placeable(Materials::kWater));
}

TEST_CASE(
    "block edit: a shape placed into water is flooded, anywhere else dry; breaking leaves water") {
  VoxelWorld world = Empty();
  for (int x = -4; x <= 4; ++x)
    for (int z = -4; z <= 8; ++z) world.SetVoxel(x, -1, z, Materials::kGrass);
  const std::array<double, 3> eye{0.5, 1.62, 0.5};
  const auto wedge = [](bool flooded) {
    return *ParseState(std::string("dwell:stone_slope[facing=east,flooded=") +
                       (flooded ? "true" : "false") + ",half=bottom,shape=wedge]");
  };
  // Into air: dry, even when the client asks for a flooded state (no water from nothing).
  auto dry = CheckBlockEdit(world, Place({0, -1, 2}, 2, wedge(true)), eye, {});
  REQUIRE(dry.check == EditCheck::kOk);
  CHECK(dry.material == wedge(false));
  // Into water: flooded, whichever state the client sent.
  world.SetVoxel(-2, 0, 0, Materials::kWater);
  for (const bool asked : {false, true}) {
    const auto wet = CheckBlockEdit(world, Place({-2, -1, 0}, 2, wedge(asked)), eye, {});
    REQUIRE(wet.check == EditCheck::kOk);
    CHECK(wet.material == wedge(true));
  }
  // Breaking a flooded shape leaves water; a dry one leaves air.
  world.SetVoxel(1, 0, 2, wedge(true));
  world.SetVoxel(-1, 0, 2, wedge(false));
  const auto leaves_water = CheckBlockEdit(world, Break({1, 0, 2}, 2), eye, {});
  REQUIRE(leaves_water.check == EditCheck::kOk);
  CHECK(leaves_water.material == Materials::kWater);
  const auto leaves_air = CheckBlockEdit(world, Break({-1, 0, 2}, 2), eye, {});
  REQUIRE(leaves_air.check == EditCheck::kOk);
  CHECK(leaves_air.material == Materials::kAir);
}

TEST_CASE("block edit: a slope is checked against players by its true volume, not its cell") {
  VoxelWorld world = Empty();
  for (int x = -4; x <= 4; ++x)
    for (int z = -4; z <= 8; ++z) world.SetVoxel(x, -1, z, Materials::kGrass);
  const std::array<double, 3> eye{0.5, 1.62, 0.5};
  const auto wedge =
      *ParseState("dwell:stone_slope[facing=east,flooded=false,half=bottom,shape=wedge]");
  // A player standing 0.2 m past the low (east) edge of the cell (0, 0, 2): too close for a cube
  // (its face is within the capsule's 0.3 m), clear of the wedge, whose edge there has no height.
  const std::vector<EditCapsule> player{{{1.2, 0.9, 2.5}, 0.3f, 0.6f}};
  CHECK(CheckBlockEdit(world, Place({0, -1, 2}, 2, Materials::kStone), eye, player).check ==
        EditCheck::kIntoPlayer);
  CHECK(CheckBlockEdit(world, Place({0, -1, 2}, 2, wedge), eye, player).check == EditCheck::kOk);
  // On the high (west) side the wedge is as solid as a cube.
  const std::vector<EditCapsule> high{{{-0.2, 0.9, 2.5}, 0.3f, 0.6f}};
  CHECK(CheckBlockEdit(world, Place({0, -1, 2}, 2, wedge), eye, high).check ==
        EditCheck::kIntoPlayer);
}

TEST_CASE("block edit: probes see a block placed into a chunk they read as open sky") {
  // Regression: creating a chunk the air test skipped did not invalidate readers' cached pointer
  // to the shared all-air chunk, so probes kept seeing air where a block had been placed.
  VoxelWorld world(GenerateEmptyChunk, [](const ChunkCoord& c) { return c.y > 0; });
  JoltRuntime runtime;
  JPH::JobSystemSingleThreaded jobs{JPH::cMaxPhysicsJobs};
  PhysicsWorld physics(jobs);
  const dwell::player::VoxelQuery query(world, physics.system());
  CHECK(query.Material(5, 40, 5) == Materials::kAir);  // caches the air chunk
  world.SetVoxel(5, 40, 5, Materials::kStone);
  CHECK(query.Material(5, 40, 5) == Materials::kStone);
}

TEST_CASE("block edit: nothing is placed outside the world's rows or disc") {
  VoxelWorld world = Empty();
  world.SetVoxel(0, dwell::core::kWorldMaxY - 1, 2, Materials::kStone);
  const std::array<double, 3> top{0.5, dwell::core::kWorldMaxY - 0.5, 0.5};
  CHECK(CheckBlockEdit(world, Place({0, dwell::core::kWorldMaxY - 1, 2}, 2, Materials::kStone),
                       {0.5, dwell::core::kWorldMaxY + 0.6, 2.5}, {})
            .check == EditCheck::kOutOfWorld);
  CHECK(CheckBlockEdit(world, Place({0, dwell::core::kWorldMaxY - 1, 2}, 5, Materials::kStone), top,
                       {})
            .check == EditCheck::kOk);
  const std::int32_t rim = dwell::core::kWorldRadius - 1;
  world.SetVoxel(rim, 0, 0, Materials::kStone);
  CHECK(CheckBlockEdit(world, Place({rim, 0, 0}, 0, Materials::kStone), {rim + 1.5, 0.5, 0.5}, {})
            .check == EditCheck::kOutOfWorld);
}

TEST_CASE("edits: a placed block reaches every client streaming its chunk, one revision each") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  JoinStreaming(f, 2);
  f.server.Step();  // chunks around both players
  f.TakeAll();

  // Player 1 stands at (−1, 0, −0.25): two blocks on the ground ahead, in one tick.
  f.Send(1, Place({-1, -1, 2}, 2, Materials::kStone));
  f.Send(1, Place({0, -1, 2}, 2, Materials::kLog));
  f.server.Step();
  CHECK(f.server.world().GetVoxel(-1, 0, 2) == Materials::kStone);
  CHECK(f.server.world().GetVoxel(0, 0, 2) == Materials::kLog);
  CHECK(f.server.world().Find({0, 0, 0})->revision() == 1);
  CHECK(f.server.world().Find({-1, 0, 0})->revision() == 1);
  auto out = f.TakeAll();
  for (const SessionId id : {1u, 2u}) {
    CAPTURE(id);
    const auto mods = Modifications(out[id]);
    REQUIRE(mods.size() == 1);
    CHECK(mods[0].reason == VoxelModificationReason::kEdit);
    // x = −1 lies in chunk −1, x = 0 in chunk 0: one entry per chunk, each at revision 1.
    REQUIRE(mods[0].chunks.size() == 2);
    CHECK(mods[0].chunks[0].coord == ChunkCoordNet{-1, 0, 0});
    CHECK(mods[0].chunks[0].revision == 1);
    CHECK(mods[0].chunks[0].changes ==
          std::vector<VoxelChange>{
              {static_cast<std::uint16_t>(LocalIndex(31, 0, 2)), Materials::kStone}});
    CHECK(mods[0].chunks[1].coord == ChunkCoordNet{0, 0, 0});
    CHECK(mods[0].chunks[1].revision == 1);
    CHECK(mods[0].chunks[1].changes ==
          std::vector<VoxelChange>{
              {static_cast<std::uint16_t>(LocalIndex(0, 0, 2)), Materials::kLog}});
  }

  // Breaking it again: revision 2.
  for (int i = 0; i < 10; ++i) f.server.Step();
  f.TakeAll();
  f.Send(2, Break({0, 0, 2}, 2));
  f.server.Step();
  CHECK(f.server.world().GetVoxel(0, 0, 2) == Materials::kAir);
  out = f.TakeAll();
  const auto mods = Modifications(out[1]);
  REQUIRE(mods.size() == 1);
  REQUIRE(mods[0].chunks.size() == 1);
  CHECK(mods[0].chunks[0].revision == 2);
  CHECK(f.server.StatsOf(2)->edits_applied == 1);
}

TEST_CASE("edits: rate limited, and refused by the edit policy") {
  SUBCASE("a burst of three, then one per BLOCK_EDIT_INTERVAL_MS") {
    Fixture f(Flat());
    JoinStreaming(f, 1);
    for (int x = -3; x <= 1; ++x) f.Send(1, Place({x, -1, 2}, 2, Materials::kDirt));
    f.server.Step();
    CHECK(f.server.StatsOf(1)->edits_applied == 3);
    CHECK(f.server.StatsOf(1)->edits_rejected == 2);
    const int interval = kBlockEditIntervalMs * kSimHz / 1000;
    for (int i = 0; i < interval; ++i) f.server.Step();
    f.Send(1, Place({0, -1, 2}, 2, Materials::kDirt));
    f.Send(1, Place({1, -1, 2}, 2, Materials::kDirt));
    f.server.Step();
    CHECK(f.server.StatsOf(1)->edits_applied == 4);
  }
  SUBCASE("nobody may edit") {
    ServerConfig config = Flat();
    config.edits = EditPolicy::kNobody;
    Fixture f(config);
    JoinStreaming(f, 1);
    f.Send(1, Place({-1, -1, 2}, 2, Materials::kDirt));
    f.server.Step();
    CHECK(f.server.StatsOf(1)->edits_applied == 0);
    CHECK(f.server.world().GetVoxel(-1, 0, 2) == Materials::kAir);
  }
  SUBCASE("only ops may edit") {
    ServerConfig config = Flat();
    config.edits = EditPolicy::kOps;
    config.ops = {Client(2).public_key};
    Fixture f(config);
    JoinStreaming(f, 1);
    JoinStreaming(f, 2);
    f.Send(1, Place({-1, -1, 2}, 2, Materials::kDirt));
    f.Send(2, Place({0, -1, 2}, 2, Materials::kDirt));
    f.server.Step();
    CHECK(f.server.world().GetVoxel(-1, 0, 2) == Materials::kAir);
    CHECK(f.server.world().GetVoxel(0, 0, 2) == Materials::kDirt);
  }
  SUBCASE("bedrock stays; the rejection is recorded") {
    Fixture f(Flat());
    JoinStreaming(f, 1);
    f.server.world().SetVoxel(-1, -1, 2, Materials::kBedrock);
    f.Send(1, Break({-1, -1, 2}, 2));
    f.server.Step();
    CHECK(f.server.StatsOf(1)->edits_rejected == 1);
    CHECK(f.server.StatsOf(1)->last_edit_check == EditCheck::kUnbreakable);
  }
}

TEST_CASE("edits: resync re-sends chunks the client has, with their current revision") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  f.server.Step();
  f.Send(1, Place({0, -1, 2}, 2, Materials::kSand));
  f.server.Step();
  f.TakeAll();
  f.Send(1, ChunkResync{{{0, 0, 0}, {0, -1, 0}, {50, 0, 0}}});  // the last one was never streamed
  auto out = f.TakeAll()[1];
  REQUIRE(out.size() == 2);
  const auto& edited = std::get<ChunkData>(out[0]);
  CHECK(edited.form == ChunkForm::kExplicit);
  CHECK(edited.revision == 1);
  CHECK(edited.voxels[LocalIndex(0, 0, 2)] == Materials::kSand);
  CHECK(std::get<ChunkData>(out[1]).form == ChunkForm::kGenerated);
  CHECK(f.server.StatsOf(1)->resyncs == 2);
}

TEST_CASE("edits: an edited chunk the air test calls empty is kept and streamed explicitly") {
  ServerConfig config;
  config.world_seed = 5;
  Fixture f(config);
  const Welcome w = f.Join(1, Client(1));
  const auto spawn = f.server.players().Position(*f.server.PlayerHandleOf(w.player_id));
  const std::int32_t x = static_cast<std::int32_t>(std::floor(spawn.GetX()));
  const std::int32_t z = static_cast<std::int32_t>(std::floor(spawn.GetZ()));
  constexpr std::int32_t y = 4000;  // open sky
  const ChunkCoord sky = ChunkOf(x, y, z + 2);
  // A floating block (tests put it directly), then the player next to it breaks it.
  f.server.world().SetVoxel(x, y, z + 2, Materials::kStone);
  f.MoveTo(w.player_id, x + 0.5f, y - 1.0f, z + 0.5f);
  f.Send(1, WorldgenCheck{0});  // full-chunk mode
  std::vector<ChunkData> chunks = f.Chunks(1, 20);
  const auto it = std::find_if(chunks.begin(), chunks.end(),
                               [&](const ChunkData& m) { return CoordOf(m) == sky; });
  REQUIRE(it != chunks.end());
  CHECK(it->form == ChunkForm::kExplicit);
  CHECK(it->revision == 1);
  CHECK(std::count_if(chunks.begin(), chunks.end(),
                      [](const ChunkData& m) { return m.form == ChunkForm::kAir; }) > 0);

  f.Send(1, Break({x, y, z + 2}, 5));
  f.server.Step();
  CHECK(f.server.StatsOf(w.player_id)->last_edit_check == EditCheck::kOk);
  CHECK(f.server.world().GetVoxel(x, y, z + 2) == Materials::kAir);
  CHECK(f.server.world().Find(sky)->revision() == 2);  // modified: kept, not read as air
}
