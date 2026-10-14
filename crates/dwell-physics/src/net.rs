//! Controller ↔ wire conversions (PLAYER_CONTROLLER.md §8.4). The client quantizes input once and
//! the server and the client's prediction dequantize the same integers here, so both simulate
//! bit-identical inputs.

use dwell_protocol::{
    ControllerState, GroundKind, InputFrame, controller_flags as cf, input_buttons as ib,
    player_flags as pf, players,
};

use crate::controller::{Cell, Input, PlayerController};
use crate::physics::Vec3;
use crate::query::GroundRef;

/// Rounds half up, like JavaScript's Math.round (the client's quantizer).
fn round_half_up(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

pub fn quantize_yaw(degrees: f32) -> i16 {
    (round_half_up(degrees / 360.0 * 65536.0) & 0xFFFF) as u16 as i16
}
pub fn quantize_pitch(degrees: f32) -> i16 {
    round_half_up(degrees.clamp(-90.0, 90.0) / 90.0 * 32767.0) as i16
}
pub fn dequantize_yaw(q: i16) -> f32 {
    f32::from(q) * (360.0 / 65536.0)
}
pub fn dequantize_pitch(q: i16) -> f32 {
    f32::from(q) * (90.0 / 32767.0)
}

const MAX_LEVEL: i32 = players::FLY_SPEED_MAX_LEVEL as i32;
const SHIFT: u32 = players::FLY_SPEED_SHIFT as u32;

pub fn quantize_input(input: &Input, seq: u32) -> InputFrame {
    // Clamp to the unit circle first: the server rejects longer move vectors (§11).
    let (mut mx, mut my) = (input.move_x, input.move_y);
    let length = (mx * mx + my * my).sqrt();
    if length > 1.0 {
        mx /= length;
        my /= length;
    }
    let buttons = (if input.jump { ib::JUMP } else { 0 })
        | (if input.run { ib::RUN } else { 0 })
        | (if input.crouch { ib::CROUCH } else { 0 })
        | (if input.fly { ib::FLY } else { 0 })
        | (((i32::from(input.fly_speed).min(MAX_LEVEL) as u16) << SHIFT) & ib::FLY_SPEED);
    InputFrame {
        seq,
        move_x: round_half_up(mx.clamp(-1.0, 1.0) * 127.0) as i8,
        move_y: round_half_up(my.clamp(-1.0, 1.0) * 127.0) as i8,
        buttons,
        yaw: quantize_yaw(input.look_yaw),
        pitch: quantize_pitch(input.look_pitch),
    }
}

pub fn dequantize_input(f: &InputFrame) -> Input {
    Input {
        move_x: f32::from(f.move_x) / 127.0,
        move_y: f32::from(f.move_y) / 127.0,
        jump: f.buttons & ib::JUMP != 0,
        run: f.buttons & ib::RUN != 0,
        crouch: f.buttons & ib::CROUCH != 0,
        fly: f.buttons & ib::FLY != 0,
        fly_speed: i32::from((f.buttons & ib::FLY_SPEED) >> SHIFT).min(MAX_LEVEL) as u8,
        look_yaw: dequantize_yaw(f.yaw),
        look_pitch: dequantize_pitch(f.pitch),
    }
}

fn ground_kind(g: GroundRef) -> GroundKind {
    match g {
        GroundRef::None | GroundRef::Tier1Body(_) => GroundKind::None, // Tier 1 bodies arrive in Phase 20
        GroundRef::Terrain => GroundKind::Terrain,
        GroundRef::Player(_) => GroundKind::Player,
    }
}

/// The resumable controller state on the wire; `ground_id` names the ground for the peer (a player
/// id for `GroundRef::Player`).
pub fn to_net(c: &PlayerController, ground_id: impl Fn(GroundRef) -> u16) -> ControllerState {
    let flag = |b: bool, f: u8| if b { f } else { 0 };
    ControllerState {
        flags: flag(c.ground.grounded, cf::GROUNDED)
            | flag(c.jump.jumping, cf::JUMPING)
            | flag(c.crouching, cf::CROUCHING)
            | flag(c.climb.climbing, cf::CLIMBING)
            | flag(c.climb.has_released, cf::HAS_RELEASED)
            | flag(c.swim.swimming, cf::SWIMMING)
            | flag(c.fly.flying, cf::FLYING)
            | flag(c.face < 0, cf::FACE_B),
        current_x: c.horizontal.current.x,
        current_z: c.horizontal.current.z,
        external_x: c.horizontal.external.x,
        external_z: c.horizontal.external.z,
        contribution_x: c.horizontal.contribution.x,
        contribution_z: c.horizontal.contribution.z,
        accumulated_y: c.vertical.accumulated_y,
        platform_y: c.vertical.platform_y,
        target_y: c.vertical.target_y,
        ground_velocity_y: c.platform.ground_velocity.y,
        ground_kind: ground_kind(c.platform.ground),
        ground_id: ground_id(c.platform.ground),
        buffer_ticks: c.jump.buffer_ticks.min(255) as u8,
        coyote_ticks: c.jump.coyote_ticks.min(255) as u8,
        step_grace: c.vertical.step_grace,
        ladder: [c.climb.ladder.x, c.climb.ladder.y, c.climb.ladder.z],
        released: [c.climb.released.x, c.climb.released.z],
    }
}

/// Overwrites the resumable parts of `c` (everything but input, outputs and per-tick scratch).
pub fn from_net(
    s: &ControllerState,
    ground: impl Fn(GroundKind, u16) -> GroundRef,
    c: &mut PlayerController,
) {
    c.ground.grounded = s.flags & cf::GROUNDED != 0;
    c.jump.jumping = s.flags & cf::JUMPING != 0;
    c.crouching = s.flags & cf::CROUCHING != 0;
    c.climb.climbing = s.flags & cf::CLIMBING != 0;
    c.climb.has_released = s.flags & cf::HAS_RELEASED != 0;
    c.swim.swimming = s.flags & cf::SWIMMING != 0;
    c.fly.flying = s.flags & cf::FLYING != 0;
    c.face = if s.flags & cf::FACE_B != 0 { -1 } else { 1 };
    c.horizontal.current = Vec3::new(s.current_x, 0.0, s.current_z);
    c.horizontal.external = Vec3::new(s.external_x, 0.0, s.external_z);
    c.horizontal.contribution = Vec3::new(s.contribution_x, 0.0, s.contribution_z);
    c.vertical.accumulated_y = s.accumulated_y;
    c.vertical.platform_y = s.platform_y;
    c.vertical.target_y = s.target_y;
    c.vertical.step_grace = s.step_grace;
    c.platform.ground = ground(s.ground_kind, s.ground_id);
    c.platform.ground_velocity = Vec3::new(0.0, s.ground_velocity_y, 0.0);
    c.jump.buffer_ticks = u16::from(s.buffer_ticks);
    c.jump.coyote_ticks = u16::from(s.coyote_ticks);
    c.climb.ladder = Cell {
        x: s.ladder[0],
        y: s.ladder[1],
        z: s.ladder[2],
    };
    c.climb.released = Cell {
        x: s.released[0],
        y: 0,
        z: s.released[1],
    };
}

pub fn player_flags_of(c: &PlayerController, dead: bool) -> u8 {
    let flag = |b: bool, f: u8| if b { f } else { 0 };
    flag(c.ground.grounded, pf::GROUNDED)
        | flag(c.crouching, pf::CROUCHED)
        | flag(c.climb.climbing, pf::CLIMBING)
        | flag(c.swim.swimming, pf::SWIMMING)
        | flag(c.fly.flying, pf::FLYING)
        | flag(c.face < 0, pf::FACE_B)
        | flag(dead, pf::DEAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_round_trips_through_the_wire() {
        let i = Input {
            move_x: 0.6,
            move_y: -0.8,
            look_yaw: 90.0,
            look_pitch: -30.0,
            run: true,
            fly: true,
            fly_speed: 17,
            ..Input::default()
        };
        let back = dequantize_input(&quantize_input(&i, 7));
        assert!((back.move_x - 0.6).abs() < 0.01 && (back.move_y + 0.8).abs() < 0.01);
        assert!((back.look_yaw - 90.0).abs() < 0.01 && (back.look_pitch + 30.0).abs() < 0.01);
        assert!(back.run && back.fly && !back.jump);
        assert_eq!(back.fly_speed, 17);
        // Longer move vectors are clamped; levels above the maximum count as it.
        let f = quantize_input(
            &Input {
                move_x: 3.0,
                move_y: 4.0,
                fly_speed: 200,
                ..Input::default()
            },
            1,
        );
        assert_eq!((f.move_x, f.move_y), (76, 102));
        assert_eq!(dequantize_input(&f).fly_speed, MAX_LEVEL as u8);
        let wire = InputFrame {
            buttons: ib::FLY_SPEED,
            ..InputFrame::default()
        };
        assert_eq!(dequantize_input(&wire).fly_speed, MAX_LEVEL as u8);
        assert_eq!(quantize_yaw(-90.0), quantize_yaw(270.0));
    }

    #[test]
    fn controller_state_round_trips() {
        let mut c = PlayerController::default();
        c.ground.grounded = true;
        c.crouching = true;
        c.face = -1;
        c.horizontal.external = Vec3::new(1.5, 0.0, -2.0);
        c.vertical.accumulated_y = 3.25;
        c.jump.coyote_ticks = 400;
        c.climb.has_released = true;
        c.climb.released = Cell { x: 4, y: 9, z: -3 };
        c.platform.ground = GroundRef::Terrain;
        let s = to_net(&c, |_| 0);
        let mut back = PlayerController::default();
        from_net(
            &s,
            |k, _| {
                if k == GroundKind::Terrain {
                    GroundRef::Terrain
                } else {
                    GroundRef::None
                }
            },
            &mut back,
        );
        assert!(back.ground.grounded && back.crouching && back.climb.has_released);
        assert_eq!(back.face, -1);
        assert_eq!(back.horizontal.external, c.horizontal.external);
        assert_eq!(back.vertical.accumulated_y, 3.25);
        assert_eq!(back.jump.coyote_ticks, 255);
        assert_eq!((back.climb.released.x, back.climb.released.z), (4, -3));
        assert_eq!(back.platform.ground, GroundRef::Terrain);
    }
}
