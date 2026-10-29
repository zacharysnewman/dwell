//! `PlayerControllerConfig` (PLAYER_CONTROLLER.md §7): the PPC's recommended feel with voxel
//! dimensions. Durations are stored as ticks at `SIM_HZ`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// ◆ fits 1-wide gaps
    pub radius: f32,
    /// ◆ fits 2-tall doorways
    pub standing_height: f32,
    /// ◆ real mass (kg)
    pub mass: f32,
    /// m/s², absolute (PPC: 2 × 10)
    pub gravity: f32,
    /// ◆ client camera, above the feet
    pub eye_height: f32,
    pub crouch_eye_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Movement {
    pub walk_speed: f32,
    pub run_speed: f32,
    pub acceleration: f32,
    pub deceleration: f32,
    pub reverse_deceleration: f32,
    pub air_control: f32,
    /// ◆ slabs (0.5 m) step up; full blocks need a jump
    pub max_step_height: f32,
    pub air_external_drag: f32,
    pub ground_external_friction: f32,
    pub carried_by_characters: bool,
    /// ◆ Dwell addition (touch preset: true)
    pub auto_jump: bool,
    /// ◆ Dwell addition
    pub edge_guard: bool,
    /// ◆ contact mass scaling (Phase 20)
    pub max_push_force: f32,
    pub pushable_mass_limit: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probes {
    pub ground_probe_margin: f32,
    pub ceiling_probe_margin: f32,
    /// Degrees: the steepest ground; steeper faces are walls. Ground steeper than
    /// `slide.min_angle` slides, so this stays above the steepest face the adaptive shapes make
    /// (a saddle's 54.74°): at 50° a player rested, ungrounded, on a saddle (§6.8).
    pub max_slope_angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jump {
    pub height: f32,
    /// 0.2 s
    pub buffer_ticks: u16,
    /// ◆ 0.2 s (PPC: 0.1 s)
    pub coyote_ticks: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crouch {
    /// ◆ fits 1-tall crawlspaces
    pub height: f32,
    pub speed: f32,
    pub mid_air_boost: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climb {
    pub speed: f32,
    /// Degrees.
    pub look_down_threshold: f32,
    pub jump_off_up: f32,
    pub jump_off_away: f32,
    /// 1/s
    pub snap_strength: f32,
    /// ◆ How far (m) in front of a ladder's plate a body grabs it (owner, 2026-10-29: the pull
    /// sticks out only a little from the ladder); the rest of its cell doesn't.
    pub grab_reach: f32,
}

/// ◆ Dwell addition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swim {
    pub speed: f32,
    pub enter_fraction: f32,
    pub exit_fraction: f32,
    /// Submerged fraction buoyancy settles at.
    pub float_fraction: f32,
    /// m/s² per unit of submersion off `float_fraction`.
    pub buoyancy: f32,
    /// 1/s
    pub drag: f32,
}

/// The flip band (BIFACIAL_WORLD.md §3): within `FLIP_BAND` of the midplane a player moves as when
/// swimming, without buoyancy or drag; moving toward the midplane it is accelerated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub boost_gravities: f32,
}

/// ◆ Creative flight (PLAYER_CONTROLLER.md §6.7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fly {
    /// m/s at the slider's level 0, walking (the same at every height).
    pub speed: f32,
    pub run_factor: f32,
    /// 1/s: how quickly velocity follows the wish.
    pub drag: f32,
    /// |x|, |y|, |z| (m): flight is not bounded by the world (ADR 0025 D3), only held inside the
    /// wire format's position range (`POS64_LIMIT`).
    pub position_limit: f32,
}

/// ◆ Sliding on steep ground (PLAYER_CONTROLLER.md §6.8): ground steeper than `min_angle` (and
/// still ground: up to `probes.max_slope_angle`) cannot be stood on or walked up; the player
/// slides. Speeds are along the slope.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    /// Degrees: ground steeper than this slides. Tested as `angle > min_angle`, so a 45° face
    /// slides whatever the rounding of its normal.
    pub min_angle: f32,
    /// Coefficient of friction: the slope pulls with `gravity × (sin θ − friction × cos θ)`.
    pub friction: f32,
    /// m/s along the slope.
    pub terminal_speed: f32,
    /// Fraction of `movement.acceleration` (and `deceleration`) for steering across the slope.
    pub steering: f32,
    /// m/s: the fastest steering across the slope.
    pub steer_speed: f32,
    /// Holding up the slope lowers the terminal speed by up to this fraction (never to 0).
    pub brake: f32,
    /// m/s² along the slope: how fast braking (or a lower terminal speed) slows the slide.
    pub brake_deceleration: f32,
    /// m/s² along the slope: momentum carried up a steep face dies at this rate (with the slope's
    /// pull), so walking or running at one never climbs it.
    pub uphill_deceleration: f32,
    /// Crouched: the terminal speed × this, and `crouch_friction` instead of `friction`.
    pub crouch_terminal_factor: f32,
    pub crouch_friction: f32,
}

