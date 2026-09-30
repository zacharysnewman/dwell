// Chunk requests (ARCHITECTURE.md §6.6): beyond the view, the chunks a client wants to draw at full
// detail are streamed on request within the render radius — generated, air or explicit like the
// view's own, with their edits — and unloaded once they leave that radius.
#include <doctest/doctest.h>

#include "server_fixture.h"

using namespace dwell::test;

namespace {

void JoinStreaming(Fixture& f, SessionId id) {
  const Welcome w = f.Join(id, Client(static_cast<std::uint8_t>(id)));
  Chunk v;
  GenerateFlatChunk({w.verification_chunk[0], w.verification_chunk[1], w.verification_chunk[2]}, v);
  f.Send(id, WorldgenCheck{ChunkHash(v)});
}

// Streams the view (the player stands at the origin chunk of the flat world).
void StreamView(Fixture& f) {
  for (int i = 0; i < 20; ++i) f.server.Step();
  f.TakeAll();
}

}  // namespace

TEST_CASE("chunk requests: chunks beyond the view arrive once, after the view") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  StreamView(f);
  const ChunkCoord ground{8, -1, 0};  // 8 chunks out: beyond the view (3), within the render radius
  const ChunkCoord sky{8, 2, 0};
  REQUIRE(kRenderRadiusChunks >= 8);
  f.Send(1, ChunkRequest{{{8, -1, 0}, {8, 2, 0}, {8, -1, 0}}});
  const auto chunks = f.Chunks(1, 3);
  REQUIRE(chunks.size() == 2);
  CHECK(CoordOf(chunks[0]) == ground);
  CHECK(chunks[0].form == ChunkForm::kGenerated);
  CHECK(CoordOf(chunks[1]) == sky);
  CHECK(f.server.StreamStatsOf(1)->requested_sent == 2);
  // Asking again for a chunk the client has sends nothing.
  f.Send(1, ChunkRequest{{{8, -1, 0}}});
  CHECK(f.Chunks(1, 3).empty());
}

TEST_CASE("chunk requests: beyond the render radius, or before the worldgen check, are dropped") {
  Fixture f(Flat());
  f.Join(1, Client(1));
  f.Send(1, ChunkRequest{{{4, -1, 0}}});  // no check yet
  CHECK(f.server.StreamStatsOf(1)->requests_dropped == 1);
  JoinStreaming(f, 2);
  StreamView(f);
  const std::int32_t far = kRenderRadiusChunks + 2;
  f.Send(2, ChunkRequest{{{far, -1, 0}, {0, -1, far}}});
  CHECK(f.Chunks(2, 3).empty());
  CHECK(f.server.StreamStatsOf(2)->requests_dropped == 2);
}

TEST_CASE("chunk requests: a modified chunk arrives explicitly, and its edits follow") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  JoinStreaming(f, 2);
  StreamView(f);
  // Player 2 builds 6 chunks away from the origin, where player 1 does not stream by view.
  f.MoveTo(2, 6 * 32 + 0.5f, 0.0f, 0.5f);
  for (int i = 0; i < 20; ++i) f.server.Step();
  f.TakeAll();
  f.Send(2, BlockEditRequest{BlockEditAction::kPlace, {6 * 32, -1, 2}, 2, Materials::kStone});
  f.server.Step();
  REQUIRE(f.server.world().GetVoxel(6 * 32, 0, 2) == Materials::kStone);
  f.TakeAll();

  f.Send(1, ChunkRequest{{{6, 0, 0}}});
  const auto chunks = f.Chunks(1, 2);
  REQUIRE(chunks.size() == 1);
  CHECK(chunks[0].form == ChunkForm::kExplicit);
  CHECK(chunks[0].revision == 1);
  CHECK(chunks[0].voxels[LocalIndex(0, 0, 2)] == Materials::kStone);

  // The next edit there reaches player 1 too.
  f.Send(2, BlockEditRequest{BlockEditAction::kPlace, {6 * 32 + 1, -1, 2}, 2, Materials::kLog});
  f.server.Step();
  bool edit_seen = false;
  for (const auto& [m, size] : f.Take(1)) {
    if (const auto* mod = std::get_if<VoxelModification>(&m)) {
      for (const auto& c : mod->chunks) edit_seen |= c.coord == ChunkCoordNet{6, 0, 0};
    }
  }
  CHECK(edit_seen);
}

TEST_CASE("chunk requests: a view chunk asked for stays when the view moves on") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  StreamView(f);
  // A ground chunk the view streamed (the player stands near the origin chunk).
  f.Send(1, ChunkRequest{{{1, -1, 0}, {-1, -1, 0}}});
  CHECK(f.Chunks(1, 2).empty());  // held already: nothing to send
  std::vector<ChunkUnload> unloads;
  f.MoveTo(1, 8 * 32 + 0.5f, 0.0f, 0.5f);
  f.Chunks(1, 5, &unloads);
  bool kept = true, other_left = false;
  for (const auto& u : unloads)
    for (const auto& x : u.coords) {
      kept &= !(x == ChunkCoordNet{-1, -1, 0});
      other_left |= x == ChunkCoordNet{-2, -1, 0};
    }
  CHECK(kept);        // 9 chunks behind, within the render radius
  CHECK(other_left);  // its neighbour, never asked for, left with the view
}

TEST_CASE("chunk requests: kept while within the render radius, unloaded beyond it") {
  Fixture f(Flat());
  JoinStreaming(f, 1);
  StreamView(f);
  f.Send(1, ChunkRequest{{{6, -1, 0}}});
  REQUIRE(f.Chunks(1, 2).size() == 1);
  // Three chunks the other way (the view moves on, the requested chunk stays)…
  std::vector<ChunkUnload> unloads;
  f.MoveTo(1, -3 * 32 + 0.5f, 0.0f, 0.5f);
  f.Chunks(1, 5, &unloads);
  const auto unloaded = [&](const ChunkCoordNet& c) {
    for (const auto& u : unloads)
      for (const auto& x : u.coords)
        if (x == c) return true;
    return false;
  };
  REQUIRE(!unloads.empty());     // the view moved: its far side left
  CHECK(!unloaded({6, -1, 0}));  // 9 chunks from the view's center, far beyond view + margin
  // …then beyond the render radius and its margin.
  f.MoveTo(1, -(kRenderRadiusChunks - 2) * 32 + 0.5f, 0.0f, 0.5f);
  f.Chunks(1, 5, &unloads);
  CHECK(unloaded({6, -1, 0}));
}
