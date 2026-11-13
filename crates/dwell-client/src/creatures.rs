//! The creatures the server tells this client of (docs/adr/0073-creatures.md): each one's latest
//! point, heading and speed, drawn smoothed toward it by the skinned renderer with the clip its
//! speed picks. An NPC (WORK_QUEUE item 38) also carries its part (`NpcNet`, from a server that
//! knows this client has `creatures.npcs`): its idle pose picks its role, and its kind's data
//! (shared/creatures/creatures.json) and palette variant seed pick its colours and held item.

use std::collections::HashMap;

use dwell_protocol::{
    CREATURE_POSES, CreatureNet, CreaturesMessage, NPC_POSES, NPC_STRIKE_POSES, NpcNet,
};
use dwell_sim::creatures::{Table, npc_colours};
use dwell_world::FaceFrame;

/// Below this speed (m/s) a creature idles; above `RUN_ABOVE` it plays its `sprint` role (the
/// run clip); between, `walk`.
pub const IDLE_BELOW: f32 = 0.05;
pub const RUN_ABOVE: f32 = 3.0;
/// The smoothing's rate (1/s): the drawn point closes this share of the gap a second.
const FOLLOW: f64 = 12.0;

/// The role a creature's speed plays (`dwell_models::ModelData::role`'s names).
pub fn role_for_speed(speed: f32) -> &'static str {
    if speed < IDLE_BELOW {
        "idle"
    } else if speed > RUN_ABOVE {
        "sprint"
    } else {
        "walk"
    }
}

/// The role an NPC plays: its speed's while it moves, else its idle pose's (`stand` plays
/// `idle`; `look` and `sit` play their own roles, a model without them its fallback).
/// `armed`: it holds a weapon (a defeated one plants it, `kneel_sword`).
pub fn npc_role(speed: f32, pose: u8, armed: bool) -> &'static str {
    // Its sword's swing (`npcs.swings`): the rig's chop clips, whatever its feet do; defeated
    // (`humans.defeat`, CHARACTERS.md §11.13): kneeling, then standing up at a shrine.
    let r = &dwell_protocol::defeat::ROLES;
    match usize::from(pose).checked_sub(NPC_POSES.len()) {
        Some(0) => return "chop_level",
        Some(1) => return "chop_vertical",
        Some(2) => return if armed { r.kneel_armed } else { r.kneel },
        Some(3) => return if armed { r.stand_armed } else { r.stand },
        _ => {}
    }
    debug_assert_eq!(NPC_STRIKE_POSES.len(), 2);
    debug_assert_eq!(dwell_protocol::NPC_DEFEAT_POSES.len(), 2);
    match (role_for_speed(speed), NPC_POSES.get(usize::from(pose))) {
        ("idle", Some(&"look")) => "look",
        ("idle", Some(&"sit")) => "sit",
        (r, _) => r,
    }
}

/// An NPC's swing role (`chop_level`, `chop_vertical` from [`npc_role`]) as its weapon's `clips`
/// name it (the sword's slashes, WORK_QUEUE item 35); any other role as it is.
pub fn npc_swing_role(role: &'static str, held: Option<&str>) -> &'static str {
    match (role, held.and_then(dwell_protocol::chopping::tool)) {
        ("chop_level", Some(t)) => t.swing_role(false),
        ("chop_vertical", Some(t)) => t.swing_role(true),
        _ => role,
    }
}

/// Whether an NPC holds a weapon (a tool of kind `Weapon`, shared/tools).
fn npc_armed(l: &NpcLook) -> bool {
    l.held
        .and_then(dwell_protocol::chopping::tool)
        .is_some_and(|t| t.kind == dwell_protocol::chopping::ToolKind::Weapon)
}

/// An NPC's looks as drawn: its colours (per part, the model's palette ramp → the one drawn in its
/// place) and the item in its hand.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcLook {
    pub kind: String,
    pub colours: Vec<dwell_sim::creatures::NpcColour>,
    pub held: Option<&'static str>,
    /// Its raised block's role (`guard_level`, `guard_vertical`) while the server says it guards
    /// (`Swordplay`'s guards; WORK_QUEUE item 35), set by the game each frame; None lowered.
    pub guard: Option<&'static str>,
}

impl NpcLook {
    /// From its part on the wire, by this build's creature data; a kind this build doesn't know
    /// is drawn plain.
    pub fn of(n: &NpcNet) -> Self {
        let def = Table::SHIPPED.npc(&n.kind);
        Self {
            kind: n.kind.clone(),
            colours: def.map_or_else(Vec::new, |d| npc_colours(d, n.variant)),
            held: def.and_then(|d| d.held),
            guard: None,
        }
    }
}

