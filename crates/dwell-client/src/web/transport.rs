//! Browser transports (ARCHITECTURE.md §8.1): WebTransport to dedicated servers (ADR 0001) and
//! WebRTC data channels to their ICE-lite endpoint (ADR 0008). Both deliver into one event queue
//! the frame loop drains, so the session and the game stay poll-driven like the server.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_protocol::{Channel, MAX_RELIABLE_MESSAGE_BYTES};
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    ReadableStream, ReadableStreamDefaultReader, RtcDataChannel, RtcDataChannelInit,
    RtcDataChannelState, RtcDataChannelType, RtcPeerConnection, RtcSdpType,
    RtcSessionDescriptionInit, WebTransport, WebTransportCongestionControl, WebTransportHash,
    WebTransportOptions, WritableStreamDefaultWriter,
};

use crate::net::datagram::DatagramSender;
use crate::net::framing::{FrameReader, frame};
use crate::net::sdp::{WebRtcInvite, build_server_answer};

/// What a transport delivers.
#[derive(Debug)]
pub enum TransportEvent {
    Reliable(Channel, Vec<u8>),
    Datagram(Vec<u8>),
    Closed(String),
}

type Events = Rc<RefCell<VecDeque<TransportEvent>>>;

fn err_text(e: &JsValue) -> String {
    e.as_string()
        .or_else(|| {
            e.dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
        })
        .unwrap_or_else(|| "connection failed".into())
}

/// Reads a byte stream to its end, calling `f` with each chunk.
async fn read_stream(stream: ReadableStream, mut f: impl FnMut(Vec<u8>) -> bool) {
    let reader: ReadableStreamDefaultReader = stream.get_reader().unchecked_into();
    loop {
        let Ok(result) = JsFuture::from(reader.read()).await else {
            return;
        };
        if Reflect::get(&result, &"done".into())
            .ok()
            .and_then(|d| d.as_bool())
            .unwrap_or(true)
        {
            return;
        }
        let Ok(value) = Reflect::get(&result, &"value".into()) else {
            return;
        };
        if !f(Uint8Array::new(&value).to_vec()) {
            return;
        }
    }
}

/// A WebTransport connection: `control` is a client-opened bidirectional stream; server-opened
/// unidirectional streams carry `world` and `lod`; datagrams carry unreliable traffic.
pub struct WebTransportConn {
    wt: WebTransport,
    control: WritableStreamDefaultWriter,
    datagram_writer: WritableStreamDefaultWriter,
    datagrams: Rc<RefCell<DatagramSender>>,
    events: Events,
    closed: Rc<RefCell<bool>>,
    pub binding: [u8; 32],
}

impl WebTransportConn {
    /// Connects using a pinned certificate hash (the server's self-signed certificate, §2.3).
    pub async fn connect(url: &str, cert_hash: [u8; 32]) -> Result<Self, String> {
        let hash = WebTransportHash::new();
        hash.set_algorithm("sha-256");
        hash.set_value_u8_array(&Uint8Array::from(&cert_hash[..]));
        let options = WebTransportOptions::new();
        options.set_server_certificate_hashes(&[hash]);
        options.set_congestion_control(WebTransportCongestionControl::LowLatency);
        let wt = WebTransport::new_with_options(url, &options).map_err(|e| err_text(&e))?;
        JsFuture::from(wt.ready()).await.map_err(|e| err_text(&e))?;
        let bidi: web_sys::WebTransportBidirectionalStream =
            JsFuture::from(wt.create_bidirectional_stream())
                .await
                .map_err(|e| err_text(&e))?
                .unchecked_into();
        let control: WritableStreamDefaultWriter =
            bidi.writable().get_writer().map_err(|e| err_text(&e))?;
        let _ = JsFuture::from(
            control.write_with_chunk(&Uint8Array::from(&[Channel::Control as u8][..])),
        )
        .await;
        let datagram_writer = wt
            .datagrams()
            .writable()
            .get_writer()
            .map_err(|e| err_text(&e))?;
        let conn = Self {
            wt,
            control,
            datagram_writer,
            datagrams: Rc::default(),
            events: Rc::default(),
            closed: Rc::default(),
            binding: cert_hash,
        };
        conn.run(bidi.readable().unchecked_into());
        Ok(conn)
    }

