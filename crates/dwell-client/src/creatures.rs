//! The creatures the server tells this client of (docs/adr/0073-creatures.md): each one's latest
//! point, heading and speed, drawn smoothed toward it by the skinned renderer with the clip its
//! speed picks.

use std::collections::HashMap;

use dwell_protocol::{CreatureNet, CreaturesMessage};
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

/// A creature as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatureView {
    pub id: u32,
    pub model: &'static str,
    pub feet: [f64; 3],
    pub yaw: f64,
    pub face: FaceFrame,
    pub role: &'static str,
}

#[derive(Clone, Debug)]
struct Seen {
    net: CreatureNet,
    model: &'static str,
    shown: [f64; 3],
    yaw: f64,
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
            });
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
                for i in 0..3 {
                    s.shown[i] = if far {
                        to[i]
                    } else {
                        s.shown[i] + (to[i] - s.shown[i]) * k
                    };
                }
                let dyaw = (f64::from(s.net.yaw) - s.yaw + 540.0).rem_euclid(360.0) - 180.0;
                s.yaw += dyaw * k;
                CreatureView {
                    id: s.net.id,
                    model: s.model,
                    feet: s.shown,
                    yaw: s.yaw,
                    face: FaceFrame::at_height(s.shown[1]),
                    role: role_for_speed(s.net.speed),
                }
            })
            .collect();
        out.sort_by_key(|v| v.id);
        out
    }

    /// Each creature's drawn feet (world m), heading (degrees) and speed (m/s), by id.
    pub fn shown(&self) -> Vec<([f64; 3], f64, f32)> {
        let mut v: Vec<_> = self
            .seen
            .values()
            .map(|s| (s.net.id, s.shown, s.yaw, s.net.speed))
            .collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|(_, p, y, s)| (p, y, s)).collect()
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
            });
            c.apply(CreaturesMessage {
                creatures: vec![CreatureNet {
                    id: 5,
                    model: "unicorn".into(),
                    ..net(0.0)
                }],
                gone: vec![],
            });
            let v = c.advance(0.016);
            assert_eq!(v.len(), 1, "a model this build lacks is skipped");
            assert_eq!((v[0].model, v[0].role, v[0].face), ("deer", "walk", f));
            c.apply(CreaturesMessage {
                creatures: vec![net(0.0)],
                gone: vec![],
            });
            assert_eq!(c.advance(0.016)[0].role, "idle");
            c.apply(CreaturesMessage {
                creatures: vec![],
                gone: vec![4],
            });
            assert!(c.advance(0.016).is_empty());
        }
    );
}
