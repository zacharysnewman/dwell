// The server's side of level of detail (ARCHITECTURE.md §6.6, ADR 0012): propagation of chunk
// changes up the octree, the LOD index and its updates, and LodRequest / LodData.
#include <doctest/doctest.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <map>
#include <memory>
#include <thread>
#include <tuple>

#include "dwell/core/lod.h"
#include "dwell/core/lod_propagation.h"
#include "server_fixture.h"

using namespace dwell::test;

namespace {

namespace M = Materials;

// A chunk's ancestor at `level`.
LodCoord Above(const ChunkCoord& c, int level) { return LodAncestor(LodOfChunk(c), level); }

LodSectionRequest Ask(const LodCoord& c, std::uint32_t known = 0) {
  return {static_cast<std::uint8_t>(c.level), {c.i, c.j, c.k}, known};
}

// The client side of the verification check (§6.3) on the flat world.
void Verify(Fixture& f, SessionId id, const Welcome& w, bool full = false) {
  Chunk v;
  GenerateFlatChunk({w.verification_chunk[0], w.verification_chunk[1], w.verification_chunk[2]}, v);
  f.Send(id, WorldgenCheck{full ? 0 : ChunkHash(v)});
}

// What a client received on the lod stream.
struct LodInbox {
  std::vector<LodIndex> index;
  std::vector<LodIndexUpdate> updates;
  std::vector<LodData> data;
  void Add(std::vector<Message>& messages) {
    for (auto& m : messages) {
      if (auto* i = std::get_if<LodIndex>(&m)) index.push_back(*i);
      if (auto* u = std::get_if<LodIndexUpdate>(&m)) updates.push_back(*u);
      if (auto* d = std::get_if<LodData>(&m)) data.push_back(*d);
    }
  }
  const LodData* Find(const LodCoord& c) const {
    for (auto it = data.rbegin(); it != data.rend(); ++it) {
      if (it->level == c.level && it->section == std::array<std::int32_t, 3>{c.i, c.j, c.k})
        return &*it;
    }
    return nullptr;
  }
};

// Steps once and files each session's messages.
void StepInto(Fixture& f, std::map<SessionId, LodInbox>& boxes) {
  f.server.Step();
  for (auto& [id, messages] : f.TakeAll()) boxes[id].Add(messages);
}

// A chunk filled with stone.
std::unique_ptr<Chunk> Stone() {
  auto c = std::make_unique<Chunk>();
  c->generation_voxels().fill(M::kStone);
  c->SetRevision(1);
  return c;
}

}  // namespace

