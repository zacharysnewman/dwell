//! The creatures the server tells this client of (docs/adr/0073-creatures.md): each one's latest
//! point, heading and speed, drawn smoothed toward it by the skinned renderer with the clip its
//! speed picks. An NPC (WORK_QUEUE item 38) also carries its part (`NpcNet`, from a server that
//! knows this client has `creatures.npcs`): its idle pose picks its role, and its kind's data
//! (shared/creatures/creatures.json) and palette variant seed pick its colours and held item.

use std::collections::HashMap;

use dwell_protocol::{CreatureNet, CreaturesMessage, NPC_POSES, NpcNet};
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
pub fn npc_role(speed: f32, pose: u8) -> &'static str {
    match (role_for_speed(speed), NPC_POSES.get(usize::from(pose))) {
        ("idle", Some(&"look")) => "look",
        ("idle", Some(&"sit")) => "sit",
        (r, _) => r,
    }
}

/// An NPC's looks as drawn: its colours (the model's palette ramp → the one drawn in its place)
/// and the item in its hand.
#[derive(Clone, Debug, PartialEq)]
pub struct NpcLook {
    pub kind: String,
    pub colours: Vec<(&'static str, &'static str)>,
    pub held: Option<&'static str>,
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
}

#[derive(Clone, Debug)]
struct Seen {
    net: CreatureNet,
    model: &'static str,
    shown: [f64; 3],
    yaw: f64,
    /// Its NPC part and looks (computed once a variant).
    npc: Option<(NpcNet, NpcLook)>,
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
            });
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
                let dyaw = (f64::from(s.net.yaw) - s.yaw + 540.0).rem_euclid(360.0) - 180.0;
                s.yaw += dyaw * k;
                CreatureView {
                    id: s.net.id,
                    model: s.model,
                    feet: s.shown,
                    yaw: s.yaw,
                    face: FaceFrame::at_height(s.shown[1]),
                    role: match &s.npc {
                        Some((n, _)) => npc_role(s.net.speed, n.pose),
                        None => role_for_speed(s.net.speed),
                    },
                    npc: s.npc.as_ref().map(|(_, l)| l.clone()),
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
            });
            c.apply(CreaturesMessage {
                creatures: vec![CreatureNet {
                    id: 5,
                    model: "unicorn".into(),
                    ..net(0.0)
                }],
                gone: vec![],
                npcs: None,
            });
            let v = c.advance(0.016);
            assert_eq!(v.len(), 1, "a model this build lacks is skipped");
            assert_eq!((v[0].model, v[0].role, v[0].face), ("deer", "walk", f));
            c.apply(CreaturesMessage {
                creatures: vec![net(0.0)],
                gone: vec![],
                npcs: None,
            });
            assert_eq!(c.advance(0.016)[0].role, "idle");
            c.apply(CreaturesMessage {
                creatures: vec![],
                gone: vec![4],
                npcs: None,
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
            assert_eq!(look.colours.len(), 4);
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
