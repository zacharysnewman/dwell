//! The client's side of the simulation viewpoint (docs/THEATER_MODE.md §7, T-4): while a camera
//! path plays, its eye is sent to the server at most once an interval (and only when it moved),
//! the last one always going out; when the path ends, one empty viewpoint clears it. The server
//! uses it only where the world's mode has the `theaterViewpoint` ability; the client sends it
//! only to a server with the `theater.viewpoint` capability, so an older server never sees one.

/// Smaller moves than this (metres) aren't worth a message.
pub const MIN_MOVE_M: f64 = 0.5;

/// What to send for the viewpoint, frame by frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sender {
    /// The last viewpoint sent (none: nothing sent, or a clear).
    sent: Option<[f64; 3]>,
    /// When it was sent (ms).
    sent_ms: Option<f64>,
}

impl Sender {
    /// The frame at `now_ms` with the camera's eye at `eye` (none: no path playing, or the
    /// viewpoint is off): `Some(message)` to send — `Some(Some(at))` a viewpoint, `Some(None)` a
    /// clear — or `None`. At most one viewpoint per `interval_ms`.
    pub fn frame(
        &mut self,
        now_ms: f64,
        eye: Option<[f64; 3]>,
        interval_ms: f64,
    ) -> Option<Option<[f64; 3]>> {
        match eye {
            None => {
                // One clear after the last viewpoint, then nothing.
                self.sent.take()?;
                self.sent_ms = None;
                Some(None)
            }
            Some(at) => {
                let due = self.sent_ms.is_none_or(|t| now_ms - t >= interval_ms);
                let moved = self.sent.is_none_or(|s| {
                    (0..3).map(|k| (at[k] - s[k]).powi(2)).sum::<f64>() >= MIN_MOVE_M * MIN_MOVE_M
                });
                if !due || !moved {
                    return None;
                }
                self.sent = Some(at);
                self.sent_ms = Some(now_ms);
                Some(Some(at))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_playing_path_is_sent_at_the_rate_and_cleared_once_when_it_ends() {
        let mut s = Sender::default();
        assert_eq!(s.frame(0.0, None, 250.0), None, "nothing to clear");
        assert_eq!(s.frame(0.0, Some([1.0; 3]), 250.0), Some(Some([1.0; 3])));
        let mut sent = 0;
        for i in 1..60 {
            // 60 frames a second for a second, moving 1 m a frame.
            let t = f64::from(i) * 1000.0 / 60.0;
            if s.frame(t, Some([1.0 + f64::from(i), 1.0, 1.0]), 250.0)
                .is_some()
            {
                sent += 1;
            }
        }
        assert_eq!(sent, 3, "four a second, the first included");
        assert_eq!(
            s.frame(2000.0, None, 250.0),
            Some(None),
            "the end clears it"
        );
        assert_eq!(s.frame(2016.0, None, 250.0), None, "once");
    }

    #[test]
    fn a_camera_that_holds_still_sends_once() {
        let mut s = Sender::default();
        assert!(s.frame(0.0, Some([5.0, 6.0, 7.0]), 250.0).is_some());
        assert_eq!(s.frame(1000.0, Some([5.1, 6.0, 7.0]), 250.0), None);
        assert!(s.frame(2000.0, Some([6.0, 6.0, 7.0]), 250.0).is_some());
    }
}
