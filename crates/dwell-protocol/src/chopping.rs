//! Tools and chopping (docs/adr/0067-stone-axe-and-chopping.md), as data
//! (`shared/tools/tools.json`, generated into `tools.gen.rs` by `shared/tools/gen.mjs`).
//!
//! A wood cell (a log) has a **grain** (its `axis`) and four **sides**, the faces across the grain.
//! Each side has an open **notch** (wood cut out, metres deep) and, in each of its two **bands**
//! (halves along the grain), a **kerf**: a cut past the notch whose walls bind the axe. A swing's
//! **share** is how well its plane crosses the grain; it cuts `bite × share` into open wood, less
//! the deeper its kerf already is. When both bands of a side are cut past the notch, the chunk
//! between comes out: the notch deepens to the shallower kerf. So chopping in, then a little above
//! or below, cuts a trunk in about 9 swings, and hacking one spot takes about four times as many.
//! The server keeps a [`Cut`] per wood cell it has been swung at ([`Cuts`]); clients draw it.

use crate::bytes::{ByteReader, ByteWriter};

#[rustfmt::skip]
mod table {
    include!("tools.gen.rs");
}

/// The fixture tools (shared/tools/fixtures/tools.json; tests only): a weapon added by data alone.
#[rustfmt::skip]
#[allow(dead_code, unused_imports)]
pub mod fixture {
    include!("tools_fixture.gen.rs");
}

pub use crate::damage::{BlockDamageDef, CarveProfile, Cracks, DamageLook, DamageLookKind};
pub use table::{
    BLOCK_DAMAGE, BREAK_CLASSES, BREAK_TOOLS, BREAK_WITH, CHOP2_DEFAULT, CREATIVE_WEAR, DURABILITY,
    HINTS, TOOLS, WEAR_BREAK_SOUND, WOODS,
};

/// The on-screen hint for a tool of `kind` held in a stance (`vertical` or level), in `table`
/// (`hints` in shared/tools/tools.json: one line per kind and stance).
pub fn hint_in(
    table: &[(ToolKind, &'static str, &'static str)],
    kind: ToolKind,
    vertical: bool,
) -> Option<&'static str> {
    table
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|&(_, level, vert)| if vertical { vert } else { level })
}

/// [`hint_in`] over the build's data.
pub fn hint(kind: ToolKind, vertical: bool) -> Option<&'static str> {
    hint_in(HINTS, kind, vertical)
}

/// The hits tool item `item` survives (`durability.items`; owner, 2026-11-11), in `table`; `None`:
/// it never wears.
pub fn durability_in(table: &[(&str, u16)], item: &str) -> Option<u16> {
    table.iter().find(|(i, _)| *i == item).map(|&(_, n)| n)
}

/// [`durability_in`] over the data's table.
pub fn durability(item: &str) -> Option<u16> {
    durability_in(DURABILITY, item)
}

/// A tool's swing (`swing` in the data): its feel (the client's) and its path (both sides').
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingDef {
    /// Before the arc (the wind-up).
    pub windup_s: f64,
    /// The arc's sweep: the head's path, sampled at `samples` steps.
    pub active_s: f64,
    /// The pause on impact (the hit-stop).
    pub hit_stop_s: f64,
    /// After a hit, and after a miss (a looser follow-through).
    pub recover_hit_s: f64,
    pub recover_miss_s: f64,
    /// The head's distance from the eye (m) and the arc it sweeps (degrees), centred on the look.
    pub reach_m: f64,
    pub arc_deg: f64,
    pub samples: u32,
    /// The view's sway with the swing (`camera` in the data, per swing kind; docs/CHARACTERS.md
    /// §5.2): the level swing's and the vertical one's.
    pub camera_level: SwingCamera,
    pub camera_vertical: SwingCamera,
    /// What a swing will draw from stamina when stamina exists (unused until then).
    pub stamina_cost: f64,
    /// Sound events (shared/sounds): the arc's swish, a bounce off what is not a wood.
    pub swish: Option<&'static str>,
    pub bounce: Option<&'static str>,
    /// The sound event of a swing that cleared blocks on its way (its `clears`), and the leaves
    /// each cleared cell scatters (the client's particles).
    pub rustle: Option<&'static str>,
    pub rustle_leaves: u32,
}

/// One key of a swing's camera sway (`camera.<kind>.keys`): at phase time `t` (0 the swing's
/// start, 1 the wind-up's end, 2 the arc's end, 3 a miss's recovery's end), the view's offset:
/// `yaw` degrees toward the screen's right, `pitch` up, `roll` the view leaning right (its top
/// toward the screen's right), `bob` metres along the view's right, up and back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraKey {
    pub t: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub bob: [f64; 3],
}

/// A view offset (the same axes as [`CameraKey`]): added to the view drawn, never to the aim.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraOffset {
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub bob: [f64; 3],
}

impl CameraOffset {
    pub const ZERO: CameraOffset = CameraOffset {
        yaw: 0.0,
        pitch: 0.0,
        roll: 0.0,
        bob: [0.0; 3],
    };
    pub fn scaled(self, k: f64) -> CameraOffset {
        CameraOffset {
            yaw: self.yaw * k,
            pitch: self.pitch * k,
            roll: self.roll * k,
            bob: self.bob.map(|b| b * k),
        }
    }
    pub fn plus(self, o: CameraOffset) -> CameraOffset {
        CameraOffset {
            yaw: self.yaw + o.yaw,
            pitch: self.pitch + o.pitch,
            roll: self.roll + o.roll,
            bob: std::array::from_fn(|k| self.bob[k] + o.bob[k]),
        }
    }
    /// The largest angle (degrees) or bob (as cm) in it: for "is it at rest" checks.
    pub fn size(&self) -> f64 {
        let bob = self.bob.iter().fold(0.0f64, |m, b| m.max(b.abs()));
        self.yaw
            .abs()
            .max(self.pitch.abs())
            .max(self.roll.abs())
            .max(bob * 100.0)
    }
}

/// The kick at contact (`camera.<kind>.kick`): an offset added at the moment the head meets
/// something (or passes the arc's middle on a miss), dying away over `decay_s`; a miss kicks at
/// `miss` of it, a hit into a wood at 1, a bounce off anything else at `bounce`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraKick {
    pub offset: CameraOffset,
    pub decay_s: f64,
    pub miss: f64,
    pub bounce: f64,
}

/// A swing kind's camera sway: keyed offsets over its phase time and the kick at contact. No
/// keys: no sway.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwingCamera {
    pub keys: &'static [CameraKey],
    pub kick: CameraKick,
}

impl SwingCamera {
    /// The keyed offset at phase time `t` (0–3), eased between keys (each key a still point);
    /// zero outside the keys.
    pub fn at(&self, t: f64) -> CameraOffset {
        let k = self.keys;
        let (Some(first), Some(last)) = (k.first(), k.last()) else {
            return CameraOffset::ZERO;
        };
        if !(first.t..=last.t).contains(&t) {
            return CameraOffset::ZERO;
        }
        let i = k.iter().rposition(|key| key.t <= t).unwrap_or(0);
        let a = k[i];
        let b = k.get(i + 1).copied().unwrap_or(a);
        let span = b.t - a.t;
        let u = if span > 1e-9 { (t - a.t) / span } else { 0.0 };
        let e = u * u * (3.0 - 2.0 * u);
        let mix = |x: f64, y: f64| x + (y - x) * e;
        CameraOffset {
            yaw: mix(a.yaw, b.yaw),
            pitch: mix(a.pitch, b.pitch),
            roll: mix(a.roll, b.roll),
            bob: std::array::from_fn(|j| mix(a.bob[j], b.bob[j])),
        }
    }
}

impl SwingDef {
    /// A time `s` s into a miss as phase time (0 the start, 1 the wind-up's end, 2 the arc's end,
    /// 3 the recovery's end): the camera's keys are placed in it, so they follow the timings.
    pub fn phase_time(&self, s: f64) -> f64 {
        let w = self.windup_s.max(1e-6);
        let a = self.active_s.max(1e-6);
        let r = self.recover_miss_s.max(1e-6);
        if s < self.windup_s {
            (s / w).max(0.0)
        } else if s < self.windup_s + self.active_s {
            1.0 + (s - self.windup_s) / a
        } else {
            (2.0 + (s - self.windup_s - self.active_s) / r).min(3.0)
        }
    }
    /// The camera sway of a level or a vertical swing.
    pub fn camera(&self, vertical: bool) -> &SwingCamera {
        if vertical {
            &self.camera_vertical
        } else {
            &self.camera_level
        }
    }
    /// When along the swing (s from its start) the head reaches arc step `step`.
    pub fn time_of_step(&self, step: u32) -> f64 {
        let n = self.samples.max(2) - 1;
        self.windup_s + self.active_s * f64::from(step.min(n)) / f64::from(n)
    }
    /// When a centred hit lands (s): the arc's middle step, where a miss's camera kick lands.
    pub fn centre_s(&self) -> f64 {
        self.time_of_step(self.samples.max(2).saturating_sub(1) / 2)
    }
    /// How long a swing that hit at arc step `step` lasts, and one that missed.
    pub fn hit_length(&self, step: u32) -> f64 {
        self.time_of_step(step) + self.hit_stop_s + self.recover_hit_s
    }
    pub fn miss_length(&self) -> f64 {
        self.windup_s + self.active_s + self.recover_miss_s
    }
}

/// Leaves a hit shakes down from a tree (`leaves` in the data; the client's particles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaves {
    /// The leaf block they fall from (none: no leaves).
    pub block: Option<&'static str>,
    pub count: u32,
    /// sRGB.
    pub color: [u8; 3],
    /// Where leaf blocks are looked for: within this of the hit across, this above it.
    pub radius_m: f64,
    pub height_m: f64,
}

