#pragma once

#include <cmath>
#include <cstdint>

#include "dwell/protocol/constants.gen.h"

// PlayerControllerConfig (PLAYER_CONTROLLER.md §7): the PPC's recommended feel with voxel
// dimensions. Rows that differ from the PPC default are marked ◆ in the spec. Durations are stored
// as ticks at SIM_HZ.
namespace dwell::player {

struct PlayerControllerConfig {
  struct Body {
    float radius = 0.3f;           // ◆ fits 1-wide gaps
    float standing_height = 1.8f;  // ◆ fits 2-tall doorways
    float mass = 80.0f;            // ◆ real mass (kg)
    float gravity = 20.0f;         // m/s², absolute (PPC: 2 × 10)
    float eye_height = 1.62f;      // ◆ client camera, above the feet
    float crouch_eye_height = 0.8f;
  } body;
  struct Movement {
    float walk_speed = 5.0f;
    float run_speed = 8.0f;
    float acceleration = 50.0f;
    float deceleration = 12.0f;
    float reverse_deceleration = 60.0f;
    float air_control = 0.2f;
    float max_step_height = 0.55f;  // ◆ slabs (0.5 m) step up; full blocks need a jump
    float air_external_drag = 0.0f;
    float ground_external_friction = 15.0f;
    bool carried_by_characters = false;
    bool auto_jump = false;              // ◆ Dwell addition (touch preset: true)
    bool edge_guard = false;             // ◆ Dwell addition
    float max_push_force = 800.0f;       // ◆ contact mass scaling (Phase 4)
    float pushable_mass_limit = 400.0f;  // ◆
  } movement;
  struct Probes {
    float ground_probe_margin = 0.15f;
    float ceiling_probe_margin = 0.10f;
    float max_slope_angle = 45.0f;  // degrees
  } probes;
  struct Jump {
    float height = 1.25f;
    std::uint16_t buffer_ticks = 12;  // 0.2 s
    std::uint16_t coyote_ticks = 6;   // 0.1 s
  } jump;
  struct Crouch {
    float height = 0.9f;  // ◆ fits 1-tall crawlspaces
    float speed = 1.6f;
    float mid_air_boost = 0.0f;
  } crouch;
  struct Climb {
    float speed = 3.0f;
    float look_down_threshold = 30.0f;  // degrees
    float jump_off_up = 4.0f;           // JumpOffVelocity.Y
    float jump_off_away = 3.0f;         // JumpOffVelocity.Z
    float snap_strength = 10.0f;        // 1/s
  } climb;
  struct Swim {  // ◆ Dwell addition
    float speed = 3.0f;
    float enter_fraction = 0.6f;
    float exit_fraction = 0.4f;
    float float_fraction = 0.7f;  // submerged fraction buoyancy settles at
    float buoyancy = 12.0f;       // m/s² per unit of submersion off float_fraction
    float drag = 2.0f;            // 1/s
  } swim;
  struct Damage {  // ◆ Dwell addition
    float fall_damage_min_speed = 12.0f;
    float fall_damage_per_speed = 8.0f;  // health points per m/s above the minimum
    float crush_speed = 4.0f;
    std::uint16_t crush_ticks = 6;
  } damage;
  struct Advanced {
    float step_probe_distance = 0.01f;
    float probe_ring_radius = 0.9f;  // fraction of the capsule radius
    float wall_check_distance = 0.16f;
    float external_absorb_threshold = 0.01f;
    float max_platform_yaw_speed = 360.0f;  // deg/s
  } advanced;

  float HalfHeight(bool crouching) const {
    return (crouching ? crouch.height : body.standing_height) * 0.5f;
  }
  // How far the capsule centre moves when crouching or standing up.
  float CrouchHeightDelta() const { return HalfHeight(false) - HalfHeight(true); }
  // Take-off speed that reaches jump.height under this config's gravity.
  float JumpVelocity() const { return std::sqrt(2.0f * body.gravity * jump.height); }
  static constexpr float Dt() { return 1.0f / protocol::kSimHz; }
};

// The default desktop preset.
inline PlayerControllerConfig DefaultConfig() { return {}; }

// Touch preset: auto-jump on (PLAYER_CONTROLLER.md §6.2).
inline PlayerControllerConfig TouchConfig() {
  PlayerControllerConfig c;
  c.movement.auto_jump = true;
  return c;
}

// The Unity package's original numbers (PPC ApplyUnityParity), with Dwell's body dimensions; kept
// for comparison testing.
inline PlayerControllerConfig UnityParityConfig() {
  PlayerControllerConfig c;
  c.body.gravity = 10.0f;
  c.movement.walk_speed = 5.0f;
  c.movement.run_speed = 10.0f;
  c.movement.acceleration = 10.0f;
  c.movement.deceleration = 10.0f;
  c.movement.reverse_deceleration = 20.0f;
  c.movement.air_control = 1.0f;
  c.movement.max_step_height = 0.5f;
  c.movement.air_external_drag = 0.5f;
  c.movement.ground_external_friction = 15.0f;
  c.jump.height = 5.0f;
  c.crouch.speed = 2.0f;
  return c;
}

}  // namespace dwell::player
