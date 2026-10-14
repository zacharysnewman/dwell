//! The client's networking (ARCHITECTURE.md §8): the session, framing, the datagram sender, the
//! WebRTC answer and the link simulator are pure and tested natively; the browser transports
//! (WebTransport, WebRTC data channels, the local worker) are in `web`.

pub mod datagram;
pub mod framing;
pub mod netsim;
pub mod sdp;
pub mod session;