/// A standing tree's sway as it is cut (`sway` in the data; drawn by the client).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sway {
    /// It begins once this much of the cross-section is cut,
    pub from_fraction: f64,
    /// and leans up to this (degrees) at the fell fraction.
    pub max_deg: f64,
}

impl WoodDef {
    /// The sway's lean (degrees) of a standing trunk cut `cross` of the way: none before
    /// `from_fraction`, rising to `max_deg` at the fell fraction (eased in: more toward the fall).
    pub fn sway_deg(&self, cross: f64) -> f64 {
        let span = (self.fell_fraction - self.sway.from_fraction).max(1e-6);
        let u = ((cross - self.sway.from_fraction) / span).clamp(0.0, 1.0);
        self.sway.max_deg * u * u
    }
    /// A hit's sound event at a cut `cross` of the way through: the last stage begun.
    pub fn sound_at(&self, cross: f64) -> Option<&'static str> {
        self.sounds
            .iter()
            .rev()
            .find(|(from, _)| cross >= *from)
            .map(|(_, e)| *e)
    }
}

/// The tool kinds the engine understands (`kind` in the data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    /// Chops the woods.
    Axe,
    /// Strikes creatures (WORK_QUEUE item 35: a sword): its swing's arc hurts the animals it
    /// crosses at contact by its [`WeaponDef`], never a player, and cuts no wood.
    Weapon,
}

/// A weapon's strike (`weapon` in shared/tools/tools.json; WORK_QUEUE item 35): what one hit does
/// to a creature its arc crosses. Its wear is `durability` (merged into [`DURABILITY`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponDef {
    /// Health taken from a creature per hit (the creature's `health`, shared/creatures).
    pub damage: f64,
    /// How far a hit pushes the creature away from the striker (metres, along the ground).
    pub knockback_m: f64,
    /// The push a connecting hit gives a player (WORK_QUEUE item 38: an NPC's strike), m/s away
    /// from the striker along the ground, and `lift_mps` up.
    pub knockback_mps: f64,
    pub lift_mps: f64,
    /// The most creatures one swing hits (in the order the arc reaches them).
    pub max_targets: u32,
    /// Its swordplay (owner, 2026-11-12: the perpendicular-vs-parallel defence).
    pub guard: GuardDef,
}

/// A sword's swordplay (`weapon.guard`; WORK_QUEUE item 35, owner 2026-11-12: "you can only hold
/// the sword vertical or horizontal, and you can choose to block or not. Attacking drops block, if
/// you hit enemy sword within an angle threshold of perpendicular you're blocked, if within an
/// angle of parallel it connects"). Each stance's blade is a line at a roll in the holder's view
/// (0 level, 90 upright); a strike's blade is its swing kind's stance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuardDef {
    /// The blade's roll held level and upright (degrees in the holder's view, 0 level).
    pub level_roll_deg: f64,
    pub vertical_roll_deg: f64,
    /// A strike within this of perpendicular to the raised blade is blocked (degrees).
    pub block_within_deg: f64,
    /// A strike within this of parallel to it slips past and connects (degrees).
    pub connect_within_deg: f64,
    /// Between the two: a glance, this share of the damage.
    pub glance_damage: f64,
    /// A blocked strike's recoil: the striker's swing stops and it may swing again after this.
    pub recoil_s: f64,
    /// The raised blade guards strikes from within this arc round the holder's facing (degrees).
    pub front_deg: f64,
    /// How long Place (right-click, the controller's and the touch Place) is held before the block
    /// rises (s); let go sooner it is a tap, turning the stance (the swing's plane) as before.
    pub raise_after_s: f64,
    /// The first-person pose clip each stance's raised block shows (None: the default hold).
    pub level_pose: Option<&'static str>,
    pub vertical_pose: Option<&'static str>,
    /// Each stance's raised blade as others see it (where a clash's blades meet, BUGS.md #86).
    pub level_blade: RaisedBlade,
    pub vertical_blade: RaisedBlade,
    /// What clients show of a clash (the `weapons.clash` capability).
    pub clash: ClashLook,
}

/// A stance's raised blade as others see it (BUGS.md #86): its grip and tip, metres from the
/// holder's feet in its facing frame (right, up, ahead; face-local), fitted to the third-person
/// raised pose (`guard_level`, `guard_vertical`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaisedBlade {
    pub grip: [f64; 3],
    pub tip: [f64; 3],
}

/// A clash's sound and sparks on the clients (round 2 of WORK_QUEUE item 35): the sound event, how
/// far the clash is sent and heard (m), its sparks (drawn as chips), and a glance's share of the
/// sparks and the volume (a connect shows none of them: the creature's own reaction shows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClashLook {
    pub sound: Option<&'static str>,
    pub hear_m: f64,
    pub sparks: Option<Chips>,
    pub glance_share: f64,
}

impl ClashLook {
    /// The share of the look (sound volume, sparks) a clash of `k` shows: all for a block, the
    /// glance share for a glance, none for a connect.
    pub fn share(&self, k: Clash) -> f64 {
        match k {
            Clash::Blocked => 1.0,
            Clash::Glanced => self.glance_share,
            Clash::Connected => 0.0,
        }
    }
}

/// What a strike that meets a raised blade does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    /// Near perpendicular: blocked (no damage; a clash, the striker recoils).
    Blocked,
    /// Between the thresholds: a glance (`glance_damage` of the damage).
    Glanced,
    /// Near parallel: it slips past and connects (full damage).
    Connected,
}

impl GuardDef {
    /// A stance's raised blade (its grip and tip from the holder's feet, right / up / ahead).
    pub fn blade(&self, vertical: bool) -> RaisedBlade {
        if vertical {
            self.vertical_blade
        } else {
            self.level_blade
        }
    }

    /// A stance's raised-block pose clip (first person), if its data names one.
    pub fn raised_pose(&self, vertical: bool) -> Option<&'static str> {
        if vertical {
            self.vertical_pose
        } else {
            self.level_pose
        }
    }

    /// The stance that blocks a strike of swing kind `strike_vertical` best (its blade nearest
    /// perpendicular to the strike's): an NPC's `counter` stance.
    pub fn counter(&self, strike_vertical: bool, strike: &GuardDef) -> bool {
        let r = strike.roll(strike_vertical);
        let a = |v: bool| Self::blade_angle(r, self.roll(v));
        a(true) >= a(false)
    }

    /// A stance's blade roll.
    pub fn roll(&self, vertical: bool) -> f64 {
        if vertical {
            self.vertical_roll_deg
        } else {
            self.level_roll_deg
        }
    }

    /// The angle (0..=90 degrees) between a strike's blade at roll `strike` in the striker's view
    /// and a raised blade at roll `guard` in its holder's view, the two facing each other (the
    /// holder's roll is mirrored in the striker's view).
    pub fn blade_angle(strike: f64, guard: f64) -> f64 {
        let a = (strike + guard).rem_euclid(180.0);
        a.min(180.0 - a)
    }

    /// A strike at roll `strike` on a raised blade at roll `guard`, by this weapon's thresholds.
    pub fn clash_at(&self, strike: f64, guard: f64) -> Clash {
        let a = Self::blade_angle(strike, guard);
        if a >= 90.0 - self.block_within_deg - 1e-9 {
            Clash::Blocked
        } else if a <= self.connect_within_deg + 1e-9 {
            Clash::Connected
        } else {
            Clash::Glanced
        }
    }

    /// A strike of swing kind `strike_vertical` on a blade raised in stance `guard_vertical` (each
    /// stance's roll from its own weapon: `guard` the defender's).
    pub fn clash(&self, strike_vertical: bool, guard: &GuardDef, guard_vertical: bool) -> Clash {
        self.clash_at(self.roll(strike_vertical), guard.roll(guard_vertical))
    }

    /// The share of a strike's damage that lands after `clash`.
    pub fn damage_share(&self, clash: Clash) -> f64 {
        match clash {
            Clash::Blocked => 0.0,
            Clash::Glanced => self.glance_damage,
            Clash::Connected => 1.0,
        }
    }
}

/// A tool (an item, by name) and how it cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolDef {
    pub name: &'static str,
    pub kind: ToolKind,
    /// Holding the use button keeps swinging it (`repeatOnHold`): a new full swing as soon as the
    /// last is over (`Chopping::ready`), until let go.
    pub repeat_on_hold: bool,
    /// The item a hand shows holding it (`shared/characters/characters.json`'s `items`).
    pub held: Option<&'static str>,
    /// The least time between two swings.
    pub swing_seconds: f64,
    /// Metres a swing straight across the grain cuts into open wood.
    pub bite_m: f64,
    /// A kerf this deep past the notch halves a swing into it.
    pub bind_m: f64,
    /// The share of a bite with the swing's plane across the grain, and along it.
    pub across: f64,
    pub along: f64,
    /// The share's falloff from across to along: `along + (across − along) |cos θ|^sharpness`.
    pub sharpness: f64,
    /// The item a hand shows holding it for a vertical swing (the second hold).
    pub held_vertical: Option<&'static str>,
    /// The swing: its timing, its arc, its feel.
    pub swing: SwingDef,
    /// The blocks (by name) its swing goes through, breaking each it touches (owner, 2026-11-09:
    /// the axe clears leaves); never a wood.
    pub clears: &'static [&'static str],
    /// The chopping prototype (`chop2.bite`): the share of the cross-section a chop across the
    /// grain removes ([`Cut::chop2`]).
    pub chop2_bite: f64,
    /// A weapon's strike (`kind` weapon); None for an axe.
    pub weapon: Option<WeaponDef>,
}

