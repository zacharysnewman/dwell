#include "dwell/player/controller.h"

#include <Jolt/Physics/Body/Body.h>
#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Body/BodyLock.h>
#include <Jolt/Physics/Collision/ContactListener.h>
#include <Jolt/Physics/Collision/Shape/CapsuleShape.h>

#include <algorithm>
#include <array>
#include <cassert>
#include <cmath>
#include <mutex>
#include <numbers>

#include "dwell/core/physics_world.h"
#include "dwell/core/terrain_collision.h"

namespace dwell::player {
namespace {

constexpr float kDt = PlayerControllerConfig::Dt();
constexpr float kRadToDeg = 180.0f / std::numbers::pi_v<float>;
constexpr float kDegToRad = std::numbers::pi_v<float> / 180.0f;

// Constants from the PPC Quantum systems.
constexpr int kRingRays = 16;
constexpr float kRisingMargin = 0.10f;        // PPCProbeSystem
constexpr float kWallMinAngle = 45.0f;        // PPCProbeSystem
constexpr float kInputDeadzone = 0.10f;       // PPCMovementLayerSystem
constexpr float kReverseDot = -0.10f;         // PPCMovementLayerSystem
constexpr float kMinDirection = 0.01f;        // PPCMovementLayerSystem
constexpr float kMinStep = 0.01f;             // PPCMovementLayerSystem
constexpr float kLaunchThreshold = 0.10f;     // PPCVerticalLayerSystem
constexpr float kStepLaunchThreshold = 1.5f;  // Dwell: during step_grace
constexpr std::uint8_t kStepGraceTicks = 6;
constexpr float kAbsorbThreshold = 0.01f;  // PPCVerticalLayerSystem
constexpr float kMovingSpeed = 0.10f;      // PPCStateSystem
constexpr float kCrouchSkin = 0.02f;       // PPCCrouchSystem
constexpr float kLadderInset = 0.10f;      // PPCClimbSystem: slightly inside touching distance
// A voxel ladder is a thin plate on the cell's back face (PLAYER_CONTROLLER.md §6.3).
constexpr float kLadderPlateHalfDepth = 0.05f;
constexpr float kLadderTopReach = 0.6f;  // climbable region above a column's top cell

Vec3 Flat(Vec3 v) { return Vec3(v.GetX(), 0.0f, v.GetZ()); }

Vec3 MoveTowards(Vec3 current, Vec3 target, float max_delta) {
  const Vec3 delta = target - current;
  const float length = delta.Length();
  if (length <= max_delta || length == 0.0f) return target;
  return current + delta * (max_delta / length);
}

Vec3 SafeNormalized(Vec3 v) {
  const float length = v.Length();
  return length > 0.0f ? v / length : Vec3::sZero();
}

float SlopeAngle(Vec3 normal) {
  return std::acos(std::clamp(normal.GetY(), -1.0f, 1.0f)) * kRadToDeg;
}

const std::array<Vec3, kRingRays>& Ring() {
  static const std::array<Vec3, kRingRays> ring = [] {
    std::array<Vec3, kRingRays> r;
    for (int i = 0; i < kRingRays; ++i) {
      const float angle = static_cast<float>(i) * (2.0f * std::numbers::pi_v<float> / kRingRays);
      r[i] = Vec3(std::cos(angle), 0.0f, std::sin(angle));
    }
    return r;
  }();
  return ring;
}

Vec3 FacingVector(core::Facing facing) {
  switch (facing) {
    case core::Facing::kNorth:
      return Vec3(0.0f, 0.0f, -1.0f);
    case core::Facing::kEast:
      return Vec3(1.0f, 0.0f, 0.0f);
    case core::Facing::kSouth:
      return Vec3(0.0f, 0.0f, 1.0f);
    case core::Facing::kWest:
      return Vec3(-1.0f, 0.0f, 0.0f);
    case core::Facing::kNone:
      break;
  }
  return Vec3(0.0f, 0.0f, 1.0f);
}

JPH::RefConst<JPH::Shape> MakeCapsule(const PlayerControllerConfig& cfg, bool crouching) {
  const float radius = cfg.body.radius;
  const float half_cylinder = std::max(0.0f, cfg.HalfHeight(crouching) - radius);
  auto* shape = new JPH::CapsuleShape(half_cylinder, radius);
  // Both capsules weigh body.mass, so swapping shapes keeps the mass (SetShape recomputes it).
  const float pi = std::numbers::pi_v<float>;
  const float volume =
      pi * radius * radius * (2.0f * half_cylinder) + 4.0f / 3.0f * pi * radius * radius * radius;
  shape->SetDensity(cfg.body.mass / volume);
  return shape;
}

}  // namespace

// Dwell's world is right-handed with Y up (Jolt, Three.js): facing +Z (yaw 0), right is −X.
// (The PPC's Unity convention is left-handed, where it would be +X.)
Vec3 CameraRight(float yaw_degrees) {
  const float yaw = yaw_degrees * kDegToRad;
  return Vec3(-std::cos(yaw), 0.0f, std::sin(yaw));
}

Vec3 MoveDirection(const Input& input) {
  const float yaw = input.look_yaw * kDegToRad;
  const Vec3 forward(std::sin(yaw), 0.0f, std::cos(yaw));
  return forward * input.move_y + CameraRight(input.look_yaw) * input.move_x;
}

struct Players::Player {
  static constexpr int kMaxContacts = 8;
  PlayerController c;
  Input pending;
  const PlayerControllerConfig* cfg;
  JPH::BodyID body;
  JPH::RefConst<JPH::Shape> standing, crouched;
  // Push-out normals (towards the player) of this body's contacts in the last physics step.
  std::array<Vec3, kMaxContacts> contact_normals;
  int contact_count = 0;
};

// Records, for every player body, the normals of its contacts during PhysicsSystem::Update (which
// may call back from several threads). The body's user data is its player handle + 1.
class Players::Contacts final : public JPH::ContactListener {
 public:
  explicit Contacts(Players& players) : players_(players) {}