/// A creature as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureView {
    pub id: u32,
    pub model: &'static str,
    pub feet: [f64; 3],
    pub yaw: f64,
    pub face: FaceFrame,
    pub role: &'static str,
    /// An NPC's looks; None: an animal, or an NPC from a server without the NPC part.
    pub npc: Option<NpcLook>,
    /// An animal's idle pose while it stands (`graze`, `lie`, ...: `CREATURE_POSES`, WORK_QUEUE
    /// item 39), from a server that sends the poses' part; None: standing, or moving.
    pub pose: Option<&'static str>,
    /// Seconds in its NPC pose so far (a defeated NPC's kneel and stand play from their start).
    pub pose_s: f64,
    /// Its hurt flash (sRGB 0..1 and the strength mixed toward it; strength 0: none), its kind's
    /// `hurtLook` after a hit that connected (WORK_QUEUE item 35, round 2).
    pub tint: [f32; 4],
}

/// The hurt look of a creature drawn with `model` (an NPC's by its `kind`): its kind's data, else
/// the first kind with that model, else the file's defaults (the first creature's).
fn hurt_look_of(model: &str, npc_kind: Option<&str>) -> dwell_sim::creatures::HurtLook {
    let t = Table::SHIPPED;
    npc_kind
        .and_then(|k| t.creatures.iter().find(|c| c.name == k))
        .or_else(|| t.creatures.iter().find(|c| c.model == model))
        .unwrap_or(&t.creatures[0])
        .hurt_look
}

#[derive(Clone, Debug)]
struct Seen {
    net: CreatureNet,
    model: &'static str,
    shown: [f64; 3],
    yaw: f64,
    /// Its NPC part and looks (computed once a variant).
    npc: Option<(NpcNet, NpcLook)>,
    /// Its idle pose's index in `CREATURE_POSES` (0 stand).
    pose: u8,
    /// Its NPC pose last drawn and the seconds since it began.
    npc_pose: (u8, f64),
    /// A hit that connected: seconds since, and the blow's way along the ground (face-local unit
    /// x, z: away from the striker).
    hurt: Option<(f64, [f64; 2])>,
}

#[derive(Debug, Default)]
pub struct ClientCreatures {
    seen: HashMap<u32, Seen>,
}

impl ClientCreatures {
    /// Applies a `Creatures` message; a model this build doesn't have is skipped.
    pub fn apply(&mut self, m: CreaturesMessage) {
        for id in m.gone {
            self.seen.remove(&id);
        }
        let npcs: HashMap<u32, NpcNet> = m.npcs.into_iter().flatten().map(|n| (n.id, n)).collect();
        // The poses' part lists the creatures not standing; one absent from it stands.
        let poses: Option<HashMap<u32, u8>> = m
            .poses
            .map(|p| p.into_iter().map(|c| (c.id, c.pose)).collect());
        for c in m.creatures {
            let Some(data) = dwell_models::find(dwell_models::MODELS, &c.model) else {
                continue;
            };
            let at = c.position.map(f64::from);
            let e = self.seen.entry(c.id).or_insert_with(|| Seen {
                net: c.clone(),
                model: data.name,
                shown: at,
                yaw: f64::from(c.yaw),
                npc: None,
                npc_pose: (0, 0.0),
                pose: 0,
                hurt: None,
            });
            if let Some(p) = &poses {
                e.pose = p.get(&c.id).copied().unwrap_or(0);
            }
            if let Some(n) = npcs.get(&c.id) {
                let same = e
                    .npc
                    .as_ref()
                    .is_some_and(|(o, _)| o.kind == n.kind && o.variant == n.variant);
                if same {
                    if let Some((o, _)) = &mut e.npc {
                        o.pose = n.pose;
                    }
                } else {
                    e.npc = Some((n.clone(), NpcLook::of(n)));
                }
            }
            e.net = c;
        }
    }

    /// Creature `id` was hit by a blow from `from` (world: where the blades met, or the striker):
    /// its kind's hurt look plays from now (WORK_QUEUE item 35, round 2). False: not seen.
    pub fn hurt(&mut self, id: u32, from: [f64; 3]) -> bool {
        let Some(s) = self.seen.get_mut(&id) else {
            return false;
        };
        let f = FaceFrame::at_height(s.shown[1]);
        let (me, them) = (f.point(s.shown), f.point(from));
        let (dx, dz) = (me[0] - them[0], me[2] - them[2]);
        let d = dx.hypot(dz);
        // From right on it: back along its heading.
        let away = if d > 1e-6 {
            [dx / d, dz / d]
        } else {
            let y = s.yaw.to_radians();
            [-y.sin(), -y.cos()]
        };
        s.hurt = Some((0.0, away));
        true
    }

