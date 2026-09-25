#include "dwell/player/net.h"

#include <algorithm>
#include <cmath>

namespace dwell::player {
namespace {

using protocol::ControllerFlags::kClimbing;
using protocol::ControllerFlags::kCrouching;
using protocol::ControllerFlags::kGrounded;
using protocol::ControllerFlags::kHasReleased;
using protocol::ControllerFlags::kJumping;
using protocol::ControllerFlags::kSwimming;

// Rounds half up, like JavaScript's Math.round (the client's quantizer).
int RoundHalfUp(float v) { return static_cast<int>(std::floor(v + 0.5f)); }

}  // namespace

std::int16_t QuantizeYaw(float degrees) {
  const int turns = RoundHalfUp(degrees / 360.0f * 65536.0f);
  return static_cast<std::int16_t>(static_cast<std::uint16_t>(turns & 0xFFFF));
}

std::int16_t QuantizePitch(float degrees) {
  return static_cast<std::int16_t>(
      RoundHalfUp(std::clamp(degrees, -90.0f, 90.0f) / 90.0f * 32767.0f));
}

float DequantizeYaw(std::int16_t q) { return static_cast<float>(q) * (360.0f / 65536.0f); }
float DequantizePitch(std::int16_t q) { return static_cast<float>(q) * (90.0f / 32767.0f); }

protocol::InputFrame QuantizeInput(const Input& input, std::uint32_t seq) {
  protocol::InputFrame f;
  f.seq = seq;
  // Clamp to the unit circle first: the server rejects longer move vectors (§11).
  float mx = input.move_x, my = input.move_y;
  const float length = std::sqrt(mx * mx + my * my);
  if (length > 1.0f) {
    mx /= length;
    my /= length;
  }
  f.move_x = static_cast<std::int8_t>(RoundHalfUp(std::clamp(mx, -1.0f, 1.0f) * 127.0f));
  f.move_y = static_cast<std::int8_t>(RoundHalfUp(std::clamp(my, -1.0f, 1.0f) * 127.0f));
  f.buttons = static_cast<std::uint16_t>((input.jump ? protocol::InputButtons::kJump : 0) |
                                         (input.run ? protocol::InputButtons::kRun : 0) |
                                         (input.crouch ? protocol::InputButtons::kCrouch : 0));
  f.yaw = QuantizeYaw(input.look_yaw);
  f.pitch = QuantizePitch(input.look_pitch);
  return f;
}

Input DequantizeInput(const protocol::InputFrame& f) {
  Input input;
  input.move_x = static_cast<float>(f.move_x) / 127.0f;
  input.move_y = static_cast<float>(f.move_y) / 127.0f;
  input.jump = (f.buttons & protocol::InputButtons::kJump) != 0;
  input.run = (f.buttons & protocol::InputButtons::kRun) != 0;
  input.crouch = (f.buttons & protocol::InputButtons::kCrouch) != 0;
  input.look_yaw = DequantizeYaw(f.yaw);
  input.look_pitch = DequantizePitch(f.pitch);
  return input;
}

protocol::ControllerState ToNet(const PlayerController& c, const GroundToNet& ground) {
  protocol::ControllerState s;
  s.flags = static_cast<std::uint8_t>(
      (c.ground.grounded ? kGrounded : 0) | (c.jump.jumping ? kJumping : 0) |
      (c.crouch.crouching ? kCrouching : 0) | (c.climb.climbing ? kClimbing : 0) |
      (c.climb.has_released ? kHasReleased : 0) | (c.swim.swimming ? kSwimming : 0));
  s.current_x = c.horizontal.current.GetX();
  s.current_z = c.horizontal.current.GetZ();
  s.external_x = c.horizontal.external.GetX();
  s.external_z = c.horizontal.external.GetZ();
  s.contribution_x = c.horizontal.contribution.GetX();
  s.contribution_z = c.horizontal.contribution.GetZ();
  s.accumulated_y = c.vertical.accumulated_y;
  s.platform_y = c.vertical.platform_y;
  s.target_y = c.vertical.target_y;
  s.ground_velocity_y = c.platform.ground_velocity.GetY();
  s.ground_kind = static_cast<protocol::GroundKind>(c.platform.ground.kind);
  s.ground_id = ground ? ground(c.platform.ground) : 0;
  s.buffer_ticks = static_cast<std::uint8_t>(std::min<std::uint16_t>(c.jump.buffer_ticks, 255));
  s.coyote_ticks = static_cast<std::uint8_t>(std::min<std::uint16_t>(c.jump.coyote_ticks, 255));
  s.step_grace = c.vertical.step_grace;
  s.ladder_x = c.climb.ladder.x;
  s.ladder_y = c.climb.ladder.y;
  s.ladder_z = c.climb.ladder.z;
  s.released_x = c.climb.released.x;
  s.released_z = c.climb.released.z;
  return s;
}

void FromNet(const protocol::ControllerState& s, const GroundFromNet& ground, PlayerController& c) {
  c.ground.grounded = (s.flags & kGrounded) != 0;
  c.jump.jumping = (s.flags & kJumping) != 0;
  c.crouch.crouching = (s.flags & kCrouching) != 0;
  c.climb.climbing = (s.flags & kClimbing) != 0;
  c.climb.has_released = (s.flags & kHasReleased) != 0;
  c.swim.swimming = (s.flags & kSwimming) != 0;
  c.horizontal.current = Vec3(s.current_x, 0.0f, s.current_z);
  c.horizontal.external = Vec3(s.external_x, 0.0f, s.external_z);
  c.horizontal.contribution = Vec3(s.contribution_x, 0.0f, s.contribution_z);
  c.vertical.accumulated_y = s.accumulated_y;
  c.vertical.platform_y = s.platform_y;
  c.vertical.target_y = s.target_y;
  c.vertical.step_grace = s.step_grace;
  c.platform.ground = ground ? ground(s.ground_kind, s.ground_id)
                             : GroundRef{static_cast<GroundRef::Kind>(s.ground_kind), 0};
  c.platform.ground_velocity = Vec3(0.0f, s.ground_velocity_y, 0.0f);
  c.jump.buffer_ticks = s.buffer_ticks;
  c.jump.coyote_ticks = s.coyote_ticks;
  c.climb.ladder = {s.ladder_x, s.ladder_y, s.ladder_z};
  c.climb.released = {s.released_x, 0, s.released_z};
}

std::uint8_t PlayerFlagsOf(const PlayerController& c, bool dead) {
  using namespace protocol::PlayerFlags;
  return static_cast<std::uint8_t>(
      (c.ground.grounded ? kGrounded : 0) | (c.crouch.crouching ? kCrouched : 0) |
      (c.climb.climbing ? kClimbing : 0) | (c.swim.swimming ? kSwimming : 0) | (dead ? kDead : 0));
}

}  // namespace dwell::player