impl ToolDef {
    /// Whether it is a weapon (strikes creatures, chops nothing).
    pub fn is_weapon(&self) -> bool {
        self.kind == ToolKind::Weapon
    }
    /// Whether its swing clears the block of this name on its way.
    pub fn clears_block(&self, block: &str) -> bool {
        self.clears.contains(&block)
    }
    /// The held item for the swing's plane.
    pub fn held_for(&self, vertical: bool) -> Option<&'static str> {
        if vertical {
            self.held_vertical.or(self.held)
        } else {
            self.held
        }
    }
}

/// A block an axe cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WoodDef {
    pub block: &'static str,
    /// A standing one falls when this much of its cross-section is cut.
    pub fell_fraction: f64,
    /// A lying one is cut through at this much.
    pub buck_fraction: f64,
    /// What a cut-through lying length drops.
    pub buck_drops: &'static [crate::inventory::DropRule],
    /// The chips a swing knocks out of it (the client's particles).
    pub chips: Chips,
    /// The leaves a hit shakes down, the standing tree's sway, each hit's sound by the cut.
    pub leaves: Leaves,
    pub sway: Sway,
    pub sounds: &'static [(f64, &'static str)],
    /// How its cuts are carved into its cell's mesh (the client's, `dwell-mesh`).
    pub cut: CutLook,
    /// The stored-ramp block a felled cell splits into (Phase 55 T2): the stump keeps its ramp
    /// rising away from the notch, the trunk's bottom its upside-down ramp. None: the cell goes.
    pub fell_wedge: Option<&'static str>,
    /// The trunk's upside-down ramp lies parallel to the stump's (`fellWedgeTrunk: matching`, the
    /// default) rather than as the notch's upper face (`open`, the log's since 2026-11-12: the gash;
    /// `dwell_sim::felling::WedgeTrunk`).
    pub fell_wedge_matching: bool,
    /// The felled trunk's tip toward the notch (`fellTip`, owner 2026-11-09: "make the tree tip if
    /// needed, like we used to, maybe 3/4 of the force"): a share of the scripted topple spin
    /// (`felling.toppleSpin`, capped by `toppleTipSpeed`) it is released with; 0: none, the
    /// wedges' contact alone.
    pub fell_tip: f64,
    /// The chopping prototype's rules (`chop2`).
    pub chop2: Chop2Wood,
}

/// A wood's rules in the chopping prototype (`chop2` in the data; WORK_QUEUE item 7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chop2Wood {
    /// It falls once more than this share of its cross-section is cut, from one side or two.
    pub fell_fraction: f64,
    /// A chop along the grain cuts this share of one across it.
    pub along: f64,
    /// The felled trunk's wedge lies parallel to the stump's (`matching`), or as the notch's
    /// upper face (`open`: the split keeps the notch, `| / \ |`).
    pub wedge_matching: bool,
}

/// How a wood's cuts are carved into its cell's mesh (`cut` in the data): each hit a head-sized V
/// bite where it struck (`bite`), the bites joining into the notch, cracks round them (`cracks`);
/// a cut with no bites recorded (an older server or save) as the notch across the side and each
/// band's kerf as a slit. The faces inside are fresh wood.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutLook {
    /// The atlas tile of the cut faces (none: the block's end-grain tile).
    pub tile: Option<&'static str>,
    /// A colour multiplying the tile (sRGB).
    pub tint: [u8; 3],
    /// The notch's opening along the grain per metre of its depth.
    pub open: f64,
    /// The kerf slits' width along the grain (m).
    pub kerf_width_m: f64,
    /// The bites each hit carves (WORK_QUEUE item 26).
    pub bite: BiteLook,
    /// The crack overlay round the bites (a `shared/textures/cracks.json` set; none: no cracks),
    /// its stages by the share of the fell (or buck) fraction cut.
    pub cracks: Option<Cracks>,
}

/// A wood's bites (`cut.bite` in the data; WORK_QUEUE item 26): each hit carves a V the axe head's
/// size where it struck, `width_m` across the side (its mouth), its two ends sloping over `ends_m`;
/// a hit on a spot already bitten (in the same band) lands beside it, its centre `step` × the width
/// from the other's, so the bites widen and join into the notch.
///
/// The notch they join into follows the bite rule (WORK_QUEUE item 18; owner, 2026-11-12: "Yes the
/// notch should grow to follow the 50% rule"): after k of the swings to fell it has carved
/// k × ½ × the log's volume ÷ those swings, one V along the grain `x` m deep, its mouth
/// `max(band_m, x ÷ depth_per_mouth)` along the grain (the cell's length at most) and each bite
/// widened to `x × width_per_depth` across the side (the head's width at least).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiteLook {
    pub width_m: f64,
    pub ends_m: f64,
    pub step: f64,
    /// The notch's least mouth along the grain (m): the band it starts in.
    pub band_m: f64,
    /// The V's depth over its mouth once it outgrows the band (2: a mouth the cell's ½ m at a
    /// depth of 1 m, the fell's gash).
    pub depth_per_mouth: f64,
    /// Each bite's width across the side per metre of the V's depth.
    pub width_per_depth: f64,
}

/// The defaults of a wood's `cut.bite`.
pub const BITE_LOOK: BiteLook = BiteLook {
    width_m: 0.14,
    ends_m: 0.03,
    step: 0.6,
    band_m: 0.25,
    depth_per_mouth: 2.0,
    width_per_depth: 1.0,
};

/// The chips a swing knocks out of a wood (`chips` in the data).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chips {
    pub count: u32,
    /// Real 3D fragments a hit flings (visual only, short-lived; data `shards`, 0–8), together the
    /// hit's bite (BLOCK_DAMAGE.md §10).
    pub shards: u32,
    /// The item whose look they are drawn with (data `piece`; default `dwell:rock_shard`).
    pub piece: &'static str,
    /// sRGB.
    pub color: [u8; 3],
    /// Their speed out of the cut (m/s).
    pub speed: f64,
    /// Their size (m).
    pub size: f64,
}

/// A tool item's class and tier for breaking (`breaking.tools` in shared/tools/tools.json).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakTool {
    pub item: &'static str,
    /// The tool class (`pickaxe`, `shovel`, `axe`, …: `breaking.classes`' keys).
    pub class: &'static str,
    pub tier: &'static str,
    /// The tier's break speed, a multiple of the bare hand's.
    pub speed: f64,
    /// When its swing's head meets the block (s after the click; `contactS`, 0: at once): a hit
    /// with it lands then (WORK_QUEUE item 25).
    pub contact_s: f64,
    /// Held, the break repeats this often (s; `repeatOnHold` and `repeatS`); none: one per press.
    pub repeat_s: Option<f64>,
}

/// When a break made holding `held` lands (s after the click), in `tools`: its `contact_s`, else at
/// once (a hand, a block, an item that is not a breaking tool).
pub fn break_contact_s_in(tools: &[BreakTool], held: Option<&str>) -> f64 {
    held.and_then(|h| tools.iter().find(|t| t.item == h))
        .map_or(0.0, |t| t.contact_s)
}

/// [`break_contact_s_in`] over the game's data.
pub fn break_contact_s(held: Option<&str>) -> f64 {
    break_contact_s_in(BREAK_TOOLS, held)
}

/// The break speed of `block` holding `held` (an item name; none: an empty hand), from tables
/// `classes` (block → tool class) and `tools`: the held tool's tier speed when its class is the
/// block's, 1 (as before) otherwise.
pub fn break_speed_in(
    classes: &[(&str, &str)],
    tools: &[BreakTool],
    block: &str,
    held: Option<&str>,
) -> f64 {
    let Some(class) = classes.iter().find(|(b, _)| *b == block).map(|(_, c)| *c) else {
        return 1.0;
    };
    held.and_then(|h| tools.iter().find(|t| t.item == h))
        .filter(|t| t.class == class)
        .map_or(1.0, |t| t.speed)
}

/// [`break_speed_in`] over the build's data.
pub fn break_speed(block: &str, held: Option<&str>) -> f64 {
    break_speed_in(BREAK_CLASSES, BREAK_TOOLS, block, held)
}

/// The tool kind breaking a block needs where blocks are not unlimited (its `breakWith`), if any.
pub fn break_with(block: &str) -> Option<ToolKind> {
    BREAK_WITH
        .iter()
        .find(|(b, _)| *b == block)
        .map(|(_, k)| *k)
}

/// Whether a break of `block` holding `held` (an item name, none: an empty hand) is allowed where
/// blocks are not unlimited: a block with a `breakWith` needs a tool of that kind.
pub fn may_break_holding(block: &str, held: Option<&str>) -> bool {
    match break_with(block) {
        None => true,
        Some(kind) => held.and_then(tool).is_some_and(|t| t.kind == kind),
    }
}

/// The tool of an item's name.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The wood of a block's name.
pub fn wood(block: &str) -> Option<&'static WoodDef> {
    WOODS.iter().find(|w| w.block == block)
}

/// The wood an axe chops a block as: a wood's own block, or its `fellWedge` (a stump's ramp,
/// chopped as its wood and never cleared: WORK_QUEUE item 26, the stump keeps its gash).
pub fn chopped_wood(block: &str) -> Option<&'static WoodDef> {
    wood(block).or_else(|| WOODS.iter().find(|w| w.fell_wedge == Some(block)))
}

/// The level swings `tool` takes to fell a standing (grain y) cell of `wood` at one aim, as the
/// server lands them (each into the side's open band, [`Cut::open_band`], at the tool's `across`
/// share): the hits to break of the bite rule for chopping (BLOCK_DAMAGE.md §10).
pub fn swings_to_fell(tool: &ToolDef, wood: &WoodDef) -> u32 {
    swings_to_cut(
        tool.bite_m * tool.across,
        tool.bind_m,
        1,
        wood.fell_fraction,
    )
}