/// ◆ The sprint slide (PLAYER_CONTROLLER.md §6.9): pressing crouch while sprinting on the ground
/// slides on in the direction of travel, crouched, slowing with friction (less downhill, more
/// uphill). Speeds are horizontal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SprintSlide {
    /// m/s: the slowest horizontal speed a crouch press starts a slide from (above the walk speed,
    /// so only a sprint slides).
    pub min_start_speed: f32,
    /// m/s added to the speed when the slide starts.
    pub start_boost: f32,
    /// m/s: the fastest a slide goes; downhill speeds it up to this.
    pub max_speed: f32,
    /// m/s²: friction's deceleration.
    pub friction: f32,
    /// Fraction of gravity along the ground that speeds the slide downhill and slows it uphill:
    /// `gravity × slope_gravity × (the ground's horizontal tilt along the slide)`.
    pub slope_gravity: f32,
    /// °/s: the fastest move input turns the slide.
    pub steer_rate: f32,
    /// Ticks after a slide ends before another can start.
    pub cooldown_ticks: u16,
}

/// ◆ Which contacts an impact can hurt on (§6.10, `damage.impact_surfaces`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactSurfaces {
    /// Ground only (owner, 2026-10-21): a contact whose face-local slope is walkable
    /// (`probes.max_slope_angle`), landings and ramps; walls, steep faces and ceilings never hurt.
    Ground,
    /// Every contact: floors, ramps, walls and ceilings.
    All,
}

/// ◆ Dwell addition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damage {
    /// m/s into a surface (`max(0, −v · n)`, the controller's `impact_speed`) a hit must exceed to
    /// hurt, on the surfaces `impact_surfaces` names (§6.10).
    pub impact_damage_min_speed: f32,
    /// Health points per m/s above the minimum.
    pub impact_damage_per_speed: f32,
    /// The contacts an impact can hurt on: the ground (default) or every surface.
    pub impact_surfaces: ImpactSurfaces,
    pub crush_speed: f32,
    pub crush_ticks: u16,
}

