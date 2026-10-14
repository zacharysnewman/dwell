//! Network condition simulator (tooling): latency, jitter and datagram loss in both directions of
//! a transport. Enabled with `?netsim=<rtt ms>,<jitter ms>,<loss %>`. Packets wait in a queue the
//! frame loop drains with `due`.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetConditions {
    /// Round trip added; each direction gets half.
    pub rtt_ms: f64,
    /// Uniform ± jitter per packet, ms.
    pub jitter_ms: f64,
    /// Datagram loss probability, 0..1 (reliable messages are only delayed, in order).
    pub loss: f64,
}

/// Parses `rtt,jitter,loss%` (e.g. `150,20,5`); `None` if absent or malformed.
pub fn parse_net_conditions(value: Option<&str>) -> Option<NetConditions> {
    let value = value.filter(|v| !v.is_empty())?;
    let mut parts = value.split(',');
    let mut next = |default: &str| {
        parts
            .next()
            .unwrap_or(default)
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
    };
    let (rtt, jitter, loss) = (next("x")?, next("0")?, next("0")?);
    Some(NetConditions {
        rtt_ms: rtt,
        jitter_ms: jitter,
        loss: (loss / 100.0).min(1.0),
    })
}

/// One direction of a simulated link.
pub struct Direction<T> {
    c: NetConditions,
    last_reliable_at: f64,
    order: u64,
    queue: Vec<(f64, u64, T)>,
}

impl<T> Direction<T> {
    pub fn new(c: NetConditions) -> Self {
        Self {
            c,
            last_reliable_at: 0.0,
            order: 0,
            queue: Vec::new(),
        }
    }
    /// Sends `item` at `now`; `random` gives uniform values in [0, 1).
    pub fn send(&mut self, now: f64, reliable: bool, item: T, random: &mut dyn FnMut() -> f64) {
        if !reliable && random() < self.c.loss {
            return;
        }
        let delay = (self.c.rtt_ms / 2.0 + (random() * 2.0 - 1.0) * self.c.jitter_ms).max(0.0);
        let mut at = now + delay;
        if reliable {
            at = at.max(self.last_reliable_at); // stay in order
            self.last_reliable_at = at;
        }
        self.queue.push((at, self.order, item));
        self.order += 1;
    }
    /// Items due by `now`, in delivery order.
    pub fn due(&mut self, now: f64) -> Vec<T> {
        let (mut due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue)
            .into_iter()
            .partition(|(at, _, _)| *at <= now);
        self.queue = rest;
        due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        due.into_iter().map(|(_, _, t)| t).collect()
    }
    pub fn pending(&self) -> impl Iterator<Item = f64> + '_ {
        self.queue.iter().map(|q| q.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rtt_jitter_loss_percent() {
        assert_eq!(
            parse_net_conditions(Some("150,20,5")),
            Some(NetConditions {
                rtt_ms: 150.0,
                jitter_ms: 20.0,
                loss: 0.05
            })
        );
        assert_eq!(
            parse_net_conditions(Some("80")),
            Some(NetConditions {
                rtt_ms: 80.0,
                jitter_ms: 0.0,
                loss: 0.0
            })
        );
        assert_eq!(parse_net_conditions(Some("x")), None);
        assert_eq!(parse_net_conditions(None), None);
    }

    #[test]
    fn delays_by_half_the_rtt_drops_datagrams_keeps_reliable_order() {
        let c = NetConditions {
            rtt_ms: 100.0,
            jitter_ms: 10.0,
            loss: 0.05,
        };
        let mut randoms = vec![0.5, 0.01, 0.5, 0.99, 0.5].into_iter(); // 0.01 < loss: dropped
        let mut random = move || randoms.next().unwrap_or(0.5);
        let (mut up, mut down) = (Direction::new(c), Direction::new(c));
        up.send(0.0, false, "d1", &mut random); // 0.5: not lost; then 0.01 for jitter
        up.send(0.0, false, "d2", &mut random); // 0.5: not lost…
        down.send(0.0, true, "r3", &mut random);
        assert_eq!(up.pending().count() + down.pending().count(), 3);
        assert!(
            up.pending()
                .chain(down.pending())
                .all(|at| (40.0..=60.0).contains(&at))
        );
        assert!(up.due(0.0).is_empty());
        assert_eq!(up.due(1000.0), vec!["d1", "d2"]);
        assert_eq!(down.due(1000.0), vec!["r3"]);
    }
}
