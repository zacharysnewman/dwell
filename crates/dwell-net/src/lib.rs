//! Dwell's network front-end (ADRs 0001, 0008): WebTransport and WebRTC data channels behind one
//! event queue, and the dedicated server's registration with the master server (ARCHITECTURE.md
//! §10.1). The host starts a [`NetServer`], drains [`Event`]s with [`NetServer::poll`] once per
//! tick and sends with [`NetServer::send`]. All networking runs on the crate's own tokio threads;
//! nothing calls back into the host.

mod framing;
mod master;
mod rtc;
mod server;

pub use master::{Master, Status as MasterStatus, signature_headers, signing_message};
pub use server::{
    CHANNEL_CONTROL, CHANNEL_LOD, CHANNEL_WORLD, Command, Event, Limits, NetServer, StartOptions,
    TRANSPORT_WEBTRANSPORT,
};
