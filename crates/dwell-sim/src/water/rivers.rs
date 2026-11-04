//! Changes carried down the world map's rivers (docs/WATER_FLOW.md §6, W2d): what a change to the
//! water sends down a river (a dam holding it back, a lake let go) reaches the river far below
//! through country nobody has loaded. The difference from the river's flow travels the map's river
//! path at `riverSpeed` as a **front**; where the river joins another the front goes on along it;
//! into a lake, the lake's outflow follows it by the time the lake takes to rise or fall to it
//! (its area over how fast its outflow grows with its level); into the sea it ends. The first body
//! a front reaches on its river (the head of the bodies found there) takes it in as more or less
//! coming in, and passes the change on itself: the front stops there.

use super::*;
use dwell_world::Face;
pub use dwell_worldgen::terrain::RiverNext;

/// The world map's rivers, as water needs them: where a column lies on one, how long each is and
/// where it goes (the server's: `dwell_worldgen`'s terrain; a test's: its own).
pub trait Rivers: Send + Sync {
    /// The river at face-local column (x, z) and how far along it (m from its source).
    fn at(&self, face: Face, x: i32, z: i32) -> Option<(u32, f64)>;
    /// Its length (m).
    fn length(&self, face: Face, river: u32) -> f64;
    /// Where it goes at its end.
    fn next(&self, face: Face, river: u32) -> RiverNext;
}

impl Rivers for dwell_worldgen::BifacialTerrain {
    fn at(&self, face: Face, x: i32, z: i32) -> Option<(u32, f64)> {
        self.local(face).river_at(x, z)
    }
    fn length(&self, face: Face, river: u32) -> f64 {
        self.local(face).river_length(river)
    }
    fn next(&self, face: Face, river: u32) -> RiverNext {
        self.local(face).river_next(river)
    }
}

/// A change to a river's flow travelling down it: `dq` (m³/s) from `from` (m along it) to `at`.
/// Once at its end (`done`) it holds the stretch behind it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Front {
    pub face: Face,
    pub river: u32,
    pub from: f64,
    pub at: f64,
    pub dq: f64,
    pub done: bool,
}

/// A lake on the map a front ran into: its outflow follows what came in.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Lagged {
    pub face: Face,
    pub lake: u32,
    pub area: f64,
    /// The rivers out of it and each one's discharge.
    pub out: Vec<(u32, f64)>,
    /// What came in (the fronts' sum) and its outflow's change now, and what was sent on.
    pub inn: f64,
    pub now: f64,
    pub sent: f64,
}

/// What a body sends down the map's river, as last sent (its river, where, the change).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sent {
    pub face: Face,
    pub river: u32,
    pub s: f64,
    pub dq: f64,
}

/// The fronts on the map's rivers and the lakes they ran into.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Flows {
    pub fronts: Vec<Front>,
    pub lakes: Vec<Lagged>,
    /// Per body, what it sent last.
    pub sent: HashMap<u32, Sent>,
    /// Per body, how long (water s) a change worth sending has lasted (not sent yet).
    pub held: HashMap<u32, f64>,
}

impl Flows {
    /// The change in a river's flow at `s` along it: the fronts from above `s` that reached it.
    pub fn offset(&self, face: Face, river: u32, s: f64) -> f64 {
        self.fronts
            .iter()
            .filter(|f| f.face == face && f.river == river && f.from < s && s <= f.at)
            .map(|f| f.dq)
            .sum()
    }

    /// A change of `dq` sent down `river` from `s`.
    pub fn send(&mut self, face: Face, river: u32, s: f64, dq: f64) {
        if dq != 0.0 {
            self.fronts.push(Front {
                face,
                river,
                from: s,
                at: s,
                dq,
                done: false,
            });
        }
    }

    /// `dt` s on: the fronts go `speed × dt` down their rivers, no farther than the first body
    /// taking them in (`heads`: per river, where bodies found there take in its flow); at a
    /// river's end on along the next one, into a lake its outflow following them.
    pub fn advance(
        &mut self,
        rivers: &dyn Rivers,
        heads: &HashMap<(Face, u32), Vec<f64>>,
        speed: f64,
        dt: f64,
    ) {
        let mut on: Vec<(Face, u32, f64, f64, f64)> = Vec::new();
        let mut into: Vec<(Face, RiverNext, f64)> = Vec::new();
        for f in self.fronts.iter_mut().filter(|f| !f.done) {
            let len = rivers.length(f.face, f.river);
            let stop = heads
                .get(&(f.face, f.river))
                .and_then(|v| v.iter().copied().filter(|&h| h > f.from).reduce(f64::min));
            let reach = f.at + speed * dt;
            if let Some(h) = stop
                && reach >= h
            {
                // Taken in by the body there, which passes the change on itself.
                f.at = h;
                f.done = true;
                continue;
            }
            if reach >= len {
                f.at = len;
                f.done = true;
                into.push((f.face, rivers.next(f.face, f.river), f.dq));
                // How far past its end it got, on the next.
                on.push((f.face, f.river, reach - len, f.dq, 0.0));
            } else {
                f.at = reach;
            }
        }
        for ((face, next, dq), (_, _, past, _, _)) in into.into_iter().zip(on) {
            match next {
                RiverNext::End => {}
                RiverNext::River(j, s) => self.fronts.push(Front {
                    face,
                    river: j,
                    from: s,
                    at: s + past.min(rivers.length(face, j) - s).max(0.0),
                    dq,
                    done: false,
                }),
                RiverNext::Lake { lake, area, out } => {
                    match self
                        .lakes
                        .iter_mut()
                        .find(|l| l.face == face && l.lake == lake)
                    {
                        Some(l) => l.inn += dq,
                        None => self.lakes.push(Lagged {
                            face,
                            lake,
                            area,
                            out,
                            inn: dq,
                            now: 0.0,
                            sent: 0.0,
                        }),
                    }
                }
            }
        }
        // The lakes' outflow follows what comes in: by the time it takes to change their level
        // that much (area over how fast the outflow grows with the level: 1.5 × it over the head
        // over the outlet, `lakeOutletHead`).
        let mut sends: Vec<(Face, u32, f64)> = Vec::new();
        for l in &mut self.lakes {
            let q: f64 = l.out.iter().map(|o| o.1).sum();
            let grow = 1.5 * q.max(1e-6) / FLOW.lake_outlet_head;
            let tau = l.area / grow;
            l.now += (l.inn - l.now) * (1.0 - (-dt / tau.max(1e-6)).exp());
            if (l.now - l.sent).abs() >= FLOW.river_change_min || (l.inn - l.now).abs() < 1e-9 {
                let d = l.now - l.sent;
                if d != 0.0 {
                    for &(r, qr) in &l.out {
                        let share = if q > 0.0 {
                            qr / q
                        } else {
                            1.0 / l.out.len() as f64
                        };
                        sends.push((l.face, r, d * share));
                    }
                    l.sent = l.now;
                }
            }
        }
        for (face, r, d) in sends {
            self.send(face, r, 0.0, d);
        }
        // Fronts at their ends from the same place hold the same stretch: one; none for nothing.
        let mut kept: Vec<Front> = Vec::new();
        for f in self.fronts.drain(..) {
            if f.done
                && let Some(k) = kept.iter_mut().find(|k| {
                    k.done
                        && k.face == f.face
                        && k.river == f.river
                        && k.from == f.from
                        && k.at == f.at
                })
            {
                k.dq += f.dq;
                continue;
            }
            kept.push(f);
        }
        kept.retain(|f| f.dq.abs() > 1e-9);
        self.fronts = kept;
    }
}