    /// Moves each drawn point toward its latest by `dt` seconds and returns them.
    pub fn advance(&mut self, dt: f64) -> Vec<CreatureView> {
        let k = 1.0 - (-FOLLOW * dt.max(0.0)).exp();
        let mut out: Vec<CreatureView> = self
            .seen
            .values_mut()
            .map(|s| {
                let to = s.net.position.map(f64::from);
                // A point far off (a teleport, a first sight) is taken at once.
                let far = (0..3).map(|i| (to[i] - s.shown[i]).powi(2)).sum::<f64>() > 16.0;
                s.shown = std::array::from_fn(|i| {
                    if far {
                        to[i]
                    } else {
                        s.shown[i] + (to[i] - s.shown[i]) * k
                    }
                });
                let now = s.npc.as_ref().map_or(0, |(n, _)| n.pose);
                s.npc_pose = if now == s.npc_pose.0 {
                    (now, s.npc_pose.1 + dt.max(0.0))
                } else {
                    (now, 0.0)
                };
                let dyaw = (f64::from(s.net.yaw) - s.yaw + 540.0).rem_euclid(360.0) - 180.0;
                s.yaw += dyaw * k;
                // The hurt look: a flinch away from the blow and a fading flash, over the data's time.
                let look = hurt_look_of(s.model, s.npc.as_ref().map(|(n, _)| n.kind.as_str()));
                let (mut feet, mut yaw, mut tint) = (s.shown, s.yaw, [0.0; 4]);
                if let Some((t, away)) = &mut s.hurt {
                    *t += dt.max(0.0);
                    let (flash, flinch) = look.at(*t);
                    if flash <= 0.0 && flinch <= 0.0 {
                        s.hurt = None;
                    } else {
                        let f = FaceFrame::at_height(s.shown[1]);
                        let m = look.flinch_m * flinch;
                        let v = f.vector([away[0] * m, 0.0, away[1] * m]);
                        feet = std::array::from_fn(|i| feet[i] + v[i]);
                        yaw += look.flinch_deg * flinch;
                        let c = look.tint;
                        tint = [c[0] as f32, c[1] as f32, c[2] as f32, flash as f32];
                    }
                }
                CreatureView {
                    id: s.net.id,
                    model: s.model,
                    feet,
                    yaw,
                    face: FaceFrame::at_height(s.shown[1]),
                    role: match &s.npc {
                        Some((n, l)) => {
                            npc_swing_role(npc_role(s.net.speed, n.pose, npc_armed(l)), l.held)
                        }
                        None => role_for_speed(s.net.speed),
                    },
                    npc: s.npc.as_ref().map(|(_, l)| l.clone()),
                    pose: (s.net.speed < 0.3 && s.pose != 0)
                        .then(|| CREATURE_POSES.get(usize::from(s.pose)).copied())
                        .flatten(),
                    pose_s: s.npc_pose.1,
                    tint,
                }
            })
            .collect();
        out.sort_by_key(|v| v.id);
        out
    }