/// The swings at one aim, each cutting `bite` m into open wood (`bind` its kerf's), that cut a
/// cell of grain `grain` past `fraction` of its cross-section ([`swings_to_fell`]'s count).
pub fn swings_to_cut(bite: f64, bind: f64, grain: usize, fraction: f64) -> u32 {
    if bite <= 1e-9 {
        return 255;
    }
    let mut c = Cut::default();
    for i in 1..=255 {
        let band = c.open_band(0, 0);
        c.swing_by(bite, bind, grain, 0, band, 1.0);
        if c.cross(grain) >= fraction - 1e-6 {
            return i;
        }
    }
    255
}

/// The bite (m³) one chop of `tool` takes out of `wood` (WORK_QUEUE item 18: "50% of the block's
/// volume / number of swings required to break it"): [`crate::damage::bite_m3`] of a cell over
/// [`swings_to_fell`]. The chips a chop throws total it.
pub fn chop_bite_m3(tool: &ToolDef, wood: &WoodDef) -> f64 {
    crate::damage::bite_m3(crate::damage::CELL_M3, swings_to_fell(tool, wood))
}

/// A damaged wood cell's hit marks as the axe's bites (WORK_QUEUE item 51: the axe hits a log
/// through block damage, and the octagonal log's carve is the bites'): each mark on a side (an
/// edit face, `axis × 2 + negative`, its point in 1/255ths of the cell along each axis, in the
/// frame the cell is drawn in) recorded where it struck ([`Cut::bite_in`], its band along the grain
/// and its place across the side), each side weighted by its marks. Drawn at the damage fraction
/// ([`crate::damage::DamageView::fraction`]) as the progress, so k hits of n carve k bites of the
/// bite rule. Marks on the end grain are not drawn. The cut's notch only weighs the sides: it is
/// never sent or saved.
pub fn cut_of_marks(
    grain: usize,
    marks: &[crate::damage::HitMark],
    look: &BiteLook,
    span: [f64; 2],
) -> Cut {
    let mut cut = Cut::default();
    for m in marks {
        let axis = usize::from(m.face / 2);
        if axis > 2 {
            continue;
        }
        let Some(side) = side_of(grain, axis, m.face % 2 == 0) else {
            continue;
        };
        let at = |a: usize| f64::from(m.point[a]) / 255.0;
        let band = usize::from(at(grain) >= 0.5);
        cut.bite_in(
            grain,
            side,
            band,
            at(side_axis(grain, side ^ 2)),
            look,
            span,
        );
        cut.notch[side] += 0.01;
    }
    cut
}

/// Sides of a wood cell: the faces across its grain.
pub const SIDES: usize = 4;
/// Bands of a side: its halves along the grain.
pub const BANDS: usize = 2;

/// A cell's size along a world axis (0 x, 1 y, 2 z): ½ m rows, 1 m across.
pub fn extent(axis: usize) -> f64 {
    if axis == 1 { 0.5 } else { 1.0 }
}

/// The two axes across `grain`, in order.
fn across_axes(grain: usize) -> [usize; 2] {
    match grain {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// The side of a cell with grain `grain` that a face with outward normal along `axis` (`positive`)
/// is; none for an end-grain face (along the grain).
pub fn side_of(grain: usize, axis: usize, positive: bool) -> Option<usize> {
    let j = across_axes(grain).iter().position(|&a| a == axis)?;
    Some(2 * j + usize::from(positive))
}

/// A side's outward normal (world).
pub fn side_normal(grain: usize, side: usize) -> [i32; 3] {
    let mut n = [0; 3];
    n[across_axes(grain)[side / 2]] = if side % 2 == 1 { 1 } else { -1 };
    n
}

/// The world axis a side's depth is measured along.
pub fn side_axis(grain: usize, side: usize) -> usize {
    across_axes(grain)[side / 2]
}

/// How well a swing whose plane has normal `normal` cuts wood whose grain is `grain` (unit vectors,
/// any frame both share): `across` when the plane is square to the grain, `along` in line with it.
pub fn share(tool: &ToolDef, normal: [f64; 3], grain: [f64; 3]) -> f64 {
    let c = (normal[0] * grain[0] + normal[1] * grain[1] + normal[2] * grain[2]).abs();
    tool.along + (tool.across - tool.along) * c.min(1.0).powf(tool.sharpness)
}

/// A swing plane's normal, face-local (+y up): a horizontal swing's is up; a vertical swing's is
/// level, square to the look's heading (the plane holds the look and up). A look straight down has
/// no heading: its vertical plane is taken along x.
pub fn swing_normal(vertical: bool, look: [f64; 3]) -> [f64; 3] {
    if !vertical {
        return [0.0, 1.0, 0.0];
    }
    // look × up = (−look.z, 0, look.x), level.
    let (x, z) = (-look[2], look[0]);
    let l = (x * x + z * z).sqrt();
    if l < 1e-6 {
        [0.0, 0.0, 1.0]
    } else {
        [x / l, 0.0, z / l]
    }
}

/// The most bites a side keeps (later hits deepen the ones there).
pub const MAX_BITES: usize = 12;

/// Where one hit struck a side (WORK_QUEUE item 26): `u` its centre across the side (0..1 of the
/// cell along the side's other axis, face-locally up for y, as the bands), `band` the band it cut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bite {
    pub u: f32,
    pub band: u8,
}

/// A side's bites, oldest first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bites {
    pub n: u8,
    pub at: [Bite; MAX_BITES],
}

impl Bites {
    pub fn all(&self) -> &[Bite] {
        &self.at[..usize::from(self.n).min(MAX_BITES)]
    }

    fn push(&mut self, b: Bite) {
        if usize::from(self.n) < MAX_BITES {
            self.at[usize::from(self.n)] = b;
            self.n += 1;
        }
    }

    fn write(&self, w: &mut ByteWriter) {
        w.u8(self.n.min(MAX_BITES as u8));
        for b in self.all() {
            w.f32(b.u);
            w.u8(b.band);
        }
    }

    fn read(r: &mut ByteReader) -> Bites {
        let n = r.u8();
        r.check(usize::from(n) <= MAX_BITES);
        let mut out = Bites::default();
        for _ in 0..n.min(MAX_BITES as u8) {
            let u = r.f32();
            let band = r.u8();
            r.check((0.0..=1.0).contains(&u) && usize::from(band) < BANDS);
            out.push(Bite {
                u,
                band: band.min(1),
            });
        }
        out
    }
}

/// The cut in one wood cell (metres).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    /// Open wood cut out of each side.
    pub notch: [f32; SIDES],
    /// Each side's band's cut past its notch.
    pub kerf: [[f32; BANDS]; SIDES],
    /// Where each side was struck (WORK_QUEUE item 26; the look only: the notch and kerfs are the
    /// cut). Carried apart from the notch and kerfs ([`Cut::write_bites`]): none from an older
    /// server or save, drawn as before.
    pub bites: [Bites; SIDES],
}

impl Cut {
    /// How deep a side is cut (its notch and its deepest kerf), metres.
    pub fn depth(&self, side: usize) -> f64 {
        f64::from(self.notch[side] + self.kerf[side][0].max(self.kerf[side][1]))
    }

    /// The share of the cross-section cut (0..1): the most of any two opposite sides together,
    /// over the cell's size across them.
    pub fn cross(&self, grain: usize) -> f64 {
        (0..2)
            .map(|j| {
                let e = extent(across_axes(grain)[j]);
                ((self.depth(2 * j) + self.depth(2 * j + 1)) / e).min(1.0)
            })
            .fold(0.0, f64::max)
    }

    /// The side cut deepest (its share of the size across it); none when uncut.
    pub fn deepest(&self, grain: usize) -> Option<usize> {
        (0..SIDES)
            .filter(|&s| self.depth(s) > 0.0)
            .max_by(|&a, &b| {
                let da = self.depth(a) / extent(side_axis(grain, a));
                let db = self.depth(b) / extent(side_axis(grain, b));
                da.total_cmp(&db)
            })
    }

    pub fn is_empty(&self) -> bool {
        *self == Cut::default()
    }

    /// The band of `side` a swing aimed at band `aimed` lands in: the aimed one, unless its kerf
    /// is deeper than the other's, when the head comes in beside it, in the open wood (BUGS.md
    /// #75: a player swings at one spot, and a quarter-metre band is too fine to aim at, so the
    /// same aim swing after swing hacked one kerf and the tree never fell).
    pub fn open_band(&self, side: usize, aimed: usize) -> usize {
        let other = 1 - aimed.min(1);
        if self.kerf[side][aimed.min(1)] > self.kerf[side][other] {
            other
        } else {
            aimed.min(1)
        }
    }

    /// One swing of `tool` with `share` (`share`) into band `band` of side `side`: the kerf there
    /// deepens by `bite × share / (1 + kerf / bind)`, at most to the far side; when both bands are
    /// cut past the notch the chunk between comes out. Returns the metres cut.
    pub fn swing(&mut self, tool: &ToolDef, grain: usize, side: usize, band: usize, s: f64) -> f64 {
        self.swing_by(tool.bite_m, tool.bind_m, grain, side, band, s)
    }

    /// [`Cut::swing`] with a tool's `bite` and `bind` (m).
    pub fn swing_by(
        &mut self,
        bite: f64,
        bind: f64,
        grain: usize,
        side: usize,
        band: usize,
        s: f64,
    ) -> f64 {
        let e = extent(side_axis(grain, side));
        let k = f64::from(self.kerf[side][band]);
        let room = (e - f64::from(self.notch[side]) - k).max(0.0);
        let gain = (bite * s / (1.0 + k / bind.max(1e-6))).min(room);
        self.kerf[side][band] = (k + gain) as f32;
        let m = self.kerf[side][0].min(self.kerf[side][1]);
        if m > 0.0 {
            self.notch[side] += m;
            self.kerf[side][0] -= m;
            self.kerf[side][1] -= m;
        }
        gain
    }

