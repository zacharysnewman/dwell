//! Unreliable datagrams through a WritableStream (WebTransport). A stream hands its sink one chunk
//! at a time, and each hand-off needs a turn of the main thread, so when frames run long queued
//! writes drain at about one per frame and the latency of every input grows without bound.
//! Datagrams are unreliable and newer ones supersede older ones of the same kind (each PlayerInput
//! repeats the latest inputs), so only one write is in flight and, per message type, only the
//! newest datagram waits behind it.

#[derive(Default)]
pub struct DatagramSender {
    in_flight: bool,
    /// Newest waiting datagram per message type (first byte), in first-queued order.
    waiting: Vec<(u8, Vec<u8>)>,
}

impl DatagramSender {
    /// Queues a datagram; returns it when it should be written now.
    pub fn send(&mut self, bytes: Vec<u8>) -> Option<Vec<u8>> {
        if self.in_flight {
            let kind = bytes.first().copied().unwrap_or(0);
            match self.waiting.iter_mut().find(|(k, _)| *k == kind) {
                Some(w) => w.1 = bytes,
                None => self.waiting.push((kind, bytes)),
            }
            return None;
        }
        self.in_flight = true;
        Some(bytes)
    }

    /// The write in flight finished (or failed): returns the next datagram to write, if any.
    pub fn completed(&mut self) -> Option<Vec<u8>> {
        self.in_flight = false;
        if self.waiting.is_empty() {
            return None;
        }
        self.in_flight = true;
        Some(self.waiting.remove(0).1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_queues_behind_a_slow_writer_only_the_newest_of_each_type_waits() {
        let mut s = DatagramSender::default();
        // One slow frame's worth of input datagrams (type 1) and a ping (type 2).
        assert_eq!(s.send(vec![1, 10]), Some(vec![1, 10])); // one write in flight
        assert_eq!(s.send(vec![1, 11]), None);
        assert_eq!(s.send(vec![2, 50]), None);
        assert_eq!(s.send(vec![1, 12]), None);
        assert_eq!(s.send(vec![1, 13]), None);
        assert_eq!(s.completed(), Some(vec![1, 13]));
        assert_eq!(s.completed(), Some(vec![2, 50]));
        assert_eq!(s.completed(), None);
        // Idle again: the next datagram goes straight out.
        assert_eq!(s.send(vec![1, 14]), Some(vec![1, 14]));
    }

    #[test]
    fn keeps_sending_after_a_failed_write() {
        let mut s = DatagramSender::default();
        assert!(s.send(vec![1]).is_some());
        assert_eq!(s.completed(), None); // the write failed: nothing waiting
        assert!(s.send(vec![1]).is_some());
    }
}