    /// Each creature's drawn feet (world m), heading (degrees) and speed (m/s), by id.
    pub fn shown(&self) -> Vec<([f64; 3], f64, f32, bool)> {
        let mut v: Vec<_> = self
            .seen
            .values()
            .map(|s| (s.net.id, s.shown, s.yaw, s.net.speed, s.npc.is_some()))
            .collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|(_, p, y, s, n)| (p, y, s, n)).collect()
    }

    /// Theater mode's subjects (THEATER_MODE.md §2): each creature's id, what it is (an NPC's
    /// kind, else its model's name), its drawn feet (world m) and whether it is an NPC, by id.
    pub fn subjects(&self) -> Vec<(u32, &str, [f64; 3], bool)> {
        let mut v: Vec<_> = self
            .seen
            .values()
            .map(|s| {
                let name = s.npc.as_ref().map_or(s.model, |(_, l)| l.kind.as_str());
                (s.net.id, name, s.shown, s.npc.is_some())
            })
            .collect();
        v.sort_by_key(|x| x.0);
        v
    }

    /// A creature's drawn feet (world m) by id, and whether it is an NPC (theater mode's targets).
    pub fn feet_of(&self, id: u32) -> Option<([f64; 3], bool)> {
        self.seen.get(&id).map(|s| (s.shown, s.npc.is_some()))
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WORK_QUEUE item 35: an NPC sent striking (`npcs.swings`) plays the rig's chop clip.
    #[test]
    fn an_npc_striking_plays_its_swing() {
        assert_eq!(npc_role(0.0, 3, true), "chop_level");
        assert_eq!(npc_role(2.0, 4, true), "chop_vertical");
        // The sword's own clips (WORK_QUEUE item 35): its `clips` name the slashes.
        assert_eq!(
            npc_swing_role(npc_role(0.0, 3, true), Some("dwell:sword")),
            "slash_level"
        );
        assert_eq!(
            npc_swing_role(npc_role(0.0, 4, true), Some("dwell:sword")),
            "slash_overhead"
        );
        assert_eq!(npc_swing_role("walk", Some("dwell:sword")), "walk");
        assert_eq!(npc_role(0.0, 2, false), "sit");
    }

    // WORK_QUEUE item 55: a defeated NPC (`humans.defeat`) kneels, its sword planted if it holds
    // one, then stands up; never a lying death.
    #[test]
    fn a_defeated_npc_kneels_then_stands() {
        assert_eq!(npc_role(0.0, 5, false), "kneel");
        assert_eq!(npc_role(0.0, 5, true), "kneel_sword");
        assert_eq!(npc_role(0.0, 6, false), "stand");
        assert_eq!(npc_role(0.0, 6, true), "stand_sword");
    }

    // WORK_QUEUE item 35, round 2: a hit creature flinches away from the blow and flashes its
    // kind's tint, by its `hurtLook` data, then is drawn as before.
    dwell_world::both_faces!(
        fn a_hit_creature_flinches_from_the_blow_and_flashes_then_settles(f: FaceFrame) {
            let mut c = ClientCreatures::default();
            let at = f.point([3.0, 20.0, -2.0]);
            c.apply(CreaturesMessage {
                creatures: vec![CreatureNet {
                    id: 4,
                    model: "wolf".into(),
                    position: at.map(|v| v as f32),
                    yaw: 90.0,
                    speed: 0.0,
                }],
                gone: vec![],
                npcs: None,
                poses: None,
            });
            let rest = c.advance(0.0)[0].clone();
            assert_eq!(rest.tint[3], 0.0, "no flash before a hit");
            let look = hurt_look_of("wolf", None);
            assert!(look.flinch_m > 0.0 && look.strength > 0.0);
            assert!(!c.hurt(99, at), "an unseen creature");
            // Struck from face-local -x of it: it flinches toward +x.
            assert!(c.hurt(4, f.point([1.5, 20.0, -2.0])));
            let v = c.advance(0.2 * look.seconds)[0].clone();
            let moved = f.point(v.feet);
            assert!(
                (moved[0] - 3.0 - look.flinch_m).abs() < 1e-6,
                "at the flinch's peak: {moved:?}"
            );
            assert!((moved[1] - 20.0).abs() < 1e-9 && (moved[2] + 2.0).abs() < 1e-9);
            assert!((v.yaw - 90.0 - look.flinch_deg).abs() < 1e-6);
            let tint = look.tint.map(|x| x as f32);
            assert_eq!(&v.tint[..3], &tint[..]);
            assert!(v.tint[3] > 0.0 && f64::from(v.tint[3]) <= look.strength);
            // Over: drawn where it stands, unflashed.
            let after = c.advance(look.seconds)[0].clone();
            assert_eq!(after.tint, [0.0; 4]);
            assert_eq!(after.feet, rest.feet);
            assert_eq!(after.yaw, rest.yaw);
        }
    );

    #[test]
    fn the_client_picks_idle_walk_or_run_from_speed() {
        assert_eq!(role_for_speed(0.0), "idle");
        assert_eq!(role_for_speed(1.3), "walk");
        assert_eq!(role_for_speed(5.0), "sprint");
    }

    dwell_world::both_faces!(
        fn a_creature_message_is_drawn_and_gone(f: FaceFrame) {
            let mut c = ClientCreatures::default();
            let at = f.point([3.0, 20.0, -2.0]).map(|v| v as f32);
            let net = |speed: f32| CreatureNet {
                id: 4,
                model: "deer".into(),
                position: at,
                yaw: 90.0,
                speed,
            };
            c.apply(CreaturesMessage {
                creatures: vec![net(1.3)],
                gone: vec![],
                npcs: None,
                poses: None,
            });
            c.apply(CreaturesMessage {
                creatures: vec![CreatureNet {
                    id: 5,
                    model: "unicorn".into(),
                    ..net(0.0)
                }],
                gone: vec![],
                npcs: None,
                poses: None,
            });
            let v = c.advance(0.016);
            assert_eq!(v.len(), 1, "a model this build lacks is skipped");
            assert_eq!((v[0].model, v[0].role, v[0].face), ("deer", "walk", f));
            c.apply(CreaturesMessage {
                creatures: vec![net(0.0)],
                gone: vec![],
                npcs: None,
                poses: None,
            });
            assert_eq!(c.advance(0.016)[0].role, "idle");
            assert_eq!(c.advance(0.016)[0].pose, None, "no poses' part: standing");
            // WORK_QUEUE item 39: the poses' part sets an animal's idle pose; absent from it, it
            // stands; moving, it shows none.
            let posed = |pose: Option<u8>, speed: f32| CreaturesMessage {
                creatures: vec![net(speed)],
                gone: vec![],
                npcs: Some(vec![]),
                poses: Some(
                    pose.map(|p| dwell_protocol::CreaturePoseNet { id: 4, pose: p })
                        .into_iter()
                        .collect(),
                ),
            };
            c.apply(posed(Some(5), 0.0));
            assert_eq!(c.advance(0.016)[0].pose, Some("lie"));
            c.apply(posed(Some(1), 1.2));
            assert_eq!(c.advance(0.016)[0].pose, None, "moving");
            c.apply(posed(Some(1), 0.0));
            assert_eq!(c.advance(0.016)[0].pose, Some("graze"));
            c.apply(posed(None, 0.0));
            assert_eq!(c.advance(0.016)[0].pose, None, "left out: standing");
            c.apply(CreaturesMessage {
                creatures: vec![],
                gone: vec![4],
                npcs: None,
                poses: None,
            });
            assert!(c.advance(0.016).is_empty());
        }
    );

    // WORK_QUEUE item 38: an NPC's part picks its role by pose and its colours by its kind's
    // data and variant seed (the same seed, the same colours); without the part it is drawn plain.
    dwell_world::both_faces!(
        fn an_npc_is_drawn_in_its_pose_and_colours(f: FaceFrame) {
            let mut c = ClientCreatures::default();
            let net = CreatureNet {
                id: 7,
                model: "peasant".into(),
                position: f.point([1.0, 12.0, 1.0]).map(|v| v as f32),
                yaw: 0.0,
                speed: 0.0,
            };
            let part = |pose: u8, variant: u32| NpcNet {
                id: 7,
                kind: "dwell:villager".into(),
                pose,
                variant,
            };
            let msg = |speed: f32, npcs: Option<Vec<NpcNet>>| CreaturesMessage {
                creatures: vec![CreatureNet {
                    speed,
                    ..net.clone()
                }],
                gone: vec![],
                npcs,
                poses: None,
            };
            c.apply(msg(0.0, None));
            let v = c.advance(0.016);
            assert_eq!((v[0].role, v[0].npc.as_ref()), ("idle", None), "plain");
            c.apply(msg(0.0, Some(vec![part(2, 99)])));
            let v = c.advance(0.016);
            assert_eq!(v[0].role, "sit");
            let look = v[0].npc.clone().expect("its looks");
            let def = Table::SHIPPED.npc("dwell:villager").unwrap();
            assert_eq!(look.colours, npc_colours(def, 99));
            // Shirt, trousers, skin, hair and hat (the hat its own since 8a5f8a1).
            assert_eq!(look.colours.len(), 5);
            c.apply(msg(1.1, Some(vec![part(2, 99)])));
            assert_eq!(
                c.advance(0.016)[0].role,
                "walk",
                "walking, whatever the pose"
            );
            c.apply(msg(0.0, Some(vec![part(1, 99)])));
            let v = c.advance(0.016);
            assert_eq!(v[0].role, "look");
            assert_eq!(
                v[0].npc.as_ref().unwrap().colours,
                look.colours,
                "same seed"
            );
            // A kind this build lacks: its pose still, no colours.
            c.apply(CreaturesMessage {
                npcs: Some(vec![NpcNet {
                    kind: "mod:knight".into(),
                    ..part(0, 1)
                }]),
                ..msg(0.0, None)
            });
            let v = c.advance(0.016);
            assert_eq!(v[0].role, "idle");
            assert!(v[0].npc.as_ref().unwrap().colours.is_empty());
        }
    );
}