    /// The wood side `side` has lost (m): its notch and its kerfs' mean (each swing at one aim
    /// adds half its bite: one band of two).
    pub fn removed_m(&self, side: usize) -> f64 {
        f64::from(self.notch[side]) + 0.5 * f64::from(self.kerf[side][0] + self.kerf[side][1])
    }

    /// The share of the swings to cut this cell through (0..1) its cut stands for (WORK_QUEUE item
    /// 18, the notch's bite rule): its hits (the bites recorded) over the swings at one aim it takes
    /// to fell (grain y: `wood`'s `fell_fraction`) or buck (`buck_fraction`) with the bite its hits
    /// cut on average, so a tool of any bite is read from the cut alone (the stone axe's 11, the
    /// metal's 5). A cut without bites: its cross-section over the fraction.
    pub fn progress(&self, wood: &WoodDef, grain: usize) -> f64 {
        let fraction = if grain == 1 {
            wood.fell_fraction
        } else {
            wood.buck_fraction
        };
        let hits: u32 = self.bites.iter().map(|b| u32::from(b.n)).sum();
        if hits == 0 {
            return (self.cross(grain) / fraction.max(1e-6)).clamp(0.0, 1.0);
        }
        let removed: f64 = (0..SIDES).map(|s| self.removed_m(s)).sum();
        let bite = 2.0 * removed / f64::from(hits);
        let n = swings_to_cut(bite, 1.0, grain, fraction);
        (f64::from(hits) / f64::from(n)).min(1.0)
    }

    /// Whether any side records where it was struck (else it is drawn as before bites).
    pub fn has_bites(&self) -> bool {
        self.bites.iter().any(|b| b.n > 0)
    }

    /// [`Cut::bite_in`] on a log as wide as its cell (`span` 0..1).
    pub fn bite(&mut self, grain: usize, side: usize, band: usize, u: f64, look: &BiteLook) -> f64 {
        self.bite_in(grain, side, band, u, look, [0.0, 1.0])
    }

    /// Records a hit on `side` in band `band` aimed at `u` across the side (0..1, [`Bite::u`]):
    /// a bite `look.width_m` wide centred there, or, where a bite already lies closer than
    /// `look.step` × the width (in either band: round 2, the bites spread across the side, not
    /// down it), beside the nearest one in the open wood, nearest the aim (the head comes in
    /// beside the last bite, as [`Cut::open_band`] does along the grain), so hacking one spot
    /// widens the notch across the log, one head's step a hit. Kept inside `span` (the log's
    /// extent across the side, 0..1 of the cell: a thinner trunk's octagon is narrower than its
    /// cell); a side with no room left (or [`MAX_BITES`] bites) keeps its bites, which the cut
    /// deepens. Returns the centre used.
    pub fn bite_in(
        &mut self,
        grain: usize,
        side: usize,
        band: usize,
        u: f64,
        look: &BiteLook,
        span: [f64; 2],
    ) -> f64 {
        let size = extent(across_axes(grain)[1 - side / 2]);
        let [s0, s1] = [span[0].clamp(0.0, 1.0), span[1].clamp(0.0, 1.0)];
        let (s0, s1) = (s0.min(s1), s1.max(s0));
        let w = (look.width_m / size).clamp(0.01, (s1 - s0).max(0.01));
        let (lo, hi) = (s0 + 0.5 * w, (s1 - 0.5 * w).max(s0 + 0.5 * w));
        let step = (look.step * w).max(1e-3);
        let band = band.min(1) as u8;
        let mine: Vec<f64> = self.bites[side]
            .all()
            .iter()
            .map(|b| f64::from(b.u))
            .collect();
        let free = |c: f64| mine.iter().all(|&m| (m - c).abs() >= step - 1e-6);
        let aimed = u.clamp(lo, hi);
        let mut cands = vec![aimed];
        for &m in &mine {
            cands.extend([m - step, m + step]);
        }
        let at = cands
            .into_iter()
            .filter(|&c| (lo - 1e-9..=hi + 1e-9).contains(&c) && free(c))
            .min_by(|a, b| (a - aimed).abs().total_cmp(&(b - aimed).abs()));
        match at {
            Some(c) => {
                self.bites[side].push(Bite { u: c as f32, band });
                c
            }
            None => aimed,
        }
    }

    /// The bites in the order the cut's sides list them, after the notch and kerfs: each side's
    /// count and its bites (`u` as f32, the band).
    pub fn write_bites(&self, w: &mut ByteWriter) {
        for b in &self.bites {
            b.write(w);
        }
    }

    /// Reads [`Cut::write_bites`] into this cut.
    pub fn read_bites(&mut self, r: &mut ByteReader) {
        for b in &mut self.bites {
            *b = Bites::read(r);
        }
    }

    /// The side holding the notch (the chopping prototype's): the first side cut, a V.
    pub fn notch_side(&self) -> Option<usize> {
        (0..SIDES).find(|&s| self.notch[s] > 0.0)
    }

    /// One chop of the chopping prototype (WORK_QUEUE item 7) at side `side`: it removes `tool`'s
    /// `chop2_bite` of the cross-section (`wood`'s `along` of that when the swing is `along_grain`),
    /// never past the far side. The first side cut is the **notch** (kept in `notch`, drawn as a
    /// V); a chop at any other side once there is a notch is a **back cut** (kept in `kerf[side][0]`,
    /// drawn as a flat slit). The same record and wire as today's cuts. Returns the share cut.
    pub fn chop2(
        &mut self,
        tool: &ToolDef,
        wood: &WoodDef,
        grain: usize,
        side: usize,
        along_grain: bool,
    ) -> f64 {
        let e = extent(side_axis(grain, side));
        let share = tool.chop2_bite * if along_grain { wood.chop2.along } else { 1.0 };
        // A cut takes a slab off its side across what is left of the other axis: deepening it by
        // `d` removes `d × left_other` of the section, so a chop of `share` goes `share × area /
        // left_other` deep (owner, round 2: "the remaining section is what's left after all cuts").
        let [ea, eb] = across_axes(grain).map(extent);
        let other = 1 - side / 2;
        let left_other =
            (if other == 0 { ea } else { eb }) - self.depth(2 * other) - self.depth(2 * other + 1);
        let room = (e - self.depth(side) - self.depth(side ^ 1)).max(0.0);
        let want = share * ea * eb / left_other.max(1e-6);
        let gain = want.min(room);
        match self.notch_side() {
            Some(n) if n != side => self.kerf[side][0] += gain as f32,
            _ => self.notch[side] += gain as f32,
        }
        gain * left_other.max(0.0) / (ea * eb)
    }

    /// The share of the section the chopping prototype's cuts have removed: 1 − what is left
    /// after every side's cut (each a slab off its side), so cuts on adjacent sides add up too.
    pub fn removed(&self, grain: usize) -> f64 {
        let [ea, eb] = across_axes(grain).map(extent);
        let a = (ea - self.depth(0) - self.depth(1)).max(0.0);
        let b = (eb - self.depth(2) - self.depth(3)).max(0.0);
        1.0 - a * b / (ea * eb)
    }

    /// Whether the chopping prototype fells (a standing wood) or cuts through (a lying one) a
    /// cell of grain `grain` cut like this: more than `fell_fraction` of its section is cut.
    pub fn chop2_through(&self, wood: &WoodDef, grain: usize) -> bool {
        self.removed(grain) > wood.chop2.fell_fraction + 1e-6
    }

    /// The side the chopping prototype fells toward: the notch's (the hinge steers the fall), else
    /// the deepest cut.
    pub fn chop2_toward(&self, grain: usize) -> Option<usize> {
        self.notch_side().or_else(|| self.deepest(grain))
    }

    pub fn write(&self, w: &mut ByteWriter) {
        for s in 0..SIDES {
            w.f32(self.notch[s]);
            w.f32(self.kerf[s][0]);
            w.f32(self.kerf[s][1]);
        }
    }

    pub fn read(r: &mut ByteReader) -> Cut {
        let mut c = Cut::default();
        for s in 0..SIDES {
            c.notch[s] = r.f32();
            c.kerf[s][0] = r.f32();
            c.kerf[s][1] = r.f32();
        }
        c
    }
}

/// The cuts in a world: wood cells swung at, by cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cuts {
    pub cells: std::collections::HashMap<[i32; 3], Cut>,
}

/// The save record's version.
const CUTS_VERSION: u8 = 1;

/// The mark before the bites' section in a save record and a `Cuts` message.
pub const BITES_MARK: u8 = 1;