    fn run(&self, control: ReadableStream) {
        let events = self.events.clone();
        spawn_local(async move {
            let mut frames = FrameReader::new(MAX_RELIABLE_MESSAGE_BYTES);
            read_stream(control, |chunk| match frames.push(&chunk) {
                Ok(fs) => {
                    events.borrow_mut().extend(
                        fs.into_iter()
                            .map(|f| TransportEvent::Reliable(Channel::Control, f)),
                    );
                    true
                }
                Err(_) => false,
            })
            .await;
        });
        // Server-opened streams start with their channel id.
        let (events, wt) = (self.events.clone(), self.wt.clone());
        spawn_local(async move {
            let streams: ReadableStreamDefaultReader = wt
                .incoming_unidirectional_streams()
                .get_reader()
                .unchecked_into();
            loop {
                let Ok(result) = JsFuture::from(streams.read()).await else {
                    return;
                };
                if Reflect::get(&result, &"done".into())
                    .ok()
                    .and_then(|d| d.as_bool())
                    .unwrap_or(true)
                {
                    return;
                }
                let Ok(stream) = Reflect::get(&result, &"value".into()) else {
                    return;
                };
                let events = events.clone();
                spawn_local(async move {
                    let mut frames = FrameReader::new(MAX_RELIABLE_MESSAGE_BYTES);
                    let mut channel: Option<Channel> = None;
                    read_stream(stream.unchecked_into(), |chunk| {
                        let body = match channel {
                            Some(_) => &chunk[..],
                            None if chunk.is_empty() => return true,
                            None => {
                                match Channel::from_u8(chunk[0]).filter(|c| *c != Channel::Control)
                                {
                                    Some(c) => channel = Some(c),
                                    None => {
                                        events.borrow_mut().push_back(TransportEvent::Closed(
                                            "unexpected server stream".into(),
                                        ));
                                        return false;
                                    }
                                }
                                &chunk[1..]
                            }
                        };
                        let c = channel.expect("tagged");
                        match frames.push(body) {
                            Ok(fs) => {
                                events
                                    .borrow_mut()
                                    .extend(fs.into_iter().map(|f| TransportEvent::Reliable(c, f)));
                                true
                            }
                            Err(_) => false,
                        }
                    })
                    .await;
                });
            }
        });
        let (events, wt) = (self.events.clone(), self.wt.clone());
        spawn_local(async move {
            read_stream(wt.datagrams().readable(), |d| {
                events.borrow_mut().push_back(TransportEvent::Datagram(d));
                true
            })
            .await;
        });
        let (events, wt, closed) = (self.events.clone(), self.wt.clone(), self.closed.clone());
        spawn_local(async move {
            let message = match JsFuture::from(wt.closed()).await {
                Ok(info) => Reflect::get(&info, &"reason".into())
                    .ok()
                    .and_then(|r| r.as_string())
                    .filter(|r| !r.is_empty()),
                Err(e) => Some(err_text(&e)),
            };
            *closed.borrow_mut() = true;
            // The control stream's last frames (a Reject just ahead of the close) were queued
            // before this: the session reads them first.
            events.borrow_mut().push_back(TransportEvent::Closed(
                message.unwrap_or_else(|| "connection closed".into()),
            ));
        });
    }

    pub fn events(&self) -> Events {
        self.events.clone()
    }

    pub fn send_reliable(&self, bytes: &[u8]) {
        if *self.closed.borrow() {
            return;
        }
        let _ = self
            .control
            .write_with_chunk(&Uint8Array::from(&frame(bytes)[..]));
    }

    pub fn send_datagram(&self, bytes: Vec<u8>) {
        if *self.closed.borrow() {
            return;
        }
        if let Some(now) = self.datagrams.borrow_mut().send(bytes) {
            write_datagram(self.datagram_writer.clone(), self.datagrams.clone(), now);
        }
    }

    pub fn close(&self) {
        *self.closed.borrow_mut() = true;
        self.wt.close();
    }
}

/// One datagram write at a time; the sender picks the next when it completes (datagram.rs).
fn write_datagram(
    writer: WritableStreamDefaultWriter,
    sender: Rc<RefCell<DatagramSender>>,
    bytes: Vec<u8>,
) {
    spawn_local(async move {
        let _ = JsFuture::from(writer.write_with_chunk(&Uint8Array::from(&bytes[..]))).await;
        let next = sender.borrow_mut().completed();
        if let Some(next) = next {
            write_datagram(writer, sender, next);
        }
    });
}

/// Data-channel ids pre-negotiated with the server (ADR 0008).
const DATAGRAM_CHANNEL_ID: u16 = 2;
const LOD_CHANNEL_ID: u16 = 3;
const CONNECT_TIMEOUT_MS: i32 = 8000;

/// WebRTC to a dedicated server's ICE-lite endpoint. Channels: 0 = control and 1 = world
/// (reliable, ordered), 2 = datagrams (unordered, no retransmits), 3 = lod. SCTP keeps message
/// boundaries, so messages are sent unframed.
pub struct WebRtcConn {
    pc: RtcPeerConnection,
    control: RtcDataChannel,
    datagrams: RtcDataChannel,
    events: Events,
    closed: Rc<RefCell<bool>>,
    _handlers: Vec<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    pub binding: [u8; 32],
}

fn channel(pc: &RtcPeerConnection, label: &str, id: u16, reliable: bool) -> RtcDataChannel {
    let init = RtcDataChannelInit::new();
    init.set_negotiated(true);
    init.set_id(id);
    if !reliable {
        init.set_ordered(false);
        init.set_max_retransmits(0);
    }
    let c = pc.create_data_channel_with_data_channel_dict(label, &init);
    c.set_binary_type(RtcDataChannelType::Arraybuffer);
    c
}