/// ◆ The camera over steps (PLAYER_CONTROLLER.md §9, Dwell): the body steps in one tick, and the
/// first-person eye (and a drawn body) follows it along a virtual ramp instead. Client-only: the
/// sim never reads it, so it changes nothing on the wire or in prediction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepView {
    /// The virtual ramp's slope (rise per m walked) over a lone step: the eye climbs or descends a
    /// step's height over `height ÷ slope` of travel (0.5: a ½ m step over 1 m, like a ½ m ramp).
    pub slope: f32,
    /// The steepest virtual ramp: on a stair the slope is the step's height over the distance
    /// walked since the step before (the stair's own slope), from `slope` up to this.
    pub max_slope: f32,
    /// The eye's least speed (m/s) toward the feet, so a step taken slowly, or standing still
    /// after one, still settles.
    pub min_speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Advanced {
    /// ◆ How far past a step's face its top is read (m).
    pub step_probe_distance: f32,
    /// ◆ Heights above the feet (m) at which the step-up looks for a step's face: one low for short
    /// steps, one high enough to see over the ground rising up to a step on a ramp.
    pub step_face_heights: [f32; 2],
    /// ◆ How far inside the probe ring a step's edge ends up after a step-up (m), so the ground ring
    /// stands on the step.
    pub step_footing: f32,
    /// ◆ The most the ground may rise from the feet to a step's foot (m), as on a ramp up to a step:
    /// the step-up then lifts by the step plus that rise.
    pub step_max_rise: f32,
    /// Fraction of the capsule radius.
    pub probe_ring_radius: f32,
    pub wall_check_distance: f32,
    pub external_absorb_threshold: f32,
    /// deg/s
    pub max_platform_yaw_speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerControllerConfig {
    pub body: Body,
    pub movement: Movement,
    pub probes: Probes,
    pub jump: Jump,
    pub crouch: Crouch,
    pub climb: Climb,
    pub swim: Swim,
    pub band: Band,
    pub fly: Fly,
    pub slide: Slide,
    pub sprint_slide: SprintSlide,
    pub damage: Damage,
    pub step_view: StepView,
    pub advanced: Advanced,
}

impl Default for PlayerControllerConfig {
    /// The default desktop preset.
    fn default() -> Self {
        Self {
            body: Body {
                radius: 0.3,
                standing_height: 1.8,
                mass: 80.0,
                gravity: 20.0,
                eye_height: 1.62,
                crouch_eye_height: 0.8,
            },
            movement: Movement {
                walk_speed: 5.0,
                run_speed: 8.0,
                acceleration: 50.0,
                deceleration: 12.0,
                reverse_deceleration: 60.0,
                air_control: 0.2,
                max_step_height: 0.55,
                air_external_drag: 0.0,
                ground_external_friction: 15.0,
                carried_by_characters: false,
                auto_jump: false,
                edge_guard: false,
                max_push_force: 800.0,
                pushable_mass_limit: 400.0,
            },
            probes: Probes {
                ground_probe_margin: 0.15,
                ceiling_probe_margin: 0.10,
                max_slope_angle: 56.0,
            },
            jump: Jump {
                height: 1.25,
                buffer_ticks: 12,
                coyote_ticks: 12,
            },
            crouch: Crouch {
                height: 0.9,
                speed: 1.6,
                mid_air_boost: 0.0,
            },
            climb: Climb {
                speed: 3.0,
                look_down_threshold: 30.0,
                jump_off_up: 4.0,
                jump_off_away: 3.0,
                snap_strength: 10.0,
                grab_reach: 0.25,
            },
            swim: Swim {
                speed: 3.0,
                enter_fraction: 0.6,
                exit_fraction: 0.4,
                float_fraction: 0.7,
                buoyancy: 12.0,
                drag: 2.0,
            },
            band: Band {
                boost_gravities: 2.0,
            },
            fly: Fly {
                speed: 8.0,
                run_factor: 2.5,
                drag: 8.0,
                position_limit: (dwell_protocol::world::POS64_LIMIT - 1_024) as f32,
            },
            slide: Slide {
                min_angle: 44.0,
                friction: 0.3,
                terminal_speed: 14.0,
                steering: 0.4,
                steer_speed: 5.0,
                brake: 0.6,
                brake_deceleration: 10.0,
                uphill_deceleration: 60.0,
                crouch_terminal_factor: 1.15,
                crouch_friction: 0.2,
            },
            sprint_slide: SprintSlide {
                min_start_speed: 6.5,
                start_boost: 1.5,
                max_speed: 14.0,
                friction: 6.0,
                slope_gravity: 1.0,
                steer_rate: 90.0,
                cooldown_ticks: 30,
            },
            damage: Damage {
                impact_damage_min_speed: 12.0,
                impact_damage_per_speed: 8.0,
                impact_surfaces: ImpactSurfaces::Ground,
                crush_speed: 4.0,
                crush_ticks: 6,
            },
            step_view: StepView {
                slope: 0.5,
                max_slope: 1.0,
                min_speed: 1.0,
            },
            advanced: Advanced {
                step_probe_distance: 0.05,
                step_face_heights: [0.05, 0.25],
                step_footing: 0.01,
                step_max_rise: 0.25,
                probe_ring_radius: 0.9,
                wall_check_distance: 0.16,
                external_absorb_threshold: 0.01,
                max_platform_yaw_speed: 360.0,
            },
        }
    }
}

impl Damage {
    /// Health points an impact at `speed` m/s into a surface takes (0 at or under the minimum).
    pub fn impact_damage(&self, speed: f32) -> i32 {
        if speed > self.impact_damage_min_speed {
            ((speed - self.impact_damage_min_speed) * self.impact_damage_per_speed).round() as i32
        } else {
            0
        }
    }
}

impl PlayerControllerConfig {
    /// Touch preset: auto-jump on (PLAYER_CONTROLLER.md §6.2).
    pub fn touch() -> Self {
        let mut c = Self::default();
        c.movement.auto_jump = true;
        c
    }

    /// The Unity package's original numbers (PPC ApplyUnityParity), with Dwell's body; kept for
    /// comparison testing.
    pub fn unity_parity() -> Self {
        let mut c = Self::default();
        c.body.gravity = 10.0;
        c.movement.walk_speed = 5.0;
        c.movement.run_speed = 10.0;
        c.movement.acceleration = 10.0;
        c.movement.deceleration = 10.0;
        c.movement.reverse_deceleration = 20.0;
        c.movement.air_control = 1.0;
        c.movement.max_step_height = 0.5;
        c.movement.air_external_drag = 0.5;
        c.movement.ground_external_friction = 15.0;
        c.jump.height = 5.0;
        c.crouch.speed = 2.0;
        c
    }

    pub fn half_height(&self, crouching: bool) -> f32 {
        (if crouching {
            self.crouch.height
        } else {
            self.body.standing_height
        }) * 0.5
    }
    /// How far the capsule centre moves when crouching or standing up.
    pub fn crouch_height_delta(&self) -> f32 {
        self.half_height(false) - self.half_height(true)
    }
    /// Take-off speed that reaches `jump.height` under this config's gravity.
    pub fn jump_velocity(&self) -> f32 {
        (2.0 * self.body.gravity * self.jump.height).sqrt()
    }
    /// The most one step-up lifts the player: a step, plus the ground's rise to its foot. The
    /// camera smooths feet changes up to this (PLAYER_CONTROLLER.md §9).
    pub fn max_step_lift(&self) -> f32 {
        self.movement.max_step_height + self.advanced.step_max_rise
    }
    pub const fn dt() -> f32 {
        1.0 / dwell_protocol::SIM_HZ as f32
    }
}
