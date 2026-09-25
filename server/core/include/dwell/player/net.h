#pragma once

#include <cstdint>
#include <functional>

#include "dwell/player/controller.h"
#include "dwell/protocol/messages.h"

// Controller ↔ wire conversions (PLAYER_CONTROLLER.md §8.4). The client quantizes input once (in
// TypeScript, mirroring QuantizeInput) and both the server and the client's WASM prediction
// dequantize the same integers here, so both simulate bit-identical inputs.
namespace dwell::player {

protocol::InputFrame QuantizeInput(const Input& input, std::uint32_t seq);
Input DequantizeInput(const protocol::InputFrame& frame);

// Quantized view angles (also used for remote players in snapshots).
std::int16_t QuantizeYaw(float degrees);
std::int16_t QuantizePitch(float degrees);
float DequantizeYaw(std::int16_t q);
float DequantizePitch(std::int16_t q);

// Maps ground references between a physics world's body ids and network ids (player ids for
// GroundRef::kPlayer). Returns 0 / an invalid ref when unknown.
using GroundToNet = std::function<std::uint16_t(const GroundRef&)>;
using GroundFromNet = std::function<GroundRef(protocol::GroundKind, std::uint16_t)>;

protocol::ControllerState ToNet(const PlayerController& c, const GroundToNet& ground);
// Overwrites the resumable parts of `c` (everything but input, outputs, and per-tick scratch).
void FromNet(const protocol::ControllerState& s, const GroundFromNet& ground, PlayerController& c);

std::uint8_t PlayerFlagsOf(const PlayerController& c, bool dead);

}  // namespace dwell::player