impl Cuts {
    /// The cells in a stable order (saves and messages).
    pub fn sorted(&self) -> Vec<([i32; 3], Cut)> {
        let mut v: Vec<_> = self.cells.iter().map(|(&c, &k)| (c, k)).collect();
        v.sort_by_key(|(c, _)| *c);
        v
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = ByteWriter::new(&mut out);
        w.u8(CUTS_VERSION);
        let all = self.sorted();
        w.u32(all.len() as u32);
        for (c, k) in &all {
            for &v in c {
                w.i32(v);
            }
            k.write(&mut w);
        }
        // The bites (WORK_QUEUE item 26), after every cell, in the same order: an older build
        // can't read the record and keeps it unread; an older record has none (drawn as before).
        if all.iter().any(|(_, k)| k.has_bites()) {
            w.u8(BITES_MARK);
            for (_, k) in &all {
                k.write_bites(&mut w);
            }
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Cuts> {
        let mut r = ByteReader::new(bytes);
        if r.u8() != CUTS_VERSION {
            return None;
        }
        let n = r.u32() as usize;
        let mut order = Vec::new();
        for _ in 0..n.min(bytes.len() / 60 + 1) {
            let c = [r.i32(), r.i32(), r.i32()];
            order.push((c, Cut::read(&mut r)));
        }
        if r.ok() && !r.at_end() {
            let mark = r.u8();
            r.check(mark == BITES_MARK);
            for (_, k) in &mut order {
                k.read_bites(&mut r);
            }
        }
        let cells = order.into_iter().collect();
        (r.ok() && r.at_end()).then_some(Cuts { cells })
    }
}

#[cfg(test)]
mod weapon_tests {
    use super::*;

    // BUGS.md: the sword showed the axe's hint ("Axe: level swing …"); the hint is the kind's data.
    #[test]
    fn each_tool_shows_its_own_kinds_hint() {
        let sword = tool("dwell:sword").expect("the sword");
        let axe = tool("dwell:stone_axe").expect("the axe");
        let s = hint(sword.kind, false).expect("a weapon hint");
        assert!(s.starts_with("Sword"), "{s}");
        assert!(hint(sword.kind, true).unwrap().contains("vertical stance"));
        assert!(hint(axe.kind, false).unwrap().starts_with("Axe: level"));
        assert!(hint(axe.kind, true).unwrap().starts_with("Axe: vertical"));
        // Every tool in the data, the fixture's too, has its kind's hint.
        for t in TOOLS.iter().chain(fixture::TOOLS) {
            assert!(
                hint_in(fixture::HINTS, t.kind, false).is_some() || hint(t.kind, false).is_some(),
                "{}",
                t.name
            );
        }
    }

    fn sword() -> WeaponDef {
        tool("dwell:sword")
            .and_then(|t| t.weapon)
            .expect("the sword is a weapon")
    }

    // Owner, 2026-11-12: "if you hit enemy sword within an angle threshold of perpendicular you're
    // blocked, if within an angle of parallel it connects".
    #[test]
    fn a_strike_across_a_raised_blade_is_blocked_and_one_along_it_connects() {
        let w = sword();
        let g = w.guard;
        // A level slash on an upright blade: perpendicular, blocked; on a level blade: parallel.
        assert_eq!(g.clash(false, &g, true), Clash::Blocked);
        assert_eq!(g.clash(true, &g, false), Clash::Blocked);
        assert_eq!(g.clash(false, &g, false), Clash::Connected);
        assert_eq!(g.clash(true, &g, true), Clash::Connected);
        // By the data's thresholds: within blockWithinDeg of 90 blocks, within connectWithinDeg
        // of 0 connects, between glances.
        let edge = 90.0 - g.block_within_deg;
        assert_eq!(g.clash_at(edge + 1.0, 0.0), Clash::Blocked);
        assert_eq!(
            g.clash_at(g.connect_within_deg - 1.0, 0.0),
            Clash::Connected
        );
        assert_eq!(g.clash_at(45.0, 0.0), Clash::Glanced);
        assert_eq!(g.damage_share(Clash::Blocked), 0.0);
        assert_eq!(g.damage_share(Clash::Glanced), g.glance_damage);
        // The angle is between lines, folded to 0..90, the holder's roll mirrored.
        assert!((GuardDef::blade_angle(170.0, 0.0) - 10.0).abs() < 1e-9);
        assert!((GuardDef::blade_angle(30.0, -30.0)).abs() < 1e-9);
    }

    #[test]
    fn the_sword_is_a_weapon_by_data_that_cuts_no_wood_and_wears() {
        let t = tool("dwell:sword").unwrap();
        assert!(t.is_weapon() && t.clears.is_empty());
        let w = sword();
        assert!(w.damage > 0.0 && w.knockback_m > 0.0 && w.max_targets >= 1);
        assert!(durability("dwell:sword").is_some(), "it wears");
    }

    // A weapon added by data alone (shared/tools/fixtures/tools.json): the stick as a club.
    #[test]
    fn a_fixture_weapon_by_data_alone() {
        let t = fixture::TOOLS
            .iter()
            .find(|t| t.name == "dwell:stick")
            .unwrap();
        assert_eq!(t.kind, ToolKind::Weapon);
        let w = t.weapon.unwrap();
        assert_eq!((w.damage, w.knockback_m, w.max_targets), (3.0, 0.25, 1));
        assert_eq!(durability_in(fixture::DURABILITY, "dwell:stick"), Some(4));
        // Its guard took the defaults: level 0, upright 90, 45 / 45.
        assert_eq!(w.guard.clash(false, &w.guard, true), Clash::Blocked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axe() -> &'static ToolDef {
        tool("dwell:stone_axe").expect("the stone axe is data")
    }

    #[test]
    fn the_right_tool_breaks_its_material_faster_metal_faster_than_stone() {
        // Pickaxe on stone, shovel on dirt and sand, axe on wood (owner, 2026-11-10).
        for (block, stone, metal) in [
            ("dwell:stone", "dwell:stone_pickaxe", "dwell:metal_pickaxe"),
            ("dwell:dirt", "dwell:wooden_shovel", "dwell:metal_shovel"),
            ("dwell:sand", "dwell:wooden_shovel", "dwell:metal_shovel"),
            ("dwell:log", "dwell:stone_axe", "dwell:metal_axe"),
        ] {
            let (bare, s, m) = (
                break_speed(block, None),
                break_speed(block, Some(stone)),
                break_speed(block, Some(metal)),
            );
            assert_eq!(bare, 1.0, "{block} by hand is as before");
            assert!(
                s > bare && m > s,
                "{block}: hand {bare}, {stone} {s}, {metal} {m}"
            );
        }
        // The wrong tool, or a block no class names, is as before.
        assert_eq!(break_speed("dwell:stone", Some("dwell:metal_shovel")), 1.0);
        assert_eq!(break_speed("dwell:log", Some("dwell:metal_pickaxe")), 1.0);
        assert_eq!(break_speed("dwell:leaves", Some("dwell:metal_axe")), 1.0);
        // Every breaking tool is an inventory item with a model look (the tool models).
        for t in BREAK_TOOLS {
            let item = crate::inventory::item(t.item).expect("a breaking tool is an item");
            assert!(
                item.look.is_some_and(|l| !l.model.is_empty()),
                "{} has a model look",
                t.item
            );
        }
    }

    #[test]
    fn a_fixture_tool_class_and_tier_by_data_alone_breaks_faster() {
        let classes = [("test:clay", "trowel")];
        let tools = [BreakTool {
            item: "test:trowel",
            class: "trowel",
            tier: "bronze",
            speed: 2.0,
            contact_s: 0.4,
            repeat_s: None,
        }];
        assert_eq!(
            break_speed_in(&classes, &tools, "test:clay", Some("test:trowel")),
            2.0
        );
        assert_eq!(break_speed_in(&classes, &tools, "test:clay", None), 1.0);
        assert_eq!(
            break_speed_in(&classes, &tools, "test:rock", Some("test:trowel")),
            1.0
        );
    }

    #[test]
    fn a_tools_contact_time_is_its_data_and_a_hand_lands_at_once() {
        // WORK_QUEUE item 25: a fixture tool's contact by data alone.
        let tools = [BreakTool {
            item: "test:trowel",
            class: "trowel",
            tier: "bronze",
            speed: 2.0,
            contact_s: 0.4,
            repeat_s: None,
        }];
        assert_eq!(break_contact_s_in(&tools, Some("test:trowel")), 0.4);
        assert_eq!(break_contact_s_in(&tools, None), 0.0);
        assert_eq!(break_contact_s_in(&tools, Some("dwell:dirt")), 0.0);
        // The game's pickaxes and shovels strike with the arms' break clip.
        for t in ["dwell:stone_pickaxe", "dwell:metal_shovel"] {
            assert!(break_contact_s(Some(t)) > 0.2, "{t}");
        }
    }
    const UP: [f64; 3] = [0.0, 1.0, 0.0];
    const Y: usize = 1;

    #[test]
    fn a_hit_stops_short_and_a_miss_follows_through_longer() {
        let s = axe().swing;
        // A hit early on the arc ends sooner than one at its end, and every hit sooner than a miss.
        assert!(s.hit_length(0) < s.hit_length(s.samples - 1));
        assert!(s.hit_length(s.samples - 1) < s.miss_length());
        assert!(
            (s.time_of_step(0) - s.windup_s).abs() < 1e-12,
            "the arc starts after the wind-up"
        );
        assert!(s.hit_stop_s > 0.0 && s.recover_miss_s > s.recover_hit_s);
        // Never sooner than the server's wait.
        assert!(s.miss_length() >= axe().swing_seconds);
        assert!(
            s.stamina_cost > 0.0,
            "a swing names its stamina cost (unused until stamina)"
        );
    }

    #[test]
    fn the_tree_sways_more_toward_the_fall_and_the_hits_sound_deeper() {
        let log = wood("dwell:log").unwrap();
        assert_eq!(log.sway_deg(0.0), 0.0);
        assert_eq!(
            log.sway_deg(log.sway.from_fraction),
            0.0,
            "still until ~40% through"
        );
        let mid = log.sway_deg((log.sway.from_fraction + log.fell_fraction) / 2.0);
        let end = log.sway_deg(log.fell_fraction);
        assert!(
            mid > 0.0 && end > 2.0 * mid,
            "more toward the fall: {mid} then {end}"
        );
        assert!((end - log.sway.max_deg).abs() < 1e-9);
        assert_eq!(log.sound_at(0.0), Some("chop_bark"));
        assert_eq!(log.sound_at(0.3), Some("chop_wood"));
        assert_eq!(log.sound_at(0.5), Some("chop_creak"));
    }

    /// Swings at a standing (grain y) 1 m trunk's −x side, `band_of(i)` for swing i, with
    /// `share` each, until the cut passes the log's fell fraction; the count.
    fn swings_to_fell(s: f64, band_of: impl Fn(usize) -> usize) -> usize {
        let fell = wood("dwell:log").expect("log").fell_fraction;
        let mut c = Cut::default();
        for i in 0..1000 {
            c.swing(axe(), Y, 0, band_of(i), s);
            if c.cross(Y) >= fell {
                return i + 1;
            }
        }
        panic!("never fell");
    }

    /// The notch's bite rule (WORK_QUEUE item 18): a cut read alone says how far along its swings
    /// to fell it is (k of 11 for the stone axe, k of 5 for the metal one, at one aim as the server
    /// lands them), so the carve can size the notch for any axe without knowing which cut it.
    #[test]
    fn a_cuts_progress_is_its_swings_over_the_swings_to_fell_for_any_axe() {
        let wood = wood("dwell:log").unwrap();
        let look = wood.cut.bite;
        for (name, n) in [("dwell:stone_axe", 11u32), ("dwell:metal_axe", 5)] {
            let t = tool(name).unwrap();
            assert_eq!(super::swings_to_fell(t, wood), n, "{name}");
            let mut c = Cut::default();
            for k in 1..=n {
                let band = c.open_band(0, 0);
                c.swing(t, 1, 0, band, t.across);
                c.bite(1, 0, band, 0.5, &look);
                let p = c.progress(wood, 1);
                assert!(
                    (p - f64::from(k) / f64::from(n)).abs() < 1e-6,
                    "{name} swing {k}: {p}"
                );
            }
        }
        // A cut without bites (an older save's): its cross-section over the fell fraction.
        let mut old = Cut::default();
        old.notch[0] = 0.275;
        assert!((old.progress(wood, 1) - 0.5).abs() < 1e-6);
    }

    /// The axe's bite rule: a chop takes 0.5 × the cell ÷ the swings to fell, so the swings to
    /// the fall total half the cell; the metal axe's fewer swings take bigger bites.
    #[test]
    fn a_chop_bites_half_the_cell_over_the_swings_to_fell() {
        let log = wood("dwell:log").unwrap();
        let (stone, metal) = (
            tool("dwell:stone_axe").unwrap(),
            tool("dwell:metal_axe").unwrap(),
        );
        assert_eq!(super::swings_to_fell(stone, log), 11);
        assert_eq!(super::swings_to_fell(metal, log), 5);
        for t in [stone, metal] {
            let n = f64::from(super::swings_to_fell(t, log));
            assert!((chop_bite_m3(t, log) * n - 0.5 * crate::damage::CELL_M3).abs() < 1e-12);
        }
        assert!(chop_bite_m3(metal, log) > 2.0 * chop_bite_m3(stone, log));
    }

    #[test]
    fn a_horizontal_swing_crosses_a_standing_trunk_and_a_vertical_one_splits_it() {
        let h = share(axe(), swing_normal(false, [1.0, 0.0, 0.0]), UP);
        let v = share(axe(), swing_normal(true, [1.0, 0.0, 0.0]), UP);
        assert_eq!(h, axe().across);
        assert_eq!(v, axe().along);
        // On a log lying along x, looked at from the side (+z): vertical crosses, horizontal splits.
        let x = [1.0, 0.0, 0.0];
        assert_eq!(
            share(axe(), swing_normal(true, [0.0, -0.5, 1.0]), x),
            axe().across
        );
        assert_eq!(
            share(axe(), swing_normal(false, [0.0, -0.5, 1.0]), x),
            axe().along
        );
        // Looked at along its length, a vertical swing splits it too.
        assert!(share(axe(), swing_normal(true, [1.0, -0.3, 0.0]), x) < 0.2);
        // Leaning at 45°: between.
        let lean = [
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ];
        let m = share(axe(), swing_normal(false, [1.0, 0.0, 0.0]), lean);
        assert!(m > axe().along && m < axe().across, "{m}");
    }

    #[test]
    fn notching_fells_in_eight_to_twelve_swings_and_hacking_takes_three_to_four_times_more() {
        let notch = swings_to_fell(1.0, |i| i % 2);
        let hack = swings_to_fell(1.0, |_| 0);
        assert!((8..=12).contains(&notch), "notching: {notch} swings");
        let ratio = hack as f64 / notch as f64;
        assert!(
            (3.0..=4.0).contains(&ratio),
            "hacking: {hack} swings, {ratio:.2}× notching"
        );
        // Vertical swings at a standing trunk barely cut it.
        let split = swings_to_fell(axe().along, |i| i % 2);
        assert!(split >= 5 * notch, "vertical: {split} swings");
    }

    /// Chops of the prototype at sides `side_of(i)` until it is through; the count and the cut.
    fn chop2_to_fell(
        tool: &ToolDef,
        along: bool,
        side_of: impl Fn(usize) -> usize,
    ) -> (usize, Cut) {
        let log = wood("dwell:log").expect("log");
        let mut c = Cut::default();
        for i in 0..1000 {
            c.chop2(tool, log, Y, side_of(i), along);
            if c.chop2_through(log, Y) {
                return (i + 1, c);
            }
        }
        panic!("never fell");
    }

    #[test]
    fn chop2_the_stone_axe_fells_in_about_twelve_along_the_grain_twice_that_metal_fewer() {
        let (stone, _) = chop2_to_fell(axe(), false, |_| 0);
        assert!((11..=13).contains(&stone), "stone: {stone} chops");
        let (along, _) = chop2_to_fell(axe(), true, |_| 0);
        let r = along as f64 / stone as f64;
        assert!(
            (1.7..=2.3).contains(&r),
            "along the grain: {along} chops, {r:.2}×"
        );
        let metal = tool("dwell:metal_axe").expect("metal axe");
        let (m, _) = chop2_to_fell(metal, false, |_| 0);
        assert!(m < stone, "metal: {m} chops, stone {stone}");
        // Not before half: one chop short of it still stands.
        let log = wood("dwell:log").unwrap();
        let mut c = Cut::default();
        for _ in 0..stone - 1 {
            c.chop2(axe(), log, Y, 0, false);
        }
        assert!(!c.chop2_through(log, Y) && c.cross(Y) <= 0.5 + 1e-6);
    }

    #[test]
    fn chop2_notch_then_back_cut_falls_toward_the_notch_one_sided_toward_the_cut() {
        // A notch on side 1 (+x), then the back cut from side 0 (−x), deeper: toward the notch.
        let (_, c) = chop2_to_fell(axe(), false, |i| if i < 4 { 1 } else { 0 });
        assert_eq!(c.notch_side(), Some(1));
        assert!(
            c.kerf[0][0] > c.notch[1],
            "the back cut is the deeper: {c:?}"
        );
        assert_eq!(c.notch[0], 0.0, "the back cut is flat (a kerf), not a V");
        assert_eq!(
            c.chop2_toward(Y),
            Some(1),
            "the hinge steers it toward the notch"
        );
        // One-sided: toward the side cut.
        let (_, c) = chop2_to_fell(axe(), false, |_| 2);
        assert_eq!(c.chop2_toward(Y), Some(2));
        // The cut never passes the far side.
        let log = wood("dwell:log").unwrap();
        let mut c = Cut::default();
        for i in 0..200 {
            c.chop2(axe(), log, Y, i % 2, false);
        }
        assert!(c.depth(0) + c.depth(1) <= 1.0 + 1e-6);
    }

    #[test]
    fn chop2_cuts_on_adjacent_sides_add_up() {
        // Red before: only opposite sides counted, so 7 chops on −x and 7 on −z (each side under
        // half) never felled it.
        let log = wood("dwell:log").unwrap();
        let mut c = Cut::default();
        let mut n = 0;
        for i in 0..40 {
            c.chop2(axe(), log, Y, if i % 2 == 0 { 0 } else { 2 }, false);
            n += 1;
            if c.chop2_through(log, Y) {
                break;
            }
        }
        assert!(c.chop2_through(log, Y), "adjacent sides fell it: {c:?}");
        assert!((11..=13).contains(&n), "{n} chops, as from one side");
        assert!(c.cross(Y) < 0.5, "neither axis alone is half: {c:?}");
        assert_eq!(
            c.chop2_toward(Y),
            Some(0),
            "toward the notch, the first side cut"
        );
    }

    #[test]
    fn chop2_a_fixture_tool_cuts_by_its_data_alone() {
        let mut t = *axe();
        t.chop2_bite = 0.26;
        let (n, _) = chop2_to_fell(&t, false, |_| 0);
        assert_eq!(n, 2, "0.26 a chop: past half in two");
    }

    /// BUGS.md #75: a swing aimed at the band already cut deeper lands in the other, so one aim
    /// notches; at an even cut it lands where aimed.
    #[test]
    fn a_swing_at_the_deeper_band_lands_in_the_open_one() {
        let mut c = Cut::default();
        assert_eq!(c.open_band(0, 0), 0);
        assert_eq!(c.open_band(0, 1), 1);
        c.swing(axe(), Y, 0, 0, 1.0);
        assert_eq!(c.open_band(0, 0), 1);
        assert_eq!(c.open_band(0, 1), 1);
        assert_eq!(c.open_band(2, 0), 0, "another side as aimed");
    }

    #[test]
    fn a_chunk_comes_out_when_both_bands_are_cut() {
        let mut c = Cut::default();
        c.swing(axe(), Y, 0, 0, 1.0);
        assert_eq!(c.notch[0], 0.0);
        assert!((f64::from(c.kerf[0][0]) - axe().bite_m).abs() < 1e-6);
        c.swing(axe(), Y, 0, 1, 1.0);
        assert!((f64::from(c.notch[0]) - axe().bite_m).abs() < 1e-6, "{c:?}");
        assert_eq!(c.kerf[0], [0.0, 0.0]);
        assert_eq!(c.deepest(Y), Some(0));
        // A cut never goes past the far side.
        for i in 0..400 {
            c.swing(axe(), Y, 0, i % 2, 1.0);
        }
        assert!(c.depth(0) <= 1.0 + 1e-6);
        assert_eq!(c.cross(Y), 1.0);
    }

    #[test]
    fn sides_are_the_faces_across_the_grain() {
        assert_eq!(
            side_of(1, 1, true),
            None,
            "a standing trunk's top is end grain"
        );
        assert_eq!(side_of(1, 0, false), Some(0));
        assert_eq!(side_normal(1, 3), [0, 0, 1]);
        assert_eq!(side_of(0, 1, true), Some(1), "a log along x: its top");
        assert_eq!(side_axis(0, 1), 1);
        for g in 0..3 {
            for s in 0..SIDES {
                let n = side_normal(g, s);
                let a = n.iter().position(|&v| v != 0).unwrap();
                assert_eq!(side_of(g, a, n[a] > 0), Some(s));
            }
        }
    }

    #[test]
    fn the_new_messages_round_trip_and_reject_bad_ones() {
        use crate::{ChopAction, CutsMessage, Message};
        let mut c = Cut::default();
        c.swing(axe(), Y, 1, 0, 1.0);
        let all = [
            Message::Chop(ChopAction::Hello),
            Message::Chop(ChopAction::Swing {
                tool: "dwell:stone_axe".into(),
                cell: [4, -80, 12],
                face: 1,
                point: [5.0, -39.8, 12.5],
                vertical: true,
            }),
            Message::Cuts(CutsMessage {
                cuts: vec![([4, -80, 12], c)],
                gone: vec![[1, 2, 3]],
                bites: false,
            }),
            // With the bites (WORK_QUEUE item 26; to a client that said `BitesHello`).
            Message::Cuts(CutsMessage {
                cuts: vec![([4, -80, 12], bitten()), ([5, -80, 12], c)],
                gone: vec![[1, 2, 3]],
                bites: true,
            }),
            Message::BitesHello,
            // The swing's path and the held tool (CHOP_SWEEP).
            Message::Chop(ChopAction::Sweep {
                tool: "dwell:stone_axe".into(),
                look: [0.6, -0.8, 0.0],
                vertical: false,
            }),
            Message::Chop(ChopAction::Held {
                tool: "dwell:stone_axe".into(),
                vertical: true,
            }),
            Message::HeldTools(vec![
                crate::HeldTool {
                    player_id: 3,
                    tool: "dwell:stone_axe".into(),
                    vertical: false,
                },
                crate::HeldTool {
                    player_id: 9,
                    tool: String::new(),
                    vertical: false,
                },
            ]),
        ];
        for m in all {
            let b = m.encode();
            assert_eq!(Message::decode(&b), Some(m.clone()));
            assert_eq!(Message::decode(&b[..b.len() - 1]), None, "{m:?}");
        }
        let mut bad = Message::Chop(ChopAction::Hello).encode();
        *bad.last_mut().unwrap() = 9;
        assert_eq!(Message::decode(&bad), None);
    }

    #[test]
    fn cuts_round_trip_and_bad_records_are_refused() {
        let mut cuts = Cuts::default();
        let mut c = Cut::default();
        c.swing(axe(), Y, 2, 1, 1.0);
        cuts.cells.insert([3, -40, 7], c);
        cuts.cells.insert([-1, 2, 0], Cut::default());
        let b = cuts.to_bytes();
        assert_eq!(Cuts::from_bytes(&b), Some(cuts));
        assert_eq!(Cuts::from_bytes(&b[..b.len() - 1]), None);
        let mut v = b.clone();
        v[0] = 9;
        assert_eq!(Cuts::from_bytes(&v), None);
    }

    /// A cut with bites on two sides (a hit beside another, and one in the other band).
    fn bitten() -> Cut {
        let mut c = Cut::default();
        c.swing(axe(), Y, 0, 0, 1.0);
        c.bite(Y, 0, 0, 0.5, &BITE_LOOK);
        c.swing(axe(), Y, 0, 0, 1.0);
        c.bite(Y, 0, 0, 0.5, &BITE_LOOK);
        c.swing(axe(), Y, 3, 1, 1.0);
        c.bite(Y, 3, 1, 0.2, &BITE_LOOK);
        c
    }

    /// WORK_QUEUE item 26: each hit records where it struck across the side; a hit on a bitten
    /// spot (either band) lands beside it, nearest the aim, in the open wood; bites stay in
    /// the cell and a side keeps at most `MAX_BITES`.
    #[test]
    fn a_hit_bites_where_it_struck_or_beside_a_bite_there() {
        let l = BITE_LOOK;
        let step = l.step * l.width_m;
        let mut c = Cut::default();
        assert!(!c.has_bites());
        assert!((c.bite(Y, 0, 0, 0.5, &l) - 0.5).abs() < 1e-9);
        assert!(c.has_bites());
        // The same spot again: beside it (the nearer of the two sides to a slightly-right aim).
        assert!((c.bite(Y, 0, 0, 0.52, &l) - (0.5 + step)).abs() < 1e-6);
        assert!((c.bite(Y, 0, 0, 0.5, &l) - (0.5 - step)).abs() < 1e-6);
        // The other band lands beside them too (round 2: across the side, not down it).
        assert!((c.bite(Y, 0, 1, 0.5, &l) - (0.5 + 2.0 * step)).abs() < 1e-6);
        // At the cell's edge: kept inside it (the mouth in the cell).
        assert!((c.bite(Y, 2, 0, 0.0, &l) - 0.5 * l.width_m).abs() < 1e-9);
        // A lying log's side across y (½ m): the width is a larger share of it.
        let mut k = Cut::default();
        assert!((k.bite(0, 2, 0, 0.0, &l) - 0.5 * l.width_m / 0.5).abs() < 1e-9);
        // Hacking one spot fills the side, then stops adding (the cut deepens them).
        for _ in 0..40 {
            c.bite(Y, 1, 0, 0.5, &l);
        }
        let n = c.bites[1].all().len();
        assert!((7..=MAX_BITES).contains(&n), "{n} bites");
        let mut us: Vec<f64> = c.bites[1].all().iter().map(|b| f64::from(b.u)).collect();
        us.sort_by(f64::total_cmp);
        assert!(us.windows(2).all(|w| w[1] - w[0] >= step - 1e-5), "{us:?}");
        assert!(us[0] >= 0.5 * l.width_m - 1e-6 && us[n - 1] <= 1.0 - 0.5 * l.width_m + 1e-6);
    }

    /// WORK_QUEUE item 26 round 2: eleven hits at one spot of a standing trunk, the bands
    /// alternating as [`Cut::open_band`] has them, spread across the side to span the log (red
    /// before: per band they reached only ~0.56 of a full log), and on a thinner trunk (an octagon
    /// half the cell wide) they stay on the log and span it too.
    #[test]
    fn eleven_hits_at_one_spot_spread_across_the_log() {
        let l = BITE_LOOK;
        for span in [[0.0, 1.0], [0.25, 0.75]] {
            let mut c = Cut::default();
            for _ in 0..11 {
                let band = c.open_band(0, 0);
                c.swing(axe(), Y, 0, band, 1.0);
                c.bite_in(Y, 0, band, 0.5, &l, span);
            }
            let us: Vec<f64> = c.bites[0].all().iter().map(|b| f64::from(b.u)).collect();
            let lo = us.iter().copied().fold(f64::INFINITY, f64::min) - 0.5 * l.width_m;
            let hi = us.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 0.5 * l.width_m;
            assert!(
                lo >= span[0] - 1e-6 && hi <= span[1] + 1e-6,
                "{span:?}: {us:?}"
            );
            let covered = (hi - lo) / (span[1] - span[0]);
            assert!(
                covered > 0.9,
                "{span:?}: the bites span {covered:.2} of the log: {us:?}"
            );
        }
    }

    /// WORK_QUEUE item 26: a wood's fell wedge (the stump's ramp) is chopped as that wood and no
    /// axe clears it; leaves are still cleared.
    #[test]
    fn a_stumps_wedge_is_chopped_as_its_wood_not_cleared() {
        let log = wood("dwell:log").expect("log");
        let wedge = log.fell_wedge.expect("the log's wedge");
        assert!(wood(wedge).is_none());
        assert_eq!(chopped_wood(wedge).map(|w| w.block), Some("dwell:log"));
        assert_eq!(
            chopped_wood("dwell:log").map(|w| w.block),
            Some("dwell:log")
        );
        assert!(chopped_wood("dwell:stone").is_none());
        for t in TOOLS {
            assert!(!t.clears_block(wedge), "{} clears {wedge}", t.name);
        }
        assert!(axe().clears_block("dwell:leaves"));
    }

    /// The bites ride the save record after its cells: read back; a record without them (an
    /// older build's) loads with none (drawn as before); a bad mark or a truncated one fails.
    #[test]
    fn the_cuts_record_keeps_bites_and_an_older_record_loads_without() {
        let mut cuts = Cuts::default();
        cuts.cells.insert([1, 2, 3], bitten());
        let mut plain = Cut::default();
        plain.swing(axe(), Y, 1, 0, 1.0);
        cuts.cells.insert([0, 0, 0], plain);
        let b = cuts.to_bytes();
        assert_eq!(Cuts::from_bytes(&b), Some(cuts.clone()));
        assert_eq!(Cuts::from_bytes(&b[..b.len() - 1]), None);
        // An older record: the same cells and depths, no bites section.
        let mut older = Cuts::default();
        for (c, k) in &cuts.cells {
            older.cells.insert(
                *c,
                Cut {
                    bites: Default::default(),
                    ..*k
                },
            );
        }
        let ob = older.to_bytes();
        assert!(ob.len() < b.len());
        let read = Cuts::from_bytes(&ob).expect("an older record reads");
        assert!(read.cells.values().all(|k| !k.has_bites()));
        assert_eq!(read, older);
        let mut bad = b.clone();
        let mark = ob.len();
        bad[mark] = 7;
        assert_eq!(Cuts::from_bytes(&bad), None);
    }
}