TEST_SUITE("lod: propagation") {
  TEST_CASE("a chunk change is downsampled into its sections up to the root, each a new revision") {
    LodPropagation lod(GenerateFlatLod, 0);
    const ChunkCoord edited{0, 0, 0};  // above the flat ground: air when generated
    const auto stone = Stone();
    const LodPropagation::ModifiedChunk modified = [&](const ChunkCoord& c) -> const Chunk* {
      return c == edited ? stone.get() : nullptr;
    };
    lod.MarkChunk(edited);
    lod.Drain({}, modified);
    std::uint32_t previous = 0;
    for (int level = 1; level <= dwell::core::kLodMaxLevel; ++level) {
      CAPTURE(level);
      const LodSection* s = lod.Find(Above(edited, level));
      REQUIRE(s);
      CHECK(s->revision > previous);  // children are written before their parents
      previous = s->revision;
    }
    CHECK(lod.sections().size() == static_cast<std::size_t>(dwell::core::kLodMaxLevel));
    // The stone cube shows where it fills cells; the rest of the section is as generated.
    for (int level = 1; level <= 6; ++level) {
      CAPTURE(level);
      const LodCoord c = Above(edited, level);
      const auto cells = lod.CellsForClient(c);
      LodCells generated;
      GenerateFlatLod(c, generated);
      const LodOrigin o = LodSectionOrigin(c);
      const std::int64_t size = LodCellSize(level);
      // The cell holding the chunk's lowest corner.
      const auto x = static_cast<int>((0 - o.x) / size), y = static_cast<int>((0 - o.y) / size),
                 z = static_cast<int>((0 - o.z) / size);
      const auto at = static_cast<std::size_t>(LodCell(x, y, z));
      CHECK(generated[at] == M::kAir);
      CHECK(cells[at] == (level <= 5 ? M::kStone : M::kAir));  // 32 m of stone: one level-5 cell
      int differ = 0;
      for (std::size_t i = 0; i < cells.size(); ++i) differ += cells[i] != generated[i];
      CHECK((differ > 0) == (level <= 5));
    }
  }

  TEST_CASE("propagation is budgeted per tick and runs on threads with the same result") {
    const auto stone = Stone();
    const LodPropagation::ModifiedChunk modified = [&](const ChunkCoord& c) -> const Chunk* {
      return c.y == 0 ? stone.get() : nullptr;
    };
    // 40 changed chunks in a row: 40 level-1 sections... fewer (2 chunks per section along x).
    LodPropagation inline_lod(GenerateFlatLod, 0);
    for (int x = 0; x < 40; ++x) inline_lod.MarkChunk({x * 2, 0, 0});
    inline_lod.Step({{0, 0, 0}}, modified, 8, std::chrono::hours(1));
    const auto first = inline_lod.TakeWritten();
    CHECK(first.size() == 8);
    // Nearest to the player first, lowest level first.
    for (const LodCoord& c : first) {
      CHECK(c.level == 1);
      CHECK(LodSectionOrigin(c).x < 8 * 64 + 64);
    }
    inline_lod.Drain({{0, 0, 0}}, modified);

    LodPropagation threaded(GenerateFlatLod, 2);
    for (int x = 0; x < 40; ++x) threaded.MarkChunk({x * 2, 0, 0});
    threaded.Drain({{0, 0, 0}}, modified);
    REQUIRE(threaded.sections().size() == inline_lod.sections().size());
    for (const auto& [c, s] : inline_lod.sections()) {
      REQUIRE(threaded.Find(c));
      CHECK(threaded.Find(c)->encoded == s.encoded);
    }
  }
}

