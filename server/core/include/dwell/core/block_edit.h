#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

#include "dwell/core/voxel.h"
#include "dwell/protocol/messages.h"

// Block interaction (ARCHITECTURE.md §6.5): the targeting ray cast shared by the client (which
// cell the crosshair is on) and the server (line of sight), and the server's validation of a
// BlockEditRequest (§11). Pure functions of the voxel world and the players' capsules.
namespace dwell::core {

// Face index → outward direction: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
inline constexpr std::array<std::array<int, 3>, 6> kFaceDirs{
    {{1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}, {0, 0, 1}, {0, 0, -1}}};

// Cells the crosshair can target: anything but air and liquids.
bool Targetable(MaterialId m);
// Materials players can place (the infinite creative palette, §6.5): every material but air,
// liquids, the indestructible anchor (bedrock) and debug materials (the launch pad).
bool Placeable(MaterialId m);

struct BlockHit {
  std::array<std::int32_t, 3> cell{};
  int face = 0;  // the face the ray entered through
  float distance = 0.0f;
};

// Nearest targetable cell along the ray (unit `dir`) within `max_distance`, as its shape's box
// (slabs: the bottom half; everything else the whole cell). A box containing the origin is skipped.
// Walks in float relative to the origin's cell, so it is as precise ~8,000 km out as at the origin.
std::optional<BlockHit> RaycastBlock(VoxelWorld& world, const std::array<double, 3>& origin,
                                     const std::array<float, 3>& dir, float max_distance);

// A player's collision capsule (vertical): centre, radius, half the cylinder's height.
struct EditCapsule {
  std::array<double, 3> center{};
  float radius = 0.3f;
  float half_cylinder = 0.6f;
};

enum class EditCheck : std::uint8_t {
  kOk,
  kOutOfReach,     // target farther than reach (+ latency slack) from the eye
  kNoLineOfSight,  // the targeted face cannot be seen from the eye
  kNothingThere,   // Break: target is air or liquid; Place: nothing to place against
  kUnbreakable,    // Break: bedrock
  kNotPlaceable,   // Place: air, liquid, bedrock, launch pad, or an unknown id
  kOccupied,       // Place: the cell holds a block
  kIntoPlayer,     // Place: a solid block would overlap a player capsule
  kOutOfWorld,     // Place: outside the world's rows or disc
};

// Extra reach the server allows over REACH_DISTANCE: its view of the player lags the client's.
inline constexpr float kReachSlack = 1.0f;

struct EditOutcome {
  EditCheck check = EditCheck::kOk;
  std::array<std::int32_t, 3> cell{};  // the cell that changes (Place: next to the target)
  MaterialId material = Materials::kAir;
};

// Validates an edit request from a player whose eye is at `eye` against the world and every
// player's capsule (§6.5, §11). Rate, permissions and liveness are the caller's.
EditOutcome CheckBlockEdit(VoxelWorld& world, const protocol::BlockEditRequest& request,
                           const std::array<double, 3>& eye,
                           const std::vector<EditCapsule>& players,
                           float reach = protocol::kReachDistance + kReachSlack);

// Applies one chunk's changes from a VoxelModification to a (client's) world; the chunk takes the
// modification's revision. A missing chunk is created first (generated, or all air when streamed).
void ApplyChunkChanges(VoxelWorld& world, const protocol::ChunkChanges& changes);

}  // namespace dwell::core
