//! WebTransport server: one tokio task per session, events to the host through a queue.
//!
//! The simulation never runs on these threads (ADR 0001): the host drains `Event`s once per tick
//! and sends `Command`s back, which are routed to the owning session task.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use wtransport::endpoint::IncomingSession;
use wtransport::{Connection, Endpoint, Identity, SendStream, ServerConfig, VarInt};

use crate::framing::{read_frame, write_frame};

pub const CHANNEL_CONTROL: u8 = 0;
pub const CHANNEL_WORLD: u8 = 1;
pub const TRANSPORT_WEBTRANSPORT: u8 = 1;

#[derive(Clone, Copy)]
pub struct Limits {
    pub max_reliable_message_bytes: u32,
    pub max_datagram_bytes: u32,
}

pub enum Event {
    Connected {
        session: u32,
        transport: u8,
        binding: [u8; 32],
    },
    Disconnected {
        session: u32,
    },
    Reliable {
        session: u32,
        channel: u8,
        data: Vec<u8>,
    },
    Datagram {
        session: u32,
        data: Vec<u8>,
    },
}

pub enum Command {
    Reliable { channel: u8, data: Vec<u8> },
    Datagram(Vec<u8>),
    Close,
}

type Sessions = Arc<Mutex<HashMap<u32, mpsc::UnboundedSender<Command>>>>;

pub struct NetServer {
    runtime: tokio::runtime::Runtime,
    events: std_mpsc::Receiver<Event>,
    sessions: Sessions,
    cert_hash: [u8; 32],
    port: u16,
}

impl NetServer {
    pub fn start(port: u16, limits: Limits) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("dwell-net")
            .enable_all()
            .build()
            .map_err(|e| format!("tokio runtime: {e}"))?;

        // WebTransport's serverCertificateHashes needs ECDSA P-256 valid for at most 14 days.
        let identity = Identity::self_signed_builder()
            .subject_alt_names(["localhost", "127.0.0.1", "::1"])
            .from_now_utc()
            .validity_days(13)
            .build()
            .map_err(|e| format!("certificate: {e}"))?;
        let mut cert_hash = [0u8; 32];
        cert_hash.copy_from_slice(identity.certificate_chain().as_slice()[0].hash().as_ref());

        let socket = bind_udp(port)?;
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();
        let config = ServerConfig::builder()
            .with_bind_socket(socket)
            .with_identity(identity)
            .keep_alive_interval(Some(Duration::from_secs(3)))
            .max_idle_timeout(Some(Duration::from_secs(15)))
            .map_err(|e| format!("idle timeout: {e}"))?
            .build();

        let (event_tx, events) = std_mpsc::channel();
        let sessions: Sessions = Arc::default();
        let endpoint = {
            let _guard = runtime.enter();
            Endpoint::server(config).map_err(|e| format!("endpoint: {e}"))?
        };
        runtime.spawn(accept_loop(
            endpoint,
            event_tx,
            sessions.clone(),
            cert_hash,
            limits,
        ));

        Ok(Self {
            runtime,
            events,
            sessions,
            cert_hash,
            port,
        })
    }

    pub fn cert_hash(&self) -> [u8; 32] {
        self.cert_hash
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn poll(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn send(&self, session: u32, command: Command) -> bool {
        let sessions = self.sessions.lock().expect("sessions lock");
        sessions
            .get(&session)
            .is_some_and(|tx| tx.send(command).is_ok())
    }

    pub fn shutdown(self) {
        self.runtime.shutdown_timeout(Duration::from_secs(1));
    }
}

/// Dual-stack `[::]:port` where IPv6 is available, else IPv4 only.
fn bind_udp(port: u16) -> Result<UdpSocket, String> {
    UdpSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)))
        .or_else(|_| UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))))
        .map_err(|e| format!("bind UDP port {port}: {e}"))
}

async fn accept_loop(
    endpoint: Endpoint<wtransport::endpoint::endpoint_side::Server>,
    events: std_mpsc::Sender<Event>,
    sessions: Sessions,
    binding: [u8; 32],
    limits: Limits,
) {
    static NEXT_SESSION: AtomicU32 = AtomicU32::new(1);
    loop {
        let incoming = endpoint.accept().await;
        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        let events = events.clone();
        let sessions = sessions.clone();
        tokio::spawn(async move {
            run_session(incoming, session, &events, &sessions, binding, limits).await;
            sessions.lock().expect("sessions lock").remove(&session);
        });
    }
}