  void OnContactAdded(const JPH::Body& a, const JPH::Body& b, const JPH::ContactManifold& m,
                      JPH::ContactSettings&) override {
    Record(a, b, m);
  }
  void OnContactPersisted(const JPH::Body& a, const JPH::Body& b, const JPH::ContactManifold& m,
                          JPH::ContactSettings&) override {
    Record(a, b, m);
  }

 private:
  void Record(const JPH::Body& a, const JPH::Body& b, const JPH::ContactManifold& m) {
    // mWorldSpaceNormal moves body 2 out of body 1.
    Add(b, m.mWorldSpaceNormal);
    Add(a, -m.mWorldSpaceNormal);
  }
  void Add(const JPH::Body& body, Vec3 normal) {
    if (body.GetObjectLayer() != core::ObjectLayers::kCharacter || body.GetUserData() == 0) return;
    const auto handle = static_cast<PlayerHandle>(body.GetUserData() - 1);
    std::lock_guard lock(mutex_);
    if (handle >= players_.players_.size() || !players_.players_[handle]) return;
    Player& p = *players_.players_[handle];
    if (p.contact_count < Player::kMaxContacts) p.contact_normals[p.contact_count++] = normal;
  }

  Players& players_;
  std::mutex mutex_;
};

Players::Players(core::VoxelWorld& world, core::PhysicsWorld& physics,
                 core::TerrainCollision* terrain)
    : physics_(physics),
      terrain_(terrain),
      query_(world, physics.system()),
      contacts_(std::make_unique<Contacts>(*this)) {
  physics_.system().SetContactListener(contacts_.get());
}

Players::~Players() {
  for (PlayerHandle h = 0; h < players_.size(); ++h) {
    if (players_[h]) Despawn(h);
  }
  physics_.system().SetContactListener(nullptr);
}

PlayerHandle Players::Spawn(const PlayerControllerConfig& config, Vec3 feet, float yaw) {
  auto* p = new Player;
  p->cfg = &config;
  p->standing = MakeCapsule(config, false);
  p->crouched = MakeCapsule(config, true);
  p->c.input.look_yaw = yaw;
  p->pending.look_yaw = yaw;

  JPH::BodyCreationSettings s(p->standing, JPH::RVec3(feet + Vec3(0, config.HalfHeight(false), 0)),
                              JPH::Quat::sIdentity(), JPH::EMotionType::Dynamic,
                              core::ObjectLayers::kCharacter);
  // Rotation locked, no physics gravity, frictionless, never sleeps (PLAYER_CONTROLLER.md §2).
  s.mAllowedDOFs = JPH::EAllowedDOFs::TranslationX | JPH::EAllowedDOFs::TranslationY |
                   JPH::EAllowedDOFs::TranslationZ;
  s.mGravityFactor = 0.0f;
  s.mFriction = 0.0f;
  s.mRestitution = 0.0f;
  s.mLinearDamping = 0.0f;
  s.mAngularDamping = 0.0f;
  s.mAllowSleeping = false;
  s.mMotionQuality = JPH::EMotionQuality::LinearCast;
  s.mEnhancedInternalEdgeRemoval = true;
  p->body = physics_.bodies().CreateAndAddBody(s, JPH::EActivation::Activate);

  PlayerHandle handle = 0;
  while (handle < players_.size() && players_[handle]) ++handle;
  if (handle == players_.size()) {
    players_.push_back(p);
  } else {
    players_[handle] = p;
  }
  physics_.bodies().SetUserData(p->body, handle + 1);
  return handle;
}

void Players::Despawn(PlayerHandle h) {
  Player* p = players_.at(h);
  if (!p) return;
  physics_.bodies().RemoveBody(p->body);
  physics_.bodies().DestroyBody(p->body);
  delete p;
  players_[h] = nullptr;
}

bool Players::Exists(PlayerHandle h) const { return h < players_.size() && players_[h]; }

Players::Player& Players::Get(PlayerHandle h) { return *players_.at(h); }
const Players::Player& Players::Get(PlayerHandle h) const { return *players_.at(h); }

void Players::SetInput(PlayerHandle h, const Input& input) { Get(h).pending = input; }

const PlayerController& Players::controller(PlayerHandle h) const { return Get(h).c; }
PlayerController& Players::mutable_controller(PlayerHandle h) { return Get(h).c; }
const PlayerControllerConfig& Players::config(PlayerHandle h) const { return *Get(h).cfg; }
JPH::BodyID Players::body(PlayerHandle h) const { return Get(h).body; }

Vec3 Players::Position(PlayerHandle h) const {
  return Vec3(physics_.bodies().GetCenterOfMassPosition(Get(h).body));
}
Vec3 Players::Velocity(PlayerHandle h) const {
  return physics_.bodies().GetLinearVelocity(Get(h).body);
}
float Players::HalfHeight(PlayerHandle h) const {
  const Player& p = Get(h);
  return p.cfg->HalfHeight(p.c.crouch.crouching);
}
float Players::Feet(PlayerHandle h) const { return Position(h).GetY() - HalfHeight(h); }
float Players::Head(PlayerHandle h) const { return Position(h).GetY() + HalfHeight(h); }

std::vector<PlayerHandle> Players::handles() const {
  std::vector<PlayerHandle> out;
  for (PlayerHandle h = 0; h < players_.size(); ++h) {
    if (players_[h]) out.push_back(h);
  }
  return out;
}

void Players::SetCrouchShape(Player& p, bool crouching) {
  p.c.crouch.crouching = crouching;
  physics_.bodies().SetShape(p.body, crouching ? p.crouched.GetPtr() : p.standing.GetPtr(),
                             /*updateMassProperties=*/true, JPH::EActivation::Activate);
}

void Players::Restore(PlayerHandle h, const PlayerController& state, Vec3 position, Vec3 velocity) {
  Player& p = Get(h);
  const bool crouching = state.crouch.crouching;
  p.c = state;
  SetCrouchShape(p, crouching);
  Teleport(h, position, velocity);
}

void Players::Teleport(PlayerHandle h, Vec3 position, Vec3 velocity) {
  Player& p = Get(h);
  auto& bodies = physics_.bodies();
  bodies.SetPosition(p.body, JPH::RVec3(position), JPH::EActivation::Activate);
  bodies.SetLinearVelocity(p.body, velocity);
}

void Players::AddVelocity(PlayerHandle h, Vec3 delta_v) {
  auto& bodies = physics_.bodies();
  const JPH::BodyID body = Get(h).body;
  bodies.SetLinearVelocity(body, bodies.GetLinearVelocity(body) + delta_v);
}

void Players::AddExplosion(Vec3 center, float radius, float speed, float upward_bias) {
  for (PlayerHandle h = 0; h < players_.size(); ++h) {
    if (!players_[h]) continue;
    const Vec3 offset = Position(h) - center;
    const float distance = offset.Length();
    if (distance > radius) continue;
    const Vec3 direction = distance > 1e-3f ? offset / distance : Vec3::sAxisY();
    const float falloff = 1.0f - distance / radius;
    AddVelocity(h, (direction + Vec3::sAxisY() * upward_bias) * (speed * falloff));
  }
}

// ---------------------------------------------------------------------------------------------
// Pipeline

void Players::Tick() {
  // Terrain collision around every player (probes read the grid; contacts need the meshes).
  if (terrain_) {
    for (Player* p : players_) {
      if (!p) continue;
      const Vec3 pos(physics_.bodies().GetCenterOfMassPosition(p->body));
      const float reach = 3.0f + physics_.bodies().GetLinearVelocity(p->body).Length() * kDt * 2;
      terrain_->EnsureBox(pos - Vec3::sReplicate(reach), pos + Vec3::sReplicate(reach));
    }
    terrain_->Sync();
  }
  // Pass-major, like the PPC's Quantum system group: each pass runs for every player in turn.
  using Pass = void (Players::*)(Player&);
  static constexpr Pass kPasses[] = {
      &Players::StepInput,     &Players::StepProbe,      &Players::StepPlatform,
      &Players::StepCrouch,    &Players::StepJump,       &Players::StepClimb,
      &Players::StepSwim,      &Players::StepHorizontal, &Players::StepVertical,
      &Players::StepAggregate, &Players::StepState,
  };
  for (Pass pass : kPasses) {
    for (Player* p : players_) {
      if (p) (this->*pass)(*p);
    }
  }
  for (Player* p : players_) {
    if (p) p->contact_count = 0;  // refilled by the coming physics step
  }
}

// 1 — PPCInputSystem
void Players::StepInput(Player& p) {
  PlayerController& c = p.c;
  c.previous_input = c.input;
  c.input = p.pending;
  const float length_sq = c.input.move_x * c.input.move_x + c.input.move_y * c.input.move_y;
  if (length_sq > 1.0f) {
    const float length = std::sqrt(length_sq);
    c.input.move_x /= length;
    c.input.move_y /= length;
  }
  c.events = 0;
  c.landed_speed = 0.0f;
}

bool Players::RingCast(const Player& p, Vec3 center, Vec3 dir, float distance, float radius,
                       ProbeHit* closest) const {
  bool found = false;
  float best = 0.0f;
  const auto& ring = Ring();
  // One broad-phase check for the whole ring; usually no moving body is near and only the voxel
  // grid is walked.
  const Vec3 end = center + dir * distance;
  const Vec3 pad(radius, 0.0f, radius);
  const bool bodies =
      query_.BodiesNear(Vec3::sMin(center, end) - pad, Vec3::sMax(center, end) + pad, p.body);
  for (int i = -1; i < kRingRays; ++i) {
    const Vec3 origin = i < 0 ? center : center + ring[i] * radius;
    ProbeHit hit;
    if (!query_.CastRay(origin, dir, distance, p.body, hit, bodies)) continue;
    if (!closest) return true;  // only "hit or not" was asked
    if (!found || hit.distance < best) {
      best = hit.distance;
      *closest = hit;
      found = true;
    }
  }
  return found;
}

// 2 — PPCProbeSystem
void Players::StepProbe(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  GroundInfo& g = c.ground;
  const Vec3 center(physics_.bodies().GetCenterOfMassPosition(p.body));
  const float half_height = cfg.HalfHeight(c.crouch.crouching);
  const float ring_radius = cfg.body.radius * cfg.advanced.probe_ring_radius;

  g.was_grounded = g.grounded;

  // Ground. While grounded, reach down a whole step so walking down stairs keeps the player on
  // the ground (Source's StayOnGround).
  const float reach = g.grounded
                          ? std::max(cfg.probes.ground_probe_margin, cfg.movement.max_step_height)
                          : cfg.probes.ground_probe_margin;
  ProbeHit hit;
  if (RingCast(p, center, -Vec3::sAxisY(), half_height + reach, ring_radius, &hit)) {
    g.normal = hit.normal;
    g.gap = hit.distance - half_height;
    g.slope_angle = SlopeAngle(hit.normal);
    g.ground = hit.ground;
    const bool walkable = g.slope_angle <= cfg.probes.max_slope_angle;
    // Rising faster than the ground below (a jump or launch in progress) keeps the previous
    // answer until the player stops rising. "The ground below" is last tick's ground velocity,
    // tracked even while airborne, so riding a lift isn't mistaken for a jump.
    const float ground_vy =
        c.platform.ground == hit.ground ? c.platform.ground_velocity.GetY() : 0.0f;
    const bool rising = c.vertical.accumulated_y > ground_vy + kRisingMargin;
    g.grounded = walkable && (!rising || g.was_grounded);
  } else {
    g.grounded = false;
    g.normal = Vec3::sAxisY();
    g.slope_angle = 0.0f;
    g.gap = 0.0f;
    g.ground = {};
  }

  // Ceiling.
  g.ceiling_blocked = RingCast(p, center, Vec3::sAxisY(),
                               half_height + cfg.probes.ceiling_probe_margin, ring_radius, nullptr);

  // Walls (fixed world axes: forward, back, left, right).
  g.touching_wall = false;
  g.wall_normal = Vec3::sZero();
  static const Vec3 kWallDirs[4] = {Vec3(0, 0, 1), Vec3(0, 0, -1), Vec3(-1, 0, 0), Vec3(1, 0, 0)};
  for (const Vec3& dir : kWallDirs) {
    ProbeHit wall;
    if (query_.CastRay(center + dir * ring_radius, dir, cfg.advanced.wall_check_distance, p.body,
                       wall) &&
        SlopeAngle(wall.normal) > kWallMinAngle) {
      g.touching_wall = true;
      g.wall_normal = wall.normal;
      break;
    }
  }
}

// 3 — PPCPlatformSystem. Terrain is static; Tier 1 bodies and players carry by the velocity of
// the point under the player (Jolt moves kinematic bodies by velocity, so one path covers both).
void Players::StepPlatform(Player& p) {
  PlayerController& c = p.c;
  PlatformState& platform = c.platform;
  const GroundRef ground = c.ground.ground;
  if (ground.kind == GroundRef::kNone) {
    platform = {};
    return;
  }
  platform.ground = ground;

  Vec3 velocity = Vec3::sZero();
  float yaw_delta = 0.0f;
  const bool carries = ground.kind == GroundRef::kTier1Body ||
                       (ground.kind == GroundRef::kPlayer && p.cfg->movement.carried_by_characters);
  if (carries) {
    const JPH::RVec3 position = physics_.bodies().GetCenterOfMassPosition(p.body);
    JPH::BodyLockRead lock(physics_.system().GetBodyLockInterface(), JPH::BodyID(ground.id));
    if (lock.Succeeded()) {
      const JPH::Body& body = lock.GetBody();
      velocity = body.GetPointVelocity(position);
      yaw_delta = body.GetAngularVelocity().GetY() * kDt * kRadToDeg;
    }
  }

  platform.ground_velocity = velocity;
  const bool grounded = c.ground.grounded;
  platform.base_velocity = grounded ? velocity : Vec3::sZero();
  const float max_yaw = p.cfg->advanced.max_platform_yaw_speed * kDt;
  platform.yaw_delta = grounded ? std::clamp(yaw_delta, -max_yaw, max_yaw) : 0.0f;
}

bool Players::FitsStanding(const Player& p, Vec3 center) const {
  // Slightly slimmer than the real capsule so touching the floor or a wall doesn't count.
  const float radius = p.cfg->body.radius;
  const Capsule capsule{center, radius - kCrouchSkin,
                        std::max(0.0f, p.cfg->HalfHeight(false) - radius - kCrouchSkin)};
  return !query_.OverlapsSolid(capsule, p.body);
}

// 4 — PPCCrouchSystem
void Players::StepCrouch(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  auto& bodies = physics_.bodies();
  const Vec3 position(bodies.GetCenterOfMassPosition(p.body));
  const float delta = cfg.CrouchHeightDelta();

  auto try_stand = [&] {
    // Grounded: grow upwards from the feet. Airborne: grow down to the feet first, else upwards.
    const Vec3 up = position + Vec3(0, delta, 0);
    const Vec3 down = position - Vec3(0, delta, 0);
    Vec3 stand_at;
    if (c.ground.grounded) {
      if (!FitsStanding(p, up)) return;
      stand_at = up;
    } else if (FitsStanding(p, down)) {
      stand_at = down;
    } else if (FitsStanding(p, up)) {
      stand_at = up;
    } else {
      return;
    }
    bodies.SetPosition(p.body, JPH::RVec3(stand_at), JPH::EActivation::Activate);
    SetCrouchShape(p, false);
    c.events |= Events::kCrouchChanged;
  };

  if (c.Exclusive()) {
    if (c.crouch.crouching) try_stand();
    return;
  }
  if (c.CrouchPressed() && !c.crouch.crouching) {
    // Grounded: keep the feet where they are. Airborne: keep the head, pull the feet up.
    const bool grounded = c.ground.grounded;
    bodies.SetPosition(p.body, JPH::RVec3(position + Vec3(0, grounded ? -delta : delta, 0)),
                       JPH::EActivation::Activate);
    SetCrouchShape(p, true);
    if (!grounded && cfg.crouch.mid_air_boost > 0.0f) {
      c.vertical.accumulated_y += cfg.crouch.mid_air_boost;
    }
    c.events |= Events::kCrouchChanged;
  } else if (!c.input.crouch && c.crouch.crouching) {
    try_stand();
  }
}

void Players::PerformJump(Player& p) {
  PlayerController& c = p.c;
  // Keep any larger upward velocity already absorbed (e.g. from a launch pad).
  c.vertical.accumulated_y =
      std::max(c.vertical.accumulated_y, p.cfg->JumpVelocity() + c.vertical.platform_y);
  c.ground.grounded = false;  // off the ground for every pass from this tick
  c.jump.buffer_ticks = 0;
  c.jump.coyote_ticks = 0;
  c.jump.jumping = true;
  c.jump.jumped_this_tick = true;
  c.events |= Events::kJumped;
}

// Auto-jump (Dwell addition): moving into a one-block obstacle with two free cells above it.
bool Players::AutoJumpObstacle(Player& p) {
  const PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  const Vec3 flat = Flat(MoveDirection(c.input));
  if (flat.Length() < kInputDeadzone) return false;
  const Vec3 center(physics_.bodies().GetCenterOfMassPosition(p.body));
  const float feet = center.GetY() - cfg.HalfHeight(c.crouch.crouching);
  const Vec3 probe = center + SafeNormalized(flat) * (cfg.body.radius + 0.2f);
  const auto x = static_cast<std::int32_t>(std::floor(probe.GetX()));
  const auto z = static_cast<std::int32_t>(std::floor(probe.GetZ()));
  const auto y = static_cast<std::int32_t>(std::floor(feet + cfg.movement.max_step_height + 0.05f));
  const float top =
      static_cast<float>(y) + core::ShapeHeight(core::GetMaterial(query_.Material(x, y, z)).shape);
  if (top <= feet + cfg.movement.max_step_height || top > feet + 1.05f) return false;
  for (int above = 1; above <= 2; ++above) {
    if (core::GetMaterial(query_.Material(x, y + above, z)).shape != core::VoxelShape::kEmpty) {
      return false;
    }
  }
  return true;
}

// 5 — PPCJumpSystem (timers in ticks)
void Players::StepJump(Player& p) {
  PlayerController& c = p.c;
  JumpState& j = c.jump;
  const bool grounded = c.ground.grounded;
  j.jumped_this_tick = false;

  if (j.buffer_ticks > 0) --j.buffer_ticks;
  if (j.coyote_ticks > 0) --j.coyote_ticks;

  if (!grounded && c.ground.was_grounded && !j.jumping) {
    j.coyote_ticks = p.cfg->jump.coyote_ticks;  // walked off an edge
  }
  if (grounded && j.jumping && physics_.bodies().GetLinearVelocity(p.body).GetY() <= 0.0f) {
    j.jumping = false;
  }

  if (c.Exclusive()) return;  // jumping off a ladder / out of water is handled there

  if (c.JumpPressed() && j.buffer_ticks == 0) j.buffer_ticks = p.cfg->jump.buffer_ticks;

  if ((grounded || j.coyote_ticks > 0) && j.buffer_ticks > 0) {
    PerformJump(p);
  } else if (p.cfg->movement.auto_jump && grounded && !c.crouch.crouching && AutoJumpObstacle(p)) {
    PerformJump(p);
  }
}

namespace {

// Ladder frame: facing axis and plate centre of the ladder in `cell`.
struct LadderFrame {
  Vec3 facing;
  Vec3 plate;
};

LadderFrame FrameOf(const Cell& cell, core::MaterialId material) {
  const Vec3 facing = FacingVector(core::GetMaterial(material).facing);
  const Vec3 center(static_cast<float>(cell.x) + 0.5f, static_cast<float>(cell.y) + 0.5f,
                    static_cast<float>(cell.z) + 0.5f);
  return {facing, center - facing * (0.5f - kLadderPlateHalfDepth)};
}

}  // namespace

bool Players::FindLadder(const Player& p, Cell& ladder, bool& in_released_column) const {
  const PlayerController& c = p.c;
  const Vec3 center(physics_.bodies().GetCenterOfMassPosition(p.body));
  const float radius = p.cfg->body.radius;
  const Capsule capsule{center, radius,
                        std::max(0.0f, p.cfg->HalfHeight(c.crouch.crouching) - radius)};
  bool found = false;
  in_released_column = false;
  query_.ForEachOverlappingCell(
      capsule, [&](std::int32_t x, std::int32_t y, std::int32_t z, core::MaterialId m) {
        if (!core::GetMaterial(m).climbable) return;
        // A contiguous ladder column counts as one ladder for `released`.
        if (c.climb.has_released && x == c.climb.released.x && z == c.climb.released.z) {
          in_released_column = true;
          return;
        }
        if (!found) {
          ladder = {x, y, z};
          found = true;
        }
      });
  if (found || !c.climb.climbing) return found;
  // The top of a column reaches kLadderTopReach above its last cell, so the climber can get
  // their feet over the top and onto the ledge (the PPC's ladder triggers overhang their ledge).
  // It only keeps a climb going: grabbing it from the ledge would pull the player back against the
  // ledge, so getting on at the top means stepping off onto the ladder itself.
  const float feet = center.GetY() - p.cfg->HalfHeight(c.crouch.crouching);
  const auto y = static_cast<std::int32_t>(std::floor(feet - kLadderTopReach));
  for (auto z = static_cast<std::int32_t>(std::floor(center.GetZ() - radius));
       z <= static_cast<std::int32_t>(std::floor(center.GetZ() + radius)); ++z) {
    for (auto x = static_cast<std::int32_t>(std::floor(center.GetX() - radius));
         x <= static_cast<std::int32_t>(std::floor(center.GetX() + radius)); ++x) {
      if (!core::GetMaterial(query_.Material(x, y, z)).climbable ||
          core::GetMaterial(query_.Material(x, y + 1, z)).climbable) {
        continue;
      }
      const Vec3 lo(static_cast<float>(x), -1e6f, static_cast<float>(z));
      const Vec3 hi(static_cast<float>(x + 1), 1e6f, static_cast<float>(z + 1));
      if (VoxelQuery::SegmentBoxDistance(capsule, lo, hi) >= radius) continue;
      if (c.climb.has_released && x == c.climb.released.x && z == c.climb.released.z) {
        in_released_column = true;
        continue;
      }
      ladder = {x, y, z};
      return true;
    }
  }
  return false;
}

// 6 — PPCClimbSystem: exclusive layer on climbable voxels.
void Players::StepClimb(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  ClimbState& climb = c.climb;
  Cell ladder;
  bool in_released = false;
  const bool found = FindLadder(p, ladder, in_released);
  if (climb.has_released && !in_released) climb.has_released = false;  // left that column

  if (!climb.climbing) {
    if (!found || c.swim.swimming) return;
    climb.climbing = true;
    climb.ladder = ladder;
    c.horizontal.current = Vec3::sZero();
    c.horizontal.external = Vec3::sZero();
    c.jump.jumping = false;
    c.events |= Events::kClimbStarted;
  }

  const Vec3 position(physics_.bodies().GetCenterOfMassPosition(p.body));
  auto release = [&](bool jump_off) {
    climb.climbing = false;
    climb.has_released = true;
    climb.released = climb.ladder;
    climb.velocity = Vec3::sZero();
    c.vertical.accumulated_y = 0.0f;
    if (jump_off) {
      const LadderFrame frame =
          FrameOf(climb.ladder, query_.Material(climb.ladder.x, climb.ladder.y, climb.ladder.z));
      const float depth = (position - frame.plate).Dot(frame.facing);
      const Vec3 away = depth >= 0.0f ? frame.facing : -frame.facing;
      c.horizontal.current = away * cfg.climb.jump_off_away;
      c.vertical.accumulated_y = cfg.climb.jump_off_up;
      c.ground.grounded = false;
      c.jump.jumped_this_tick = true;
    }
    c.events |= Events::kClimbEnded;
  };

  if (c.JumpPressed()) {
    release(true);
    return;
  }
  if (!found || (c.ground.grounded && !c.ground.was_grounded)) {
    release(false);
    return;
  }
  climb.ladder = ladder;

  const core::MaterialId material = query_.Material(ladder.x, ladder.y, ladder.z);
  const float speed = cfg.climb.speed * core::GetMaterial(material).climb_speed_scale;
  float vertical = c.input.move_y;
  if (c.input.look_pitch < -cfg.climb.look_down_threshold) vertical = -vertical;
  Vec3 velocity = Vec3::sAxisY() * (vertical * speed) +
                  CameraRight(c.input.look_yaw) * (c.input.move_x * speed);
  // Over the top (Dwell): near the column's top, climbing up also moves onto what it leans on.
  std::int32_t top = ladder.y;
  for (int i = 0;
       i < 256 && core::GetMaterial(query_.Material(ladder.x, top + 1, ladder.z)).climbable; ++i) {
    ++top;
  }
  const float feet = position.GetY() - cfg.HalfHeight(c.crouch.crouching);
  const LadderFrame frame = FrameOf(ladder, material);
  const bool over_top = vertical > 0.0f && feet > static_cast<float>(top + 1) - 0.3f;
  if (over_top) velocity -= frame.facing * (vertical * speed);
  // Pull onto the ladder face: a fixed distance in front of the plate along its facing axis.
  if (cfg.climb.snap_strength > 0.0f && !over_top) {
    const float depth = (position - frame.plate).Dot(frame.facing);
    const float side = depth >= 0.0f ? 1.0f : -1.0f;
    const float target = side * (kLadderPlateHalfDepth + cfg.body.radius - kLadderInset);
    velocity += frame.facing * ((target - depth) * cfg.climb.snap_strength);
  }
  climb.velocity = velocity;

  // Hold the other layers: their contributions equal the climb velocity, so letting go isn't
  // mistaken for an external force.
  c.horizontal.current = Vec3::sZero();
  c.horizontal.external = Vec3::sZero();
  c.horizontal.contribution = Flat(velocity);
  c.vertical.accumulated_y = 0.0f;
  c.vertical.target_y = velocity.GetY();
}

// 7 — Swim (Dwell addition, PLAYER_CONTROLLER.md §6.4): exclusive layer in water.
void Players::StepSwim(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  SwimState& swim = c.swim;
  auto& bodies = physics_.bodies();
  const Vec3 position(bodies.GetCenterOfMassPosition(p.body));
  swim.submerged = query_.SubmergedFraction(position, cfg.HalfHeight(c.crouch.crouching));
  if (c.climb.climbing) return;

  const Vec3 body_velocity = bodies.GetLinearVelocity(p.body);
  if (!swim.swimming) {
    if (swim.submerged <= cfg.swim.enter_fraction) return;
    swim.swimming = true;
    c.jump.jumping = false;
    c.events |= Events::kSwimStarted;
  } else if (swim.submerged < cfg.swim.exit_fraction) {
    // Hand back to the normal layers with the current velocity (not an external force).
    swim.swimming = false;
    swim.velocity = Vec3::sZero();
    c.horizontal.current = Flat(body_velocity);
    c.horizontal.external = Vec3::sZero();
    c.horizontal.contribution = Flat(body_velocity);
    c.vertical.accumulated_y = body_velocity.GetY();
    c.vertical.target_y = body_velocity.GetY();
    if (c.input.jump) {  // jump out of the water, e.g. onto the bank
      c.vertical.accumulated_y = std::max(c.vertical.accumulated_y, cfg.JumpVelocity());
      c.ground.grounded = false;
      c.jump.jumped_this_tick = true;
      c.jump.jumping = true;
      c.events |= Events::kJumped;
    }
    c.events |= Events::kSwimEnded;
    return;
  }

  // Move in the look direction (pitch included); jump ascends, crouch descends.
  const float yaw = c.input.look_yaw * kDegToRad;
  const float pitch = c.input.look_pitch * kDegToRad;
  const Vec3 forward(std::sin(yaw) * std::cos(pitch), std::sin(pitch),
                     std::cos(yaw) * std::cos(pitch));
  Vec3 wish =
      (forward * c.input.move_y + CameraRight(c.input.look_yaw) * c.input.move_x) * cfg.swim.speed;
  if (c.input.jump) wish.SetY(cfg.swim.speed);
  if (c.input.crouch) wish.SetY(-cfg.swim.speed);
  Vec3 velocity = body_velocity + (wish - body_velocity) * (1.0f - std::exp(-cfg.swim.drag * kDt));
  velocity +=
      Vec3::sAxisY() * (cfg.swim.buoyancy * (swim.submerged - cfg.swim.float_fraction) * kDt);
  swim.velocity = velocity;

  c.horizontal.current = Vec3::sZero();
  c.horizontal.external = Vec3::sZero();
  c.horizontal.contribution = Flat(velocity);
  c.vertical.accumulated_y = 0.0f;
  c.vertical.target_y = velocity.GetY();
}

// Lifts the player onto a step just ahead (PPC TryStep). The probe starts at centre height, or
// just above step height when crouched (Dwell: a crouched centre is below a slab's top).
void Players::TryStep(Player& p, Vec3 move_direction) {
  const PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  const Vec3 flat = Flat(move_direction);
  if (flat.Length() < kMinDirection) return;
  auto& bodies = physics_.bodies();
  const Vec3 center(bodies.GetCenterOfMassPosition(p.body));
  const float half_height = cfg.HalfHeight(c.crouch.crouching);
  const float feet = center.GetY() - half_height;
  Vec3 origin = center + flat.Normalized() * (cfg.body.radius + cfg.advanced.step_probe_distance);
  origin.SetY(std::max(center.GetY(), feet + cfg.movement.max_step_height + 0.05f));
  ProbeHit hit;
  if (!query_.CastRay(origin, -Vec3::sAxisY(), origin.GetY() - feet + half_height, p.body, hit)) {
    return;
  }
  const float step_height = hit.point.GetY() - feet;
  if (step_height > kMinStep && step_height <= cfg.movement.max_step_height &&
      SlopeAngle(hit.normal) <= cfg.probes.max_slope_angle) {
    // Dwell: also nudge forward so the probe ring (inside the capsule) is over the step; with
    // the slim voxel capsule the ring would otherwise still see the lower floor and snap back.
    const float ring = cfg.body.radius * cfg.advanced.probe_ring_radius;
    const float nudge = cfg.body.radius + cfg.advanced.step_probe_distance - ring + 0.01f;
    bodies.SetPosition(p.body,
                       JPH::RVec3(center + Vec3(0, step_height, 0) + flat.Normalized() * nudge),
                       JPH::EActivation::Activate);
    p.c.vertical.step_grace = kStepGraceTicks;
  }
}

// Edge guard (Dwell addition): while crouched on the ground, don't move (per axis) where the
// ground ring would lose ground within a step.
void Players::ApplyEdgeGuard(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  const Vec3 center(physics_.bodies().GetCenterOfMassPosition(p.body));
  const float half_height = cfg.HalfHeight(true);
  const float ring_radius = cfg.body.radius * cfg.advanced.probe_ring_radius;
  const float reach = half_height + cfg.movement.max_step_height;
  HorizontalLayer& h = c.horizontal;
  for (int axis = 0; axis < 3; axis += 2) {
    const float v = h.contribution[axis];
    if (v == 0.0f) continue;
    Vec3 offset = Vec3::sZero();
    offset.SetComponent(axis, v * kDt);
    if (RingCast(p, center + offset, -Vec3::sAxisY(), reach, ring_radius, nullptr)) continue;
    Vec3 current = h.current, contribution = h.contribution;
    current.SetComponent(axis, 0.0f);
    contribution.SetComponent(axis, 0.0f);
    h.current = current;
    h.contribution = contribution;
  }
}

// 8 — PPCMovementLayerSystem
void Players::StepHorizontal(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  const auto& m = cfg.movement;
  HorizontalLayer& h = c.horizontal;
  const bool grounded = c.ground.grounded;
  if (c.Exclusive()) return;

  // External forces: whatever moved the body away from what it was driven to last tick.
  const Vec3 actual = Flat(physics_.bodies().GetLinearVelocity(p.body));
  Vec3 external_delta = actual - h.contribution;
  // Dwell: velocity a contact removed because the player pushed into it (a wall, a block) is not
  // an external force. Absorbing it (as the PPC does) leaves a phantom push-back that, in the air,
  // never decays, so a player holding forward against a block could not jump onto it. The drive
  // is the whole of last tick's target velocity: snapping down a stair drives into the edge of
  // the step being left, and its sideways push-out must not become momentum either.
  const Vec3 driven = h.contribution + Vec3(0.0f, c.vertical.target_y, 0.0f);
  for (int i = 0; i < p.contact_count; ++i) {
    const Vec3 normal = p.contact_normals[i];
    Vec3 n = Flat(normal);
    const float length = n.Length();
    if (length < 0.1f) continue;  // floors and ceilings: the vertical layer's business
    n /= length;
    const float into = -driven.Dot(normal);  // how fast we drove into the contact
    const float removed = external_delta.Dot(n);
    if (into > 0.0f && removed > 0.0f) external_delta -= n * std::min(removed, into * length);
  }
  if (external_delta.Length() > cfg.advanced.external_absorb_threshold) {
    h.external += external_delta;
  }
  h.external = grounded ? MoveTowards(h.external, Vec3::sZero(), m.ground_external_friction * kDt)
                        : h.external * std::exp(-m.air_external_drag * kDt);

  const Input& input = c.input;
  const Vec3 move_direction = MoveDirection(input);
  if (grounded && move_direction.Length() > kMinDirection) TryStep(p, move_direction);

  const float speed = c.crouch.crouching ? cfg.crouch.speed
                      : input.run        ? m.run_speed
                                         : m.walk_speed;
  const Vec3 player_target = move_direction * speed;

  // Accelerate in the platform's frame so standing on a moving body needs no input.
  const Vec3 base = Flat(c.platform.base_velocity);
  const Vec3 relative = h.current - base;
  const Vec3 relative_delta = player_target - relative;
  const float dot = relative.Length() > kMinDirection && player_target.Length() > kMinDirection
                        ? relative.Normalized().Dot(player_target.Normalized())
                        : 1.0f;
  const float move_length = std::sqrt(input.move_x * input.move_x + input.move_y * input.move_y);
  float rate = move_length > kInputDeadzone
                   ? (dot < kReverseDot ? m.reverse_deceleration : m.acceleration)
                   : m.deceleration;
  if (!grounded) rate *= m.air_control;

  const Vec3 change = MoveTowards(Vec3::sZero(), relative_delta, rate * kDt);
  h.current = relative + change + base;
  h.contribution = h.current + h.external;

  if (m.edge_guard && grounded && c.crouch.crouching) ApplyEdgeGuard(p);
}

// 9 — PPCVerticalLayerSystem
void Players::StepVertical(Player& p) {
  PlayerController& c = p.c;
  const PlayerControllerConfig& cfg = *p.cfg;
  VerticalLayer& v = c.vertical;
  const float gravity = -cfg.body.gravity;
  const float body_y = physics_.bodies().GetLinearVelocity(p.body).GetY();
  if (v.step_grace > 0) --v.step_grace;
  if (c.Exclusive()) return;

  v.last_platform_y = v.platform_y;
  v.platform_y = c.platform.base_velocity.GetY();

  const bool was_grounded = c.ground.was_grounded;
  if (was_grounded && !c.ground.grounded && !c.jump.jumped_this_tick) {
    v.accumulated_y = v.last_platform_y;  // walk-off: keep the platform's vertical motion
  }
  if (c.ground.ceiling_blocked && v.accumulated_y > 0.0f) v.accumulated_y = 0.0f;

  if (c.ground.grounded) {
    // A significant upward deviation while grounded is a launch, unless the ground explains it.
    const float explained = std::max(v.target_y, c.platform.ground_velocity.GetY());
    const float threshold = v.step_grace > 0 ? kStepLaunchThreshold : kLaunchThreshold;
    if (body_y - explained > threshold) {
      v.accumulated_y = body_y + gravity * kDt;
      c.ground.grounded = false;
    } else {
      // Follow the ground along slopes, then close any gap within one tick (ground snap).
      const Vec3 n = c.ground.normal;
      const Vec3 horizontal = c.horizontal.contribution - Flat(c.platform.base_velocity);
      const float along_slope =
          n.GetY() > 0.1f
              ? -(n.GetX() * horizontal.GetX() + n.GetZ() * horizontal.GetZ()) / n.GetY()
              : 0.0f;
      v.accumulated_y = v.platform_y + along_slope - std::max(0.0f, c.ground.gap) / kDt;
    }
  } else {
    // Airborne: absorb external vertical forces, then integrate gravity.
    const float external = body_y - v.target_y;
    if (std::abs(external) > kAbsorbThreshold && !c.jump.jumped_this_tick) {
      v.accumulated_y += external;
    }
    v.accumulated_y += gravity * kDt;
  }

  if (!was_grounded && c.ground.grounded) {
    c.events |= Events::kLanded;
    c.landed_speed = std::max(0.0f, -v.target_y);
  }
  v.target_y = v.accumulated_y;
}

// 10 — PPCAggregateSystem
void Players::StepAggregate(Player& p) {
  PlayerController& c = p.c;
  Vec3 target;
  if (c.climb.climbing) {
    target = c.climb.velocity;
  } else if (c.swim.swimming) {
    target = c.swim.velocity;
  } else {
    target = c.horizontal.contribution + Vec3(0, c.vertical.target_y, 0);
  }
  c.target_velocity = target;
  physics_.bodies().SetLinearVelocity(p.body, target);
}

// 11 — PPCStateSystem
void Players::StepState(Player& p) {
  PlayerController& c = p.c;
  const Vec3 velocity = c.target_velocity;
  if (c.climb.climbing) {
    c.state = State::kClimbing;
  } else if (c.swim.swimming) {
    c.state = State::kSwimming;
  } else if (c.crouch.crouching) {
    c.state = State::kCrouching;
  } else if (!c.ground.grounded) {
    c.state = velocity.GetY() > 0.0f || c.jump.jumping ? State::kJumping : State::kFalling;
  } else if (Flat(velocity).Length() > kMovingSpeed) {
    c.state = c.input.run ? State::kRunning : State::kWalking;
  } else {
    c.state = State::kIdle;
  }
}

}  // namespace dwell::player
