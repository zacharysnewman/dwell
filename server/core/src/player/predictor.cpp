#include "dwell/player/predictor.h"

#include <Jolt/Physics/Body/BodyCreationSettings.h>
#include <Jolt/Physics/Collision/Shape/CapsuleShape.h>

#include <algorithm>
#include <cmath>

#include "dwell/player/net.h"

namespace dwell::player {
namespace {

using JPH::Vec3;
constexpr float kDt = PlayerControllerConfig::Dt();
// Knockback corrections are smoothed up to this multiple of the snap distance: they are expected
// to be large (the client learns of them about one RTT late) and must not pop.
constexpr float kKnockbackSnapFactor = 4.0f;

Vec3 ToVec3(const float (&v)[3]) { return Vec3(v[0], v[1], v[2]); }

}  // namespace

Predictor::Predictor(core::VoxelWorld& world, JPH::JobSystem& jobs,
                     const PlayerControllerConfig& config)
    : world_(world),
      config_(config),
      physics_(std::make_unique<core::PhysicsWorld>(
          jobs, core::PhysicsConfig{.max_bodies = 1024,
                                    .max_body_pairs = 4096,
                                    .max_contact_constraints = 2048,
                                    .temp_allocator_bytes = 4 * 1024 * 1024})),
      terrain_(std::make_unique<core::TerrainCollision>(world_, *physics_)),
      players_(std::make_unique<Players>(world_, *physics_, terrain_.get())) {
  const float r = config_.body.radius;
  remote_standing_ = new JPH::CapsuleShape(std::max(0.0f, config_.HalfHeight(false) - r), r);
  remote_crouched_ = new JPH::CapsuleShape(std::max(0.0f, config_.HalfHeight(true) - r), r);
}

Predictor::~Predictor() {
  auto& bodies = physics_->bodies();
  for (auto& [id, remote] : remotes_) {
    bodies.RemoveBody(remote.body);
    bodies.DestroyBody(remote.body);
  }
  players_.reset();
  terrain_.reset();
}

Vec3 Predictor::Position() const { return handle_ ? players_->Position(*handle_) : Vec3::sZero(); }
Vec3 Predictor::Velocity() const { return handle_ ? players_->Velocity(*handle_) : Vec3::sZero(); }
float Predictor::HalfHeight() const {
  return handle_ ? players_->HalfHeight(*handle_) : config_.HalfHeight(false);
}

const PlayerController& Predictor::controller() const {
  static const PlayerController kIdle{};
  return handle_ ? players_->controller(*handle_) : kIdle;
}

GroundRef Predictor::GroundFromNet(protocol::GroundKind kind, std::uint16_t id) const {
  switch (kind) {
    case protocol::GroundKind::kTerrain:
      return {GroundRef::kTerrain, 0};
    case protocol::GroundKind::kPlayer: {
      const auto it = remotes_.find(id);
      if (it != remotes_.end())
        return {GroundRef::kPlayer, it->second.body.GetIndexAndSequenceNumber()};
      return {};
    }
    default:
      return {};  // Tier 1 bodies arrive in Phase 4
  }
}

void Predictor::Simulate(const protocol::InputFrame& input, bool forward) {
  auto& bodies = physics_->bodies();
  if (forward) {
    constexpr int kMaxTicks = static_cast<int>(kMaxDeadReckoning / kDt);
    for (auto& [id, remote] : remotes_) {
      if (remote.dead_reckoning_ticks++ < kMaxTicks) remote.target += remote.velocity * kDt;
      bodies.MoveKinematic(remote.body, JPH::RVec3(remote.target), JPH::Quat::sIdentity(), kDt);
    }
  }
  players_->SetInput(*handle_, DequantizeInput(input));
  players_->Tick();
  if (const auto it = knockbacks_.find(input.seq); it != knockbacks_.end()) {
    players_->AddVelocity(*handle_, it->second);
  }
  physics_->Step(kDt);
  if (forward) {
    for (auto& [id, remote] : remotes_) {
      bodies.SetLinearVelocity(remote.body, Vec3::sZero());  // replays see them standing still
    }
    offset_ *= std::exp(-kDt / kSmoothingSeconds);
    if (offset_.LengthSq() < 1e-8f) offset_ = Vec3::sZero();
  }
}

void Predictor::Record(const protocol::InputFrame& input) {
  Entry& e = history_[input.seq % kHistory];
  e.seq = input.seq;
  e.input = input;
  e.controller = players_->controller(*handle_);
  e.position = players_->Position(*handle_);
  e.velocity = players_->Velocity(*handle_);
}

void Predictor::Tick(const protocol::InputFrame& input) {
  if (!handle_ || input.seq != next_seq()) return;
  Simulate(input, /*forward=*/true);
  latest_seq_ = input.seq;
  Record(input);
  ++stats_.ticks;
}

void Predictor::Replay(std::uint32_t from_seq) {
  std::uint32_t replayed = 0;
  for (std::uint32_t seq = from_seq; seq <= latest_seq_; ++seq) {
    const Entry& e = history_[seq % kHistory];
    if (e.seq != seq) break;  // history ran out
    const protocol::InputFrame input = e.input;
    Simulate(input, /*forward=*/false);
    Record(input);
    ++replayed;
  }
  stats_.last_replay_ticks = replayed;
}

void Predictor::Reset(const protocol::PhysicsSnapshot& snapshot) {
  const auto& l = snapshot.local;
  const Vec3 center = ToVec3(l.position);
  if (!handle_) {
    handle_ = players_->Spawn(config_, center - Vec3(0, config_.HalfHeight(false), 0));
  }
  PlayerController c;
  FromNet(l.controller, [this](auto kind, auto id) { return GroundFromNet(kind, id); }, c);
  players_->Restore(*handle_, c, center, ToVec3(l.velocity));
  latest_seq_ = std::max(latest_seq_, snapshot.ack_input_seq);
  last_ack_ = snapshot.ack_input_seq;
  for (Entry& e : history_) e.seq = 0;  // (no temporary: the array is ~100 KB)
  protocol::InputFrame none;
  none.seq = snapshot.ack_input_seq;
  if (none.seq == latest_seq_) Record(none);
  knockbacks_.clear();
  offset_ = Vec3::sZero();
  ++stats_.resets;
}

void Predictor::OnSnapshot(const protocol::PhysicsSnapshot& snapshot) {
  const auto& l = snapshot.local;
  if (l.flags & protocol::PlayerFlags::kDead) {
    if (handle_) players_->Despawn(*handle_);
    handle_.reset();
    return;
  }
  if (!handle_) {
    Reset(snapshot);
    return;
  }
  const std::uint32_t ack = snapshot.ack_input_seq;
  if (ack < last_ack_) return;  // an older snapshot, reordered
  last_ack_ = ack;
  // A knockback we hadn't heard of yet (its event still in flight) explains a large correction.
  const bool knockback = l.last_knockback_seq > last_knockback_seq_;
  last_knockback_seq_ = std::max(last_knockback_seq_, l.last_knockback_seq);
  std::erase_if(knockbacks_, [ack](const auto& kv) { return kv.first <= ack; });
  ++stats_.snapshots;

  const Entry& e = history_[ack % kHistory];
  const bool known = e.seq == ack && ack <= latest_seq_;
  const Vec3 server_position = ToVec3(l.position), server_velocity = ToVec3(l.velocity);
  if (known) {
    stats_.last_error = (e.position - server_position).Length();
    const auto predicted = ToNet(e.controller, {});
    const bool match =
        stats_.last_error <= kPositionTolerance &&
        (e.velocity - server_velocity).Length() <= kVelocityTolerance &&
        predicted.flags == l.controller.flags &&
        std::abs(predicted.external_x - l.controller.external_x) <= kVelocityTolerance &&
        std::abs(predicted.external_z - l.controller.external_z) <= kVelocityTolerance;
    if (match) return;  // the common case: no replay
  }

  // Mispredicted (or history lost): restart from the server's state at `ack` and replay.
  ++stats_.replays;
  const Vec3 before = Position();
  PlayerController c = known ? e.controller : players_->controller(*handle_);
  FromNet(l.controller, [this](auto kind, auto id) { return GroundFromNet(kind, id); }, c);
  players_->Restore(*handle_, c, server_position, server_velocity);
  protocol::InputFrame at_ack = known ? e.input : protocol::InputFrame{};
  at_ack.seq = ack;
  Record(at_ack);
  if (ack >= latest_seq_) latest_seq_ = ack;
  Replay(ack + 1);
  const Vec3 correction = before - Position();
  stats_.last_correction = correction.Length();
  const float snap = protocol::kReconcileSnapDistance * (knockback ? kKnockbackSnapFactor : 1.0f);
  if (stats_.last_correction > snap) {
    offset_ = Vec3::sZero();
    ++stats_.snaps;
  } else {
    offset_ += correction;
  }
}

void Predictor::OnKnockback(std::uint32_t input_seq, Vec3 delta_v) {
  if (!handle_ || input_seq <= last_ack_) return;  // already in the authoritative state
  last_knockback_seq_ = std::max(last_knockback_seq_, input_seq);
  knockbacks_[input_seq] = delta_v;
  if (input_seq > latest_seq_) return;  // applied when that input is predicted
  const Entry& previous = history_[(input_seq - 1) % kHistory];
  if (previous.seq != input_seq - 1) return;  // too old to replay; the next snapshot corrects
  const Vec3 before = Position();
  players_->Restore(*handle_, previous.controller, previous.position, previous.velocity);
  Replay(input_seq);
  ++stats_.knockback_replays;
  const Vec3 correction = before - Position();
  if (correction.Length() > protocol::kReconcileSnapDistance * kKnockbackSnapFactor) {
    offset_ = Vec3::sZero();
    ++stats_.snaps;
  } else {
    offset_ += correction;
  }
}

void Predictor::SetRemote(std::uint16_t player_id, Vec3 feet, Vec3 velocity, bool crouched,
                          float lead_seconds) {
  auto& bodies = physics_->bodies();
  const float half = config_.HalfHeight(crouched);
  const float lead = std::clamp(lead_seconds, 0.0f, kMaxDeadReckoning);
  const Vec3 center = feet + Vec3(0, half, 0) + velocity * lead;
  auto it = remotes_.find(player_id);
  if (it == remotes_.end()) {
    JPH::BodyCreationSettings s(crouched ? remote_crouched_ : remote_standing_, JPH::RVec3(center),
                                JPH::Quat::sIdentity(), JPH::EMotionType::Kinematic,
                                core::ObjectLayers::kCharacter);
    Remote remote;
    remote.body = bodies.CreateAndAddBody(s, JPH::EActivation::Activate);
    remote.crouched = crouched;
    it = remotes_.emplace(player_id, remote).first;
  } else if (it->second.crouched != crouched) {
    bodies.SetShape(it->second.body,
                    crouched ? remote_crouched_.GetPtr() : remote_standing_.GetPtr(), false,
                    JPH::EActivation::Activate);
    it->second.crouched = crouched;
  }
  it->second.target = center;
  it->second.velocity = velocity;
  it->second.dead_reckoning_ticks = static_cast<int>(lead / kDt);
}

void Predictor::RemoveRemote(std::uint16_t player_id) {
  const auto it = remotes_.find(player_id);
  if (it == remotes_.end()) return;
  physics_->bodies().RemoveBody(it->second.body);
  physics_->bodies().DestroyBody(it->second.body);
  remotes_.erase(it);
}

}  // namespace dwell::player