async fn run_session(
    incoming: IncomingSession,
    session: u32,
    events: &std_mpsc::Sender<Event>,
    sessions: &Sessions,
    binding: [u8; 32],
    limits: Limits,
) {
    let Ok(request) = incoming.await else { return };
    let Ok(conn) = request.accept().await else {
        return;
    };

    let (cmd_tx, mut commands) = mpsc::unbounded_channel();
    sessions
        .lock()
        .expect("sessions lock")
        .insert(session, cmd_tx);
    if events
        .send(Event::Connected {
            session,
            transport: TRANSPORT_WEBTRANSPORT,
            binding,
        })
        .is_err()
    {
        return;
    }

    let mut control_tx: Option<SendStream> = None;
    let mut world_tx: Option<SendStream> = None;
    // Control replies can only be written once the client has opened the control stream.
    let mut pending_control: Vec<Vec<u8>> = Vec::new();
    let (inbound_tx, mut inbound) = mpsc::unbounded_channel::<Result<Vec<u8>, ()>>();

    loop {
        tokio::select! {
            stream = conn.accept_bi(), if control_tx.is_none() => {
                let Ok((mut tx, mut rx)) = stream else { break };
                let mut channel = [0u8; 1];
                if rx.read_exact(&mut channel).await.is_err() || channel[0] != CHANNEL_CONTROL {
                    break;
                }
                for msg in pending_control.drain(..) {
                    if write_frame(&mut tx, &msg).await.is_err() { break; }
                }
                control_tx = Some(tx);
                let inbound_tx = inbound_tx.clone();
                let max = limits.max_reliable_message_bytes;
                tokio::spawn(async move {
                    loop {
                        match read_frame(&mut rx, max).await {
                            Ok(Some(msg)) => { if inbound_tx.send(Ok(msg)).is_err() { break; } }
                            Ok(None) | Err(_) => { let _ = inbound_tx.send(Err(())); break; }
                        }
                    }
                });
            }
            msg = inbound.recv() => {
                match msg {
                    Some(Ok(data)) => {
                        if events.send(Event::Reliable { session, channel: CHANNEL_CONTROL, data }).is_err() { break; }
                    }
                    _ => break,
                }
            }
            dgram = conn.receive_datagram() => {
                let Ok(dgram) = dgram else { break };
                if dgram.payload().len() <= limits.max_datagram_bytes as usize {
                    let data = dgram.payload().to_vec();
                    if events.send(Event::Datagram { session, data }).is_err() { break; }
                }
            }
            command = commands.recv() => {
                match command {
                    Some(Command::Reliable { channel: CHANNEL_CONTROL, data }) => match control_tx.as_mut() {
                        Some(tx) => { if write_frame(tx, &data).await.is_err() { break; } }
                        None => pending_control.push(data),
                    },
                    Some(Command::Reliable { channel: _, data }) => {
                        if world_tx.is_none() {
                            world_tx = open_world_stream(&conn).await;
                        }
                        match world_tx.as_mut() {
                            Some(tx) => { if write_frame(tx, &data).await.is_err() { break; } }
                            None => break,
                        }
                    }
                    Some(Command::Datagram(data)) => {
                        // Datagrams are best-effort; oversize or congested sends are dropped.
                        let _ = conn.send_datagram(data);
                    }
                    Some(Command::Close) | None => {
                        // Finish the control stream and give the client a moment to read it, so a
                        // final Reject arrives before the connection closes.
                        if let Some(tx) = control_tx.as_mut() {
                            let _ = tx.finish().await;
                            let _ = tokio::time::timeout(Duration::from_secs(1), tx.stopped()).await;
                        }
                        conn.close(VarInt::from_u32(0), b"closed by server");
                        break;
                    }
                }
            }
        }
    }
    let _ = events.send(Event::Disconnected { session });
}

async fn open_world_stream(conn: &Connection) -> Option<SendStream> {
    let mut tx = conn.open_uni().await.ok()?.await.ok()?;
    tx.write_all(&[CHANNEL_WORLD]).await.ok()?;
    Some(tx)
}