TEST_SUITE("lod: streaming") {
  TEST_CASE("the index follows the worldgen check; untouched worlds cost no LodData") {
    Fixture f(Flat());
    std::map<SessionId, LodInbox> boxes;
    const Welcome w = f.Join(1, Client(1));
    for (int i = 0; i < 5; ++i) StepInto(f, boxes);
    CHECK(boxes[1].index.empty());  // nothing before the check
    Verify(f, 1, w);
    for (int i = 0; i < 120; ++i) StepInto(f, boxes);
    REQUIRE(boxes[1].index.size() == 1);
    CHECK(boxes[1].index[0].last);
    CHECK(boxes[1].index[0].entries.empty());
    CHECK(boxes[1].updates.empty());
    CHECK(boxes[1].data.empty());
  }

  TEST_CASE("an edit reaches a far client: index update, then Explicit only where it changed") {
    // A builds near the spawn; B is 50 km away and holds sections over A's area; C, 60 km the
    // other way, has no modification in view and requests nothing.
    Fixture f(Flat());
    std::map<SessionId, LodInbox> boxes;
    const Welcome a = f.Join(1, Client(1));
    const Welcome b = f.Join(2, Client(2));
    const Welcome c = f.Join(3, Client(3));
    f.MoveTo(b.player_id, 50000.5f, 0.0f, 0.5f);
    f.MoveTo(c.player_id, -60000.5f, 0.0f, 0.5f);
    for (const auto& [id, w] : {std::pair{1u, a}, std::pair{2u, b}, std::pair{3u, c}}) {
      Verify(f, id, w);
    }
    for (int i = 0; i < 5; ++i) StepInto(f, boxes);

    // B holds these sections over the spawn, all generated so far (the index is empty).
    const ChunkCoord site1{-1, 0, 0};               // cell (-1, 0, 2)
    const ChunkCoord site2 = ChunkOf(-5001, 0, 2);  // another level-7 section, same level 8
    const LodCoord index_section = Above(site1, dwell::core::kLodIndexLevel);
    REQUIRE(Above(site2, dwell::core::kLodIndexLevel) == index_section);
    REQUIRE_FALSE(Above(site2, 7) == Above(site1, 7));

    f.Send(1, BlockEditRequest{BlockEditAction::kPlace, {-1, -1, 2}, 2, M::kLog});
    const std::uint32_t edit_tick = f.server.tick();
    // The server's sections above the edit change within a bounded time, and B hears of it.
    int ticks = 0;
    const auto updated = [&] {
      for (const auto& u : boxes[2].updates)
        for (const auto& e : u.entries)
          if (e.i == index_section.i && e.k == index_section.k) return true;
      return false;
    };
    while (!updated() && ticks < 600) {
      StepInto(f, boxes);
      ++ticks;
    }
    REQUIRE(updated());
    MESSAGE("index update reached a far client " << ticks << " ticks after the edit");
    CHECK(ticks <= 60);  // ≤ 1 s
    while (!f.server.lod().Find({dwell::core::kLodMaxLevel, 0, 0, 0}) &&
           f.server.tick() < edit_tick + 600) {
      StepInto(f, boxes);
    }
    CHECK(f.server.lod().Find({dwell::core::kLodMaxLevel, 0, 0, 0}));
    CHECK(f.server.tick() - edit_tick <= 60);  // up to the root within a second

    // B re-requests what it holds under the entry, and the entry's ancestors.
    std::vector<LodSectionRequest> held = {Ask(index_section), Ask(Above(site1, 7)),
                                           Ask(Above(site2, 7)), Ask(Above(site1, 3))};
    for (int level = dwell::core::kLodIndexLevel + 1; level <= dwell::core::kLodMaxLevel; ++level) {
      held.push_back(Ask(Above(site1, level)));
    }
    f.Send(2, LodRequest{held});
    for (int i = 0; i < 10; ++i) StepInto(f, boxes);
    const LodInbox& inbox = boxes[2];
    const LodData* entry = inbox.Find(index_section);
    const LodData* path7 = inbox.Find(Above(site1, 7));
    const LodData* other7 = inbox.Find(Above(site2, 7));
    const LodData* path3 = inbox.Find(Above(site1, 3));
    REQUIRE((entry && path7 && other7 && path3));
    CHECK(entry->form == LodForm::kExplicit);
    CHECK(entry->cells.size() == static_cast<std::size_t>(kLodCellCount));
    CHECK(path7->form == LodForm::kExplicit);
    CHECK(path3->form == LodForm::kExplicit);
    CHECK(other7->form == LodForm::kGenerated);  // nothing modified there: B generates it
    for (int level = dwell::core::kLodIndexLevel + 1; level <= dwell::core::kLodMaxLevel; ++level) {
      const LodData* d = inbox.Find(Above(site1, level));
      REQUIRE(d);
      CHECK(d->form == LodForm::kExplicit);
    }

    // A second edit elsewhere under the same entry: B asks again with what it holds. Only the
    // changed path comes back Explicit; the first path's section is Unchanged.
    const std::uint32_t known_entry = entry->revision, known_path7 = path7->revision;
    const std::size_t updates_before = boxes[2].updates.size();
    f.MoveTo(a.player_id, -4999.5f, 0.0f, 0.5f);
    for (int i = 0; i < 3; ++i) StepInto(f, boxes);
    f.Send(1, BlockEditRequest{BlockEditAction::kPlace, {-5001, -1, 2}, 2, M::kLog});
    for (int i = 0; i < 120 && boxes[2].updates.size() == updates_before; ++i) StepInto(f, boxes);
    REQUIRE(boxes[2].updates.size() > updates_before);
    const std::uint64_t bytes_before = f.server.LodStatsOf(b.player_id)->bytes;
    const std::uint32_t explicit_before = f.server.LodStatsOf(b.player_id)->explicit_sent;
    f.Send(2, LodRequest{{Ask(index_section, known_entry), Ask(Above(site1, 7), known_path7),
                          Ask(Above(site2, 7), 0)}});
    for (int i = 0; i < 10; ++i) StepInto(f, boxes);
    CHECK(boxes[2].Find(index_section)->form == LodForm::kExplicit);
    CHECK(boxes[2].Find(index_section)->revision > known_entry);
    CHECK(boxes[2].Find(Above(site1, 7))->form == LodForm::kUnchanged);
    CHECK(boxes[2].Find(Above(site2, 7))->form == LodForm::kExplicit);
    CHECK(f.server.LodStatsOf(b.player_id)->explicit_sent == explicit_before + 2);
    MESSAGE("second round: " << f.server.LodStatsOf(b.player_id)->bytes - bytes_before
                             << " bytes for 3 sections");

    // C saw the index and its updates, and no section data.
    CHECK(boxes[3].index.size() == 1);
    CHECK_FALSE(boxes[3].updates.empty());
    CHECK(boxes[3].data.empty());
    CHECK(f.server.LodStatsOf(c.player_id)->explicit_sent == 0);
  }

  TEST_CASE("requests are rate-limited and answered within the byte budget") {
    ServerConfig config = Flat();
    config.lod_bytes_per_second = 60 * 2000;  // 2 KB per tick
    Fixture f(config);
    std::map<SessionId, LodInbox> boxes;
    const Welcome w = f.Join(1, Client(1));
    Verify(f, 1, w, /*full=*/true);  // full-chunk mode: every answer Explicit (~1–6 KB)
    StepInto(f, boxes);
    // 3 × 32 sections at once: the bucket holds LOD_REQUESTS_PER_SECOND.
    const LodCoord base = Above({0, -1, 0}, 2);
    for (int m = 0; m < 3; ++m) {
      LodRequest r;
      for (int i = 0; i < 32; ++i)
        r.sections.push_back(Ask({2, base.i + m * 32 + i, base.j, base.k}));
      f.Send(1, r);
    }
    const LodStats after = *f.server.LodStatsOf(w.player_id);
    CHECK(after.requests == static_cast<std::uint32_t>(kLodRequestsPerSecond));
    CHECK(after.requests_dropped == 96 - static_cast<std::uint32_t>(kLodRequestsPerSecond));
    // Bytes per tick stay within the budget (one message may overdraw it).
    std::size_t max_tick = 0;
    for (int i = 0; i < 600 && boxes[1].data.size() < 64; ++i) {
      std::size_t bytes = 0;
      f.server.Step();
      for (auto& o : f.server.TakeOutbox()) {
        if (o.channel != Channel::kLod || o.kind != Outgoing::Kind::kReliable) continue;
        bytes += o.bytes.size();
        auto m = Decode(o.bytes);
        REQUIRE(m);
        std::vector<Message> one{std::move(*m)};
        boxes[1].Add(one);
      }
      max_tick = std::max(max_tick, bytes);
      if (config.lod_threads != 0) std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    REQUIRE(boxes[1].data.size() == 64);
    CHECK(max_tick <= 2000 + 8000);
    // Full mode: unmodified sections come Explicit, as generated.
    const LodData& d = boxes[1].data.front();
    CHECK(d.form == LodForm::kExplicit);
    LodCells generated;
    GenerateFlatLod({d.level, d.section[0], d.section[1], d.section[2]}, generated);
    CHECK(d.cells == generated);
  }

  TEST_CASE("Explicit sections carry their modified neighbours' borders in the apron") {
    LodPropagation lod(GenerateFlatLod, 0);
    const auto stone = Stone();
    // Two chunks in neighbouring level-1 sections along x.
    const ChunkCoord left{-1, 0, 0}, right{0, 0, 0};
    REQUIRE_FALSE(Above(left, 1) == Above(right, 1));
    lod.MarkChunk(left);
    lod.MarkChunk(right);
    lod.Drain({}, [&](const ChunkCoord& c) -> const Chunk* {
      return c == left || c == right ? stone.get() : nullptr;
    });
    const auto cells = lod.CellsForClient(Above(right, 1));
    // The right section's −x apron is the left section's stone border; a generated apron would
    // be air there (above the ground).
    const int y = static_cast<int>((0 - LodSectionOrigin(Above(right, 1)).y) / 2);
    CHECK(cells[static_cast<std::size_t>(LodCell(-1, y, 0))] == M::kStone);
    LodCells generated;
    GenerateFlatLod(Above(right, 1), generated);
    CHECK(generated[static_cast<std::size_t>(LodCell(-1, y, 0))] == M::kAir);
  }
}

TEST_SUITE("lod: builds from afar") {
  TEST_CASE("a large build is in the sections drawn from 50 km away and from altitude") {
    // The level the client draws at distance d (LOD_PIXEL_ERROR 4 px, 1080 px at 75°: ~703 px
    // per radian): the coarsest whose cells still project within 4 px.
    const auto drawn_level = [](double d) {
      int level = 1;
      while ((std::ldexp(1.0, level + 1) / d) * 703.0 <= 4.0) ++level;
      return level;
    };
    const int at_50km = drawn_level(50'000), at_100km = drawn_level(100'000);
    CHECK(at_50km == 8);
    CHECK(at_100km == 9);
    // A wall 512 m wide, 512 m tall and 1 m thick on the flat ground (x = 1000, z 0..511).
    constexpr int kX = 1000;
    std::map<std::tuple<int, int, int>, std::unique_ptr<Chunk>> built;
    for (int cy = 0; cy < 16; ++cy)
      for (int cz = 0; cz < 16; ++cz) {
        auto chunk = std::make_unique<Chunk>();
        GenerateFlatChunk({kX / 32, cy, cz}, *chunk);
        for (int y = 0; y < 32; ++y)
          for (int z = 0; z < 32; ++z) chunk->Set(kX % 32, y, z, Materials::kStone);
        built[{kX / 32, cy, cz}] = std::move(chunk);
      }
    LodPropagation lod(GenerateFlatLod, 0);
    for (const auto& [c, chunk] : built)
      lod.MarkChunk({std::get<0>(c), std::get<1>(c), std::get<2>(c)});
    lod.Drain({}, [&](const ChunkCoord& c) -> const Chunk* {
      const auto it = built.find({c.x, c.y, c.z});
      return it == built.end() ? nullptr : it->second.get();
    });
    for (const int level : {at_50km, at_100km}) {
      CAPTURE(level);
      // The section holding the wall's middle, 256 m up.
      const std::int64_t s = LodSectionSize(level), cell = LodCellSize(level);
      const LodCoord c{level, static_cast<std::int32_t>((kX - kLodOriginX) / s),
                       static_cast<std::int32_t>((256 - kLodOriginY) / s),
                       static_cast<std::int32_t>((256 - kLodOriginZ) / s)};
      const auto cells = lod.CellsForClient(c);
      REQUIRE(cells.size() == static_cast<std::size_t>(kLodVolume));
      LodCells generated;
      GenerateFlatLod(c, generated);
      const LodOrigin o = LodSectionOrigin(c);
      const auto at = [&](std::int64_t x, std::int64_t y, std::int64_t z) {
        return static_cast<std::size_t>(LodCell(static_cast<int>((x - o.x) / cell),
                                                static_cast<int>((y - o.y) / cell),
                                                static_cast<int>((z - o.z) / cell)));
      };
      CHECK(generated[at(kX, 256, 256)] == Materials::kAir);
      CHECK(cells[at(kX, 256, 256)] == Materials::kStone);  // the wall, in mid air above ground
      CHECK(cells[at(kX + 4 * cell, 256, 256)] == Materials::kAir);
    }
  }
}