impl WebRtcConn {
    pub async fn connect(invite: &WebRtcInvite, cert_hash: [u8; 32]) -> Result<Self, String> {
        let pc = RtcPeerConnection::new().map_err(|e| err_text(&e))?;
        let control = channel(&pc, "control", Channel::Control as u16, true);
        let world = channel(&pc, "world", Channel::World as u16, true);
        let datagrams = channel(&pc, "datagrams", DATAGRAM_CHANNEL_ID, false);
        let lod = channel(&pc, "lod", LOD_CHANNEL_ID, true);
        let channels = [
            control.clone(),
            world.clone(),
            datagrams.clone(),
            lod.clone(),
        ];
        let result: Result<(), String> = async {
            let offer = JsFuture::from(pc.create_offer())
                .await
                .map_err(|e| err_text(&e))?;
            let sdp = Reflect::get(&offer, &"sdp".into())
                .ok()
                .and_then(|s| s.as_string())
                .unwrap_or_default();
            JsFuture::from(pc.set_local_description(offer.unchecked_ref()))
                .await
                .map_err(|e| err_text(&e))?;
            let mid = sdp
                .lines()
                .find_map(|l| l.strip_prefix("a=mid:"))
                .map(|m| m.trim().to_string())
                .unwrap_or_else(|| "0".into());
            let answer = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer.set_sdp(&build_server_answer(invite, &cert_hash, &mid));
            JsFuture::from(pc.set_remote_description(&answer))
                .await
                .map_err(|e| err_text(&e))?;
            // Wait until every channel is open (or time out, or fail).
            let deadline = js_sys::Date::now() + f64::from(CONNECT_TIMEOUT_MS);
            loop {
                if channels
                    .iter()
                    .all(|c| c.ready_state() == RtcDataChannelState::Open)
                {
                    return Ok(());
                }
                if pc.connection_state() == web_sys::RtcPeerConnectionState::Failed {
                    return Err("WebRTC connection failed".into());
                }
                if js_sys::Date::now() > deadline {
                    return Err("WebRTC connection timed out".into());
                }
                sleep_ms(20).await;
            }
        }
        .await;
        if let Err(e) = result {
            pc.close();
            return Err(e);
        }
        let events: Events = Rc::default();
        let closed: Rc<RefCell<bool>> = Rc::default();
        let deliver = |c: Option<Channel>| {
            let events = events.clone();
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
                let bytes = Uint8Array::new(&e.data()).to_vec();
                match c {
                    Some(c) if bytes.len() <= MAX_RELIABLE_MESSAGE_BYTES => events
                        .borrow_mut()
                        .push_back(TransportEvent::Reliable(c, bytes)),
                    Some(_) => {}
                    None => events
                        .borrow_mut()
                        .push_back(TransportEvent::Datagram(bytes)),
                }
            })
        };
        let handlers = vec![
            deliver(Some(Channel::Control)),
            deliver(Some(Channel::World)),
            deliver(Some(Channel::Lod)),
            deliver(None),
        ];
        control.set_onmessage(Some(handlers[0].as_ref().unchecked_ref()));
        world.set_onmessage(Some(handlers[1].as_ref().unchecked_ref()));
        lod.set_onmessage(Some(handlers[2].as_ref().unchecked_ref()));
        datagrams.set_onmessage(Some(handlers[3].as_ref().unchecked_ref()));
        let on_close = {
            let (events, closed) = (events.clone(), closed.clone());
            Closure::<dyn FnMut()>::new(move || {
                if !std::mem::replace(&mut *closed.borrow_mut(), true) {
                    events
                        .borrow_mut()
                        .push_back(TransportEvent::Closed("connection closed".into()));
                }
            })
        };
        for c in &channels {
            c.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        }
        pc.set_onconnectionstatechange(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget(); // lives as long as the connection's channels
        Ok(Self {
            pc,
            control,
            datagrams,
            events,
            closed,
            _handlers: handlers,
            binding: cert_hash,
        })
    }

    pub fn events(&self) -> Events {
        self.events.clone()
    }
    pub fn send_reliable(&self, bytes: &[u8]) {
        if !*self.closed.borrow() {
            let _ = self.control.send_with_u8_array(bytes);
        }
    }
    pub fn send_datagram(&self, bytes: &[u8]) {
        if !*self.closed.borrow() && self.datagrams.ready_state() == RtcDataChannelState::Open {
            let _ = self.datagrams.send_with_u8_array(bytes);
        }
    }
    pub fn close(&self) {
        *self.closed.borrow_mut() = true;
        self.pc.close();
    }
}

/// Resolves after `ms` (setTimeout), on the window or in a worker.
pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        let set_timeout = Reflect::get(&global, &"setTimeout".into()).expect("setTimeout");
        let _ = set_timeout.unchecked_ref::<js_sys::Function>().call2(
            &global,
            &resolve,
            &JsValue::from(ms),
        );
    });
    let _ = JsFuture::from(promise).await;
}
