//! The game client's JS API (Phase 24e): one [`Client`] per page owns the WebGPU canvas and the
//! renderer, the session and its transport, the game loop, the input state machines and the
//! worker jobs. The TS shell keeps the DOM (menus, HUD, settings, the master and rooms), forwards
//! input events, calls `frame` from `requestAnimationFrame` and draws what `hud()` returns.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::input::{EditAction, FLY_KEY, InputEvent, KeyboardMouseInput};
use dwell_game::touch::{TouchButton, TouchControls, TouchEvent};
use dwell_lod::fade::Fade;
use dwell_look::clouds::CloudQuality;
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::{Channel, TransportKind};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::jobs::WebJobs;
use super::transport::{TransportEvent, WebRtcConn, WebTransportConn};
use crate::game::{DayOverride, Game, LodSettings, Send};
use crate::net::netsim::{Direction, NetConditions, parse_net_conditions};
use crate::net::sdp::WebRtcInvite;
use crate::net::session::{Out, Session, SessionState};
use crate::pool::Pool;
use crate::render::Renderer;
use crate::telemetry;

/// The most cells `Client::voxels` reads at once (about 2 M: a 128³ box).
const MAX_VOXEL_READ: u64 = 128 * 128 * 128;

type Events = Rc<RefCell<VecDeque<TransportEvent>>>;
/// The device key's signature of the join transcript, once the shell's promise settles.
type SignatureSlot = Rc<RefCell<Option<Result<[u8; 64], String>>>>;

/// The join prompt's lists (ADR 0038): `{ hash, different, added, clientOnly }`, each item of the
/// first two `{ name, fields }`.
fn registry_object(d: &dwell_blocks::sync::Differences, hash: u64) -> Object {
    let items = |list: &[dwell_blocks::sync::Difference]| {
        let a = Array::new();
        for item in list {
            let o = Object::new();
            set(&o, "name", item.name.as_str());
            let fields = Array::new();
            for f in &item.fields {
                fields.push(&JsValue::from(f.as_str()));
            }
            set(&o, "fields", fields);
            a.push(&o);
        }
        a
    };
    let r = Object::new();
    set(&r, "hash", format!("{hash:016x}"));
    set(&r, "different", items(&d.different));
    set(&r, "added", items(&d.added));
    let only = Array::new();
    for n in &d.client_only {
        only.push(&JsValue::from(n.as_str()));
    }
    set(&r, "clientOnly", only);
    r
}

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &k.into(), &v.into());
}

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// A transport the TS shell made (a friend world's peer connection, local mode's loopback): an
/// object with `kind`, `binding`, `setHandlers`, `sendReliable`, `sendDatagram` and `close`.
struct JsTransport {
    object: JsValue,
    _handlers: Vec<Closure<dyn FnMut(JsValue, JsValue)>>,
}

impl JsTransport {
    fn new(object: JsValue, events: Events) -> Result<Self, JsValue> {
        let handlers = Object::new();
        let (a, b, c) = (events.clone(), events.clone(), events);
        let reliable =
            Closure::<dyn FnMut(JsValue, JsValue)>::new(move |channel: JsValue, bytes: JsValue| {
                let channel = channel
                    .as_f64()
                    .and_then(|c| Channel::from_u8(c as u8))
                    .unwrap_or(Channel::Control);
                a.borrow_mut().push_back(TransportEvent::Reliable(
                    channel,
                    Uint8Array::new(&bytes).to_vec(),
                ));
            });
        let datagram =
            Closure::<dyn FnMut(JsValue, JsValue)>::new(move |bytes: JsValue, _: JsValue| {
                b.borrow_mut()
                    .push_back(TransportEvent::Datagram(Uint8Array::new(&bytes).to_vec()));
            });
        let close =
            Closure::<dyn FnMut(JsValue, JsValue)>::new(move |info: JsValue, _: JsValue| {
                let message = Reflect::get(&info, &"message".into())
                    .ok()
                    .and_then(|m| m.as_string())
                    .unwrap_or_else(|| "connection closed".into());
                c.borrow_mut().push_back(TransportEvent::Closed(message));
            });
        set(&handlers, "onReliable", reliable.as_ref().clone());
        set(&handlers, "onDatagram", datagram.as_ref().clone());
        set(&handlers, "onClose", close.as_ref().clone());
        call(&object, "setHandlers", &[handlers.into()])?;
        Ok(Self {
            object,
            _handlers: vec![reliable, datagram, close],
        })
    }
    fn kind(&self) -> TransportKind {
        Reflect::get(&self.object, &"kind".into())
            .ok()
            .and_then(|k| k.as_f64())
            .and_then(|k| TransportKind::from_u8(k as u8))
            .unwrap_or(TransportKind::Loopback)
    }
    fn binding(&self) -> [u8; 32] {
        Reflect::get(&self.object, &"binding".into())
            .map(|b| Uint8Array::new(&b).to_vec())
            .ok()
            .and_then(|v| v.try_into().ok())
            .unwrap_or([0; 32])
    }
}

fn call(object: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let f: Function = Reflect::get(object, &method.into())?.dyn_into()?;
    f.apply(object, &args.iter().collect::<Array>())
}

enum Link {
    WebTransport(WebTransportConn),
    WebRtc(WebRtcConn),
    Js(JsTransport),
}

impl Link {
    fn send_reliable(&self, bytes: &[u8]) {
        match self {
            Link::WebTransport(t) => t.send_reliable(bytes),
            Link::WebRtc(t) => t.send_reliable(bytes),
            Link::Js(t) => {
                let _ = call(
                    &t.object,
                    "sendReliable",
                    &[
                        (Channel::Control as u8).into(),
                        Uint8Array::from(bytes).into(),
                    ],
                );
            }
        }
    }
    fn send_datagram(&self, bytes: Vec<u8>) {
        match self {
            Link::WebTransport(t) => t.send_datagram(bytes),
            Link::WebRtc(t) => t.send_datagram(&bytes),
            Link::Js(t) => {
                let _ = call(
                    &t.object,
                    "sendDatagram",
                    &[Uint8Array::from(&bytes[..]).into()],
                );
            }
        }
    }
    fn close(&self) {
        match self {
            Link::WebTransport(t) => t.close(),
            Link::WebRtc(t) => t.close(),
            Link::Js(t) => {
                let _ = call(&t.object, "close", &[]);
            }
        }
    }
    fn kind_name(&self) -> &'static str {
        match self {
            Link::WebTransport(_) => "webtransport",
            Link::WebRtc(_) => "webrtc",
            Link::Js(t) => match t.kind() {
                TransportKind::WebRtc => "webrtc",
                TransportKind::WebTransport => "webtransport",
                _ => "loopback",
            },
        }
    }
}

/// Simulated latency, jitter and loss on both directions (`?netsim=`).
struct NetSim {
    up: Direction<(bool, Vec<u8>)>,
    down: Direction<TransportEvent>,
}

/// How the client is set up (from the page's address and settings).
struct Options {
    display_name: String,
    client_version: String,
    public_key: [u8; 32],
    sign: Option<Function>,
    make_worker: Function,
    workers: usize,
    lod: bool,
    lod_colors: bool,
    full_chunks: bool,
    netsim: Option<NetConditions>,
    /// `?time=`, `?moon=`, `?timespeed=` (the day/night cycle's dev switches).
    day: DayOverride,
    /// The page's query, for the dev switches read from it (`?outline=`).
    search: String,
    /// The loading screen on joining (ARCHITECTURE.md §6.3); the shell turns it off for
    /// `?loading=off` and the `dwell.loading` store.
    loading_screen: bool,
}

struct Inner {
    /// Kept for the page's lifetime: Chromium's devices stop working once their instance is gone.
    _instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Rc<RefCell<Renderer>>,
    input: KeyboardMouseInput,
    touch: TouchControls,
    touch_used: bool,
    options: Options,
    link: Option<Link>,
    events: Events,
    session: Option<Session>,
    /// A registry definitions hash the player said not to ask about for this server (ADR 0038).
    trusted_registry: Option<u64>,
    netsim: Option<NetSim>,
    signature: SignatureSlot,
    game: Option<Game<Renderer>>,
    lod_settings: LodSettings,
    /// Input events for the shell (toggles, hotbar changes, flight), drained by `takeEvents`.
    shell_events: Vec<JsValue>,
    /// The F3 overlay is on: kept here so it holds for a game started later (`?debug=1`).
    debug_visible: bool,
    /// The motion effects' settings and whether the game menu is open (docs/MOTION_EFFECTS.md).
    motion: dwell_look::motion::MotionSettings,
    menu_open: bool,
    /// The Field of view setting (degrees, vertical).
    fov_setting: f64,
    /// The browser asks for reduced motion (the spyglass zooms at once).
    reduced_motion: bool,
    /// The Show your arms setting (docs/CHARACTERS.md §5).
    show_arms: bool,
}

fn shell_event(kind: &str, value: impl Into<JsValue>) -> JsValue {
    let o = Object::new();
    set(&o, "t", kind);
    set(&o, "value", value);
    o.into()
}

#[wasm_bindgen]
pub struct Client {
    inner: Rc<RefCell<Inner>>,
}

#[wasm_bindgen]
impl Client {
    /// Starts WebGPU on the canvas. Rejects when WebGPU is unavailable (the shell shows its
    /// "unsupported" screen). `options`: `{ displayName, clientVersion, makeWorker() → Worker,
    /// workers, lod, lodColors, fullChunks, netsim, search, loadingScreen }`.
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        options: JsValue,
    ) -> Result<Client, JsError> {
        let get = |k: &str| Reflect::get(&options, &k.into()).unwrap_or(JsValue::UNDEFINED);
        let options = Options {
            display_name: get("displayName")
                .as_string()
                .unwrap_or_else(|| "Player".into()),
            client_version: get("clientVersion").as_string().unwrap_or_default(),
            public_key: [0; 32],
            sign: None,
            make_worker: get("makeWorker")
                .dyn_into()
                .map_err(|_| JsError::new("options.makeWorker is required"))?,
            workers: get("workers").as_f64().unwrap_or(2.0).max(1.0) as usize,
            lod: get("lod").as_bool().unwrap_or(true),
            lod_colors: get("lodColors").as_bool().unwrap_or(false),
            full_chunks: get("fullChunks").as_bool().unwrap_or(false),
            netsim: parse_net_conditions(get("netsim").as_string().as_deref()),
            day: DayOverride::from_query(&get("search").as_string().unwrap_or_default()),
            search: get("search").as_string().unwrap_or_default(),
            loading_screen: get("loadingScreen").as_bool().unwrap_or(true),
        };
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::BROWSER_WEBGPU;
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(err)?;
        let (adapter, device, queue) = crate::request_device(&instance, Some(&surface))
            .await
            .map_err(|e| JsError::new(&e))?;
        let caps = surface.get_capabilities(&adapter);
        // Encoding to sRGB is done in the shader (tone mapping first), so the canvas is linear.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
            web_sys::console::error_1(&format!("Dwell: WebGPU error: {e}").into());
        }));
        device.set_device_lost_callback(|reason, message| {
            web_sys::console::error_1(
                &format!("Dwell: WebGPU device lost ({reason:?}): {message}").into(),
            );
        });
        let device_type = adapter.get_info().device_type;
        let mut renderer = Renderer::with_samples(
            device,
            queue,
            format,
            crate::render::samples_for(device_type),
        );
        renderer.set_adapter(device_type);
        renderer.resize(width, height, 1.0);
        renderer.set_lod_level_colors(options.lod_colors);
        // `?outline=<mode>` (docs/ART_DIRECTION.md §7): the ink outline, off without it.
        renderer.set_outline_query(&options.search);
        // `?pixelart=<preset>` (docs/proposals/PIXEL_ART_LOOK.md): the pixel-art preview, off
        // without it.
        renderer.set_pixel_art_query(&options.search);
        // `?lodshadows=off` (docs/DAY_NIGHT.md §12): the distant shadows off, for their cost.
        renderer.set_lod_shadows_query(&options.search);
        let inner = Inner {
            _instance: instance,
            _adapter: adapter,
            surface,
            config,
            renderer: Rc::new(RefCell::new(renderer)),
            input: KeyboardMouseInput::new(),
            touch: TouchControls::new(),
            touch_used: false,
            options,
            link: None,
            events: Rc::default(),
            session: None,
            trusted_registry: None,
            netsim: None,
            signature: Rc::default(),
            game: None,
            lod_settings: LodSettings {
                pixel_error: 2.0,
                cache_bytes: 256 << 20,
                detail_distance_m: None,
                full_mode: false,
            },
            shell_events: Vec::new(),
            debug_visible: false,
            motion: Default::default(),
            menu_open: false,
            fov_setting: dwell_lod::frustum::VERTICAL_FOV,
            reduced_motion: false,
            show_arms: true,
        };
        Ok(Client {
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    /// The device key the join is signed with (ADR 0004): its public key and
    /// `sign(transcript) → Promise<Uint8Array>`. Call before connecting.
    #[wasm_bindgen(js_name = setIdentity)]
    pub fn set_identity(&self, public_key: Vec<u8>, sign: Function) -> Result<(), JsError> {
        let mut s = self.inner.borrow_mut();
        s.options.public_key = public_key
            .try_into()
            .map_err(|_| JsError::new("a public key is 32 bytes"))?;
        s.options.sign = Some(sign);
        Ok(())
    }

    /// Connects to a dedicated server over WebTransport (its pinned certificate hash).
    #[wasm_bindgen(js_name = connectWebTransport)]
    pub async fn connect_web_transport(
        &self,
        url: String,
        cert_hash: Vec<u8>,
    ) -> Result<(), JsError> {
        let hash: [u8; 32] = cert_hash
            .try_into()
            .map_err(|_| JsError::new("certificate hash must be 32 bytes"))?;
        let conn = WebTransportConn::connect(&url, hash)
            .await
            .map_err(|e| JsError::new(&e))?;
        let (events, binding) = (conn.events(), conn.binding);
        self.attach(Link::WebTransport(conn), events, binding);
        Ok(())
    }

    /// Connects to a dedicated server's WebRTC endpoint (ICE-lite, ADR 0008).
    #[wasm_bindgen(js_name = connectWebRtc)]
    pub async fn connect_web_rtc(
        &self,
        ip: String,
        port: u16,
        ufrag: String,
        pwd: String,
        cert_hash: Vec<u8>,
    ) -> Result<(), JsError> {
        let hash: [u8; 32] = cert_hash
            .try_into()
            .map_err(|_| JsError::new("certificate hash must be 32 bytes"))?;
        let conn = WebRtcConn::connect(
            &WebRtcInvite {
                ip,
                port,
                ufrag,
                pwd,
            },
            hash,
        )
        .await
        .map_err(|e| JsError::new(&e))?;
        let (events, binding) = (conn.events(), conn.binding);
        self.attach(Link::WebRtc(conn), events, binding);
        Ok(())
    }

    /// Joins over a transport the shell made (friend worlds' peers, local mode's loopback).
    #[wasm_bindgen(js_name = connectTransport)]
    pub fn connect_transport(&self, transport: JsValue) -> Result<(), JsValue> {
        let events: Events = Rc::default();
        let t = JsTransport::new(transport, events.clone())?;
        let binding = t.binding();
        self.attach(Link::Js(t), events, binding);
        Ok(())
    }

    fn attach(&self, link: Link, events: Events, binding: [u8; 32]) {
        let mut s = self.inner.borrow_mut();
        let mut session = Session::new(
            binding,
            s.options.public_key,
            &s.options.display_name,
            &s.options.client_version,
            1000.0,
        );
        session.trust_registry(s.trusted_registry);
        // The frame loop's clock (requestAnimationFrame's timestamps are performance.now()'s).
        session.start(perf_now());
        s.netsim = s.options.netsim.map(|c| NetSim {
            up: Direction::new(c),
            down: Direction::new(c),
        });
        s.link = Some(link);
        s.events = events;
        s.session = Some(session);
    }

    /// One animation frame: network, the game's ticks, and drawing.
    pub fn frame(&self, now_ms: f64) {
        let t0 = telemetry::now_ms();
        telemetry::with(|t| t.begin_frame(now_ms));
        let mut s = self.inner.borrow_mut();
        let s = &mut *s;
        telemetry::span("net", || s.pump_network(now_ms));
        if s.game.is_none() {
            s.start_game();
        }
        if let (Some(game), Some(session)) = (&mut s.game, &mut s.session) {
            while let Some((m, bytes)) = session.next_game() {
                // An edit ack names the session's numbers: the game's ids come from the session,
                // in order with the modifications before it (edit prediction, §6.5).
                if let dwell_protocol::Message::EditAck(a) = &m {
                    let done = session.edit_results(a);
                    game.on_edit_results(&done);
                    continue;
                }
                game.on_game_message(m, bytes.len(), now_ms);
            }
            let stats = session.stats();
            game.rtt_ms = stats.datagram_rtt_ms.or(stats.rtt_ms);
            game.motion_settings = s.motion;
            game.menu_open = s.menu_open;
            game.fov_setting = s.fov_setting;
            game.reduced_motion = s.reduced_motion;
            game.show_arms = s.show_arms;
            let touch = s.touch_used.then_some(s.touch.state);
            let perf = web_sys::window().and_then(|w| w.performance());
            let mut clock = || perf.as_ref().map_or(0.0, |p| p.now());
            game.frame(now_ms, &mut s.input, touch.as_ref(), &mut clock);
            for out in game.take_outbox() {
                match out {
                    Send::Control(m) => session.send_control(&m),
                    Send::Datagram(m) => session.send_game_datagram(&m),
                    Send::Edit(m, id) => {
                        session.send_edit(&m, id);
                    }
                }
            }
            let dropped: Vec<(u32, bool)> = session
                .take_dropped_edits()
                .into_iter()
                .map(|id| (id, true))
                .collect();
            game.on_edit_results(&dropped);
        }
        telemetry::span("net", || s.pump_network(now_ms));
        // The clouds drift by the world's clock once the server's ticks have arrived.
        let world = s.game.as_ref().and_then(|g| g.clock.seconds(now_ms));
        s.renderer
            .borrow_mut()
            .set_cloud_time(world.unwrap_or(now_ms / 1000.0));
        s.draw();
        telemetry::add("frame.cpu", telemetry::now_ms() - t0);
    }

    /// Dev telemetry (docs/DEV_TOOLS.md): `{ frames: {name: stats}, samples: {name: stats},
    /// counters: {name: total}, frameCount }`, each stats `{ n, mean, p50, p95, p99, max, sum }`
    /// over the last frames kept (per-frame series) or events kept (samples).
    pub fn telemetry(&self) -> JsValue {
        telemetry::with(|t| {
            let stats = |list: Vec<(&'static str, telemetry::Stats)>| {
                let o = Object::new();
                for (name, st) in list {
                    let r = Object::new();
                    set(&r, "n", st.n as u32);
                    set(&r, "mean", st.mean);
                    set(&r, "p50", st.p50);
                    set(&r, "p95", st.p95);
                    set(&r, "p99", st.p99);
                    set(&r, "min", st.min);
                    set(&r, "max", st.max);
                    set(&r, "sum", st.sum);
                    set(&o, name, r);
                }
                o
            };
            let o = Object::new();
            set(&o, "frames", stats(t.frame_stats()));
            set(&o, "samples", stats(t.sample_stats()));
            let c = Object::new();
            for (name, v) in t.counters() {
                set(&c, name, v);
            }
            set(&o, "counters", c);
            set(&o, "frameCount", t.frames().count() as u32);
            o.into()
        })
    }

    /// The kept frames: `{ names, frames: [[atMs, ...values in names' order]] }`.
    #[wasm_bindgen(js_name = telemetryFrames)]
    pub fn telemetry_frames(&self) -> JsValue {
        telemetry::with(|t| {
            let o = Object::new();
            set(
                &o,
                "names",
                t.names()
                    .iter()
                    .map(|&n| JsValue::from(n))
                    .collect::<Array>(),
            );
            let frames: Array = t
                .frames()
                .map(|f| {
                    std::iter::once(f.at_ms)
                        .chain(f.values.iter().copied())
                        .map(JsValue::from)
                        .collect::<Array>()
                })
                .collect();
            set(&o, "frames", frames);
            o.into()
        })
    }

    /// The kept controller ticks, oldest first.
    #[wasm_bindgen(js_name = telemetryTicks)]
    pub fn telemetry_ticks(&self) -> Array {
        telemetry::with(|t| {
            t.ticks()
                .map(|k| {
                    let v3 = |v: [f64; 3]| v.iter().map(|&x| JsValue::from(x)).collect::<Array>();
                    let o = Object::new();
                    set(&o, "seq", k.seq);
                    set(&o, "atMs", k.at_ms);
                    set(&o, "feet", v3(k.feet));
                    set(&o, "velocity", v3(k.velocity));
                    set(&o, "eye", k.eye);
                    set(&o, "state", k.state);
                    set(&o, "grounded", k.grounded);
                    set(&o, "crouched", k.crouched);
                    set(&o, "touchingWall", k.touching_wall);
                    set(&o, "ceilingBlocked", k.ceiling_blocked);
                    set(
                        &o,
                        "move",
                        k.move_input
                            .iter()
                            .map(|&x| JsValue::from(x))
                            .collect::<Array>(),
                    );
                    set(&o, "jump", k.jump);
                    set(&o, "crouch", k.crouch);
                    JsValue::from(o)
                })
                .collect()
        })
    }

    /// Forgets the telemetry recorded so far (measure from here).
    #[wasm_bindgen(js_name = telemetryReset)]
    pub fn telemetry_reset(&self) {
        telemetry::with(telemetry::Telemetry::reset);
    }

    /// The drawing buffer's size: CSS pixels × the pixel ratio (a share of it on a CPU adapter,
    /// `render::pixel_ratio_for`).
    pub fn resize(&self, css_width: f64, css_height: f64, pixel_ratio: f64) {
        let mut s = self.inner.borrow_mut();
        let adapter = s.renderer.borrow().adapter();
        let pixel_ratio = crate::render::pixel_ratio_for(adapter, pixel_ratio);
        let (w, h) = (
            ((css_width * pixel_ratio).round() as u32).max(1),
            ((css_height * pixel_ratio).round() as u32).max(1),
        );
        if (w, h) != (s.config.width, s.config.height) {
            s.config.width = w;
            s.config.height = h;
            let renderer = s.renderer.clone();
            s.surface.configure(renderer.borrow().device(), &s.config);
        }
        s.renderer.borrow_mut().resize(w, h, pixel_ratio);
        if let Some(g) = &mut s.game {
            g.viewport_css = (css_width, css_height);
        }
    }

    // ----- Input (the shell forwards DOM events) -----

    /// A `keydown`; returns whether to `preventDefault()` it.
    #[wasm_bindgen(js_name = keyDown)]
    pub fn key_down(
        &self,
        code: &str,
        repeat: bool,
        ctrl: bool,
        meta: bool,
        time_stamp_ms: f64,
    ) -> bool {
        let mut s = self.inner.borrow_mut();
        // The spyglass (docs/SPYGLASS.md): its key, while playing (the pointer locked).
        if s.input.pointer_locked
            && dwell_look::spyglass::is_spyglass_key(
                &dwell_look::spyglass::SPYGLASS,
                code,
                ctrl,
                meta,
            )
        {
            if !repeat && let Some(g) = s.game.as_mut() {
                g.spyglass.toggle(&dwell_look::spyglass::SPYGLASS);
                g.sound(dwell_game::sound::SoundKind::Tool);
            }
            return true;
        }
        let k = s.input.key_down(code, repeat, ctrl, meta);
        for e in k.events {
            s.on_input_event(e, time_stamp_ms);
        }
        k.prevent_default
    }
    #[wasm_bindgen(js_name = keyUp)]
    pub fn key_up(&self, code: &str) {
        self.inner.borrow_mut().input.key_up(code);
    }
    pub fn blur(&self) {
        self.inner.borrow_mut().input.blur();
    }
    /// Presses or releases a key programmatically (tests and automation).
    pub fn press(&self, code: &str, down: bool) {
        self.inner.borrow_mut().input.set_key(code, down);
    }
    #[wasm_bindgen(js_name = setPointerLocked)]
    pub fn set_pointer_locked(&self, locked: bool) {
        self.inner.borrow_mut().input.pointer_locked = locked;
    }
    #[wasm_bindgen(js_name = mouseMove)]
    pub fn mouse_move(&self, dx: f64, dy: f64) {
        let mut s = self.inner.borrow_mut();
        // Zoomed, the view turns as many pixels per movement as unzoomed (docs/SPYGLASS.md).
        let k = s
            .game
            .as_ref()
            .map_or(1.0, |g| g.spyglass_view().look_scale);
        s.input.mouse_move(dx * k, dy * k);
    }
    /// A `mousedown`: `alt` is Alt (⌥) held. With the brush tool on, a middle or Alt click picks
    /// the replace target (BRUSHES.md).
    #[wasm_bindgen(js_name = mouseDown)]
    pub fn mouse_down(&self, button: i16, now_ms: f64, alt: bool) {
        let mut s = self.inner.borrow_mut();
        if s.input.pointer_locked
            && (button == 1 || alt)
            && let Some(g) = &mut s.game
            && g.brush_pick()
        {
            return;
        }
        if let Some(action) = s.input.mouse_down(button) {
            s.edit(action, now_ms);
        }
    }
    /// A `wheel` event: the hotbar, or with Alt held and the brush tool on, the brush's size, or
    /// while the spyglass is zoomed its steps (up zooms in; docs/SPYGLASS.md).
    pub fn wheel(&self, delta_y: f64, alt: bool) {
        let mut s = self.inner.borrow_mut();
        if !alt
            && let Some(step) = s.input.wheel(delta_y)
            && let Some(g) = &mut s.game
            && g.spyglass.on()
        {
            if g.spyglass.cycle(-step, &dwell_look::spyglass::SPYGLASS) {
                g.sound(dwell_game::sound::SoundKind::Tool);
            }
            return;
        }
        if let Some(step) = s.input.wheel(delta_y)
            && alt
            && let Some(g) = &mut s.game
            && g.brush_wheel(step)
        {
            return;
        }
        if let Some(step) = s.input.wheel(delta_y)
            && let Some(g) = &mut s.game
            && let Some(slot) = g.interaction.scroll(step)
        {
            s.shell_events.push(shell_event("slot", slot as u32));
        }
    }
    /// Sets the view (degrees).
    #[wasm_bindgen(js_name = setLook)]
    pub fn set_look(&self, yaw: f64, pitch: f64) {
        let mut s = self.inner.borrow_mut();
        s.input.look.yaw = yaw;
        s.input.look.pitch = pitch;
    }
    /// Places the camera at (x, y, z) looking along yaw/pitch (degrees) for tools and tests: the
    /// renderer and the LOD view from there while the player stays put (`releaseCamera` ends it).
    #[wasm_bindgen(js_name = placeCamera)]
    pub fn place_camera(&self, x: f64, y: f64, z: f64, yaw: f64, pitch: f64) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.set_view(Some(crate::game::ViewOverride {
            eye: [x, y, z],
            yaw_deg: yaw,
            pitch_deg: pitch,
        }));
        true
    }
    /// Stand-in players for the dev tools (`__dwell.puppets`): `count` bodies ahead of `x, y, z`
    /// (feet) on `face` (its sign: negative for face B) holding block `held` (0: none); 0 removes
    /// them.
    pub fn puppets(&self, count: u16, x: f64, y: f64, z: f64, face: f64, held: u16) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.puppets = (count > 0).then_some(crate::game::Puppets {
            count,
            origin: [x, y, z],
            face: dwell_world::FaceFrame::of_sign(face),
            held,
        });
        true
    }
    #[wasm_bindgen(js_name = releaseCamera)]
    pub fn release_camera(&self) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            g.set_view(None);
        }
    }
    /// [yaw, pitch] in degrees.
    pub fn look(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        vec![s.input.look.yaw, s.input.look.pitch]
    }

    // Touch controls: the shell's DOM forwards pointer events on the zones and buttons.
    #[wasm_bindgen(js_name = stickDown)]
    pub fn stick_down(&self, id: i32, x: f64, y: f64) {
        let mut s = self.inner.borrow_mut();
        s.touch_used = true;
        s.touch.stick_down(id, x, y);
    }
    #[wasm_bindgen(js_name = stickMove)]
    pub fn stick_move(&self, id: i32, x: f64, y: f64) {
        self.inner.borrow_mut().touch.stick_move(id, x, y);
    }
    #[wasm_bindgen(js_name = stickUp)]
    pub fn stick_up(&self, id: i32) {
        self.inner.borrow_mut().touch.stick_up(id);
    }
    /// Where to draw the stick: [originX, originY, knobX, knobY, running], or empty when hidden.
    #[wasm_bindgen(js_name = stickView)]
    pub fn stick_view(&self) -> Vec<f64> {
        self.inner
            .borrow()
            .touch
            .stick_view()
            .map_or_else(Vec::new, |v| {
                vec![
                    v.origin.0,
                    v.origin.1,
                    v.knob.0,
                    v.knob.1,
                    f64::from(u8::from(v.running)),
                ]
            })
    }
    #[wasm_bindgen(js_name = lookDown)]
    pub fn look_down(&self, id: i32, x: f64, y: f64, time_ms: f64) {
        let mut s = self.inner.borrow_mut();
        s.touch_used = true;
        s.touch.look_down(id, x, y, time_ms);
    }
    #[wasm_bindgen(js_name = lookMove)]
    pub fn look_move(&self, id: i32, x: f64, y: f64) {
        let mut s = self.inner.borrow_mut();
        let s = &mut *s;
        s.touch.look_move(id, x, y, &mut s.input.look);
    }
    #[wasm_bindgen(js_name = lookUp)]
    pub fn look_up(&self, id: i32, time_ms: f64, cancelled: bool, now_ms: f64) {
        let mut s = self.inner.borrow_mut();
        if let Some(e) = s.touch.look_up(id, time_ms, cancelled) {
            s.on_touch_event(e, now_ms);
        }
    }
    /// A touch button (`edit`, `run`, `fly`, `debug`, `crouch`, `jump`) pressed or released;
    /// returns whether it is now active.
    #[wasm_bindgen(js_name = touchButton)]
    pub fn touch_button(&self, name: &str, down: bool, now_ms: f64) -> bool {
        let Some(button) = touch_button(name) else {
            return false;
        };
        let mut s = self.inner.borrow_mut();
        s.touch_used = true;
        let e = if down {
            s.touch.button_down(button)
        } else {
            s.touch.button_up(button)
        };
        if let Some(e) = e {
            s.on_touch_event(e, now_ms);
        }
        s.touch.button_active(button)
    }

    /// Input events for the shell since the last call: `{ t: 'toggle' | 'slot' | 'flight', value }`.
    #[wasm_bindgen(js_name = takeEvents)]
    pub fn take_events(&self) -> Array {
        self.inner.borrow_mut().shell_events.drain(..).collect()
    }

    /// The game's sound events since the last call (docs/AUDIO.md §5): 7 numbers each (kind,
    /// block state, intensity, x, y, z, flags), for the shell's audio (`client/src/audio`).
    #[wasm_bindgen(js_name = takeSounds)]
    pub fn take_sounds(&self) -> Vec<f64> {
        let mut s = self.inner.borrow_mut();
        s.game
            .as_mut()
            .map_or_else(Vec::new, |g| dwell_game::sound::encode(&g.take_sounds()))
    }

    /// The audio listener (the camera at the last frame): eye, forward and up, 9 numbers; empty
    /// before playing.
    pub fn listener(&self) -> Vec<f64> {
        self.inner
            .borrow()
            .game
            .as_ref()
            .map_or_else(Vec::new, |g| g.listener().to_vec())
    }

    // ----- Game actions (the debug hooks use these too) -----

    /// Breaks (`"break"`) or places (`"place"`) at the crosshair.
    pub fn edit(&self, action: &str, now_ms: f64) -> bool {
        let action = if action == "place" {
            EditAction::Place
        } else {
            EditAction::Break
        };
        self.inner.borrow_mut().edit(action, now_ms)
    }
    /// Runs a region tool by name (`cornerA`, `cornerB`, `deselect`, `fill`, `hollow`, `replace`,
    /// `clear`, `copy`, `pasteMode`, `rotate`, `mirror`, `paste`, `undo`, `redo`) as its key would;
    /// true if it sent a region edit. For tools and tests (keys need the pointer locked).
    pub fn tool(&self, name: &str, now_ms: f64) -> bool {
        use dwell_game::region_tools::ToolAction as T;
        // `cornerA@x,y,z`: a corner at that world cell, not the crosshair's (the bench's region
        // step, docs/DEV_TOOLS.md).
        let (name, at) = match name.split_once('@') {
            Some((n, c)) => {
                let v: Vec<i32> = c.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                let Ok(cell) = <[i32; 3]>::try_from(v) else {
                    return false;
                };
                (n, Some(cell))
            }
            None => (name, None),
        };
        let tool = match name {
            "cornerA" => T::CornerA,
            "cornerB" => T::CornerB,
            "deselect" => T::Deselect,
            "fill" => T::Fill,
            "hollow" => T::Hollow,
            "replace" => T::Replace,
            "clear" => T::Clear,
            "copy" => T::Copy,
            "pasteMode" => T::PasteMode,
            "rotate" => T::Rotate,
            "mirror" => T::Mirror,
            "paste" => T::Paste,
            "undo" => T::Undo,
            "redo" => T::Redo,
            _ => return false,
        };
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .is_some_and(|g| g.tool_at(tool, at, now_ms))
    }
    /// Drives the brush tool (BRUSHES.md) as its keys and clicks would, for tools and tests:
    /// `toggle`, `preset:<name>`, `size:<n>`, `nextBrush`, `nextPerformer`, `apply` (left click),
    /// `gunpowder` (right click), `pick` (middle click). True if it did (for `apply` and
    /// `gunpowder`: queued a stroke).
    pub fn brush(&self, command: &str, now_ms: f64) -> bool {
        use dwell_game::brush_tool::BrushAction as B;
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else {
            return false;
        };
        let (name, arg) = command.split_once(':').unwrap_or((command, ""));
        match name {
            "toggle" => g.brush_key(B::Toggle),
            "nextBrush" => g.brush_key(B::NextBrush),
            "nextPerformer" => g.brush_key(B::NextPerformer),
            "preset" => {
                let Some(i) = dwell_world::brush::PRESETS
                    .iter()
                    .position(|p| p.name == arg)
                else {
                    return false;
                };
                g.brush_key(B::Preset(i));
            }
            "size" => {
                let Ok(n) = arg.parse::<i32>() else {
                    return false;
                };
                g.brush.resize(n - i32::from(g.brush.spec().size));
            }
            "apply" => return g.brush_apply(false, now_ms),
            "gunpowder" => return g.brush_apply(true, now_ms),
            "pick" => return g.brush_pick(),
            _ => return false,
        }
        true
    }
    pub fn select(&self, slot: i32) {
        let mut s = self.inner.borrow_mut();
        if let Some(g) = &mut s.game {
            let slot = g.interaction.select(i64::from(slot));
            s.shell_events.push(shell_event("slot", slot as u32));
        }
    }
    /// The hotbar's block names (the palette, in slot order).
    pub fn palette(&self) -> Vec<String> {
        dwell_game::block_interaction::palette()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }
    /// `{ cell: [x, y, z], material }` the selected slot would place at the crosshair, or null.
    pub fn placement(&self) -> JsValue {
        let s = self.inner.borrow();
        let Some((cell, material)) = s.game.as_ref().and_then(|g| g.interaction.placement()) else {
            return JsValue::NULL;
        };
        let o = Object::new();
        set(
            &o,
            "cell",
            cell.iter().map(|&v| JsValue::from(v)).collect::<Array>(),
        );
        set(&o, "material", material);
        o.into()
    }
    pub fn voxel(&self, x: i32, y: i32, z: i32) -> u16 {
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .map_or(0, |g| g.voxel(x, y, z))
    }
    /// The client's states in the box of `sx × sy × sz` cells from its lowest corner, in a
    /// structure's layer order (y-slices, z-rows, x fastest; `region::read_box`): saving a
    /// structure (STRUCTURES.md §4). Empty before playing, where a chunk of the box isn't loaded,
    /// or for a box over `MAX_VOXEL_READ`.
    pub fn voxels(&self, x: i32, y: i32, z: i32, sx: u32, sy: u32, sz: u32) -> Vec<u16> {
        let cells = u64::from(sx) * u64::from(sy) * u64::from(sz);
        if cells == 0 || cells > MAX_VOXEL_READ {
            return Vec::new();
        }
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .map_or_else(Vec::new, |g| g.voxels([x, y, z], [sx, sy, sz]))
    }
    /// The box selected with the region tools (B, N): `[x, y, z, sx, sy, sz]` (its lowest corner
    /// and size); empty with none. Saving a structure saves it when there is one.
    pub fn selection(&self) -> Vec<i32> {
        self.inner
            .borrow()
            .game
            .as_ref()
            .and_then(|g| g.region.selection())
            .map_or_else(Vec::new, |(min, size)| [min, size].concat())
    }
    /// Loads a structure into the region tools' clipboard and turns paste mode on: the ghost box
    /// follows the crosshair, R and M turn it, a right click pastes it as region edits (§6.5).
    /// `cells` in layer order; `keep[i]` non-zero where the structure keeps what is there. An
    /// error message, or empty when loaded.
    #[wasm_bindgen(js_name = loadStructure)]
    pub fn load_structure(
        &self,
        sx: i32,
        sy: i32,
        sz: i32,
        cells: Vec<u16>,
        keep: Vec<u8>,
        label: &str,
    ) -> String {
        let mut s = self.inner.borrow_mut();
        let Some(g) = s.game.as_mut() else {
            return "Not playing".into();
        };
        let keep: Vec<bool> = keep.iter().map(|&k| k != 0).collect();
        match g.region.load_clipboard([sx, sy, sz], cells, keep, label) {
            Ok(()) => String::new(),
            Err(e) => e,
        }
    }
    /// Turns flight on or off (automation: the bench's `fly` step, e2e) by pressing the fly key, as a
    /// player would, if it isn't already in that state; the server's flight policy still applies.
    pub fn fly(&self, on: bool) {
        if self.flying() == on {
            return;
        }
        self.key_down(FLY_KEY, false, false, false, perf_now());
        self.key_up(FLY_KEY);
    }
    #[wasm_bindgen(js_name = flySpeed)]
    pub fn fly_speed(&self, level: f64) {
        let mut s = self.inner.borrow_mut();
        if let Some(l) = s.input.flight.set_speed_level(level) {
            s.shell_events.push(shell_event("flySpeed", l));
        }
    }
    #[wasm_bindgen(js_name = flySpeedLevel)]
    pub fn fly_speed_level(&self) -> u8 {
        self.inner.borrow().input.flight.speed_level()
    }
    pub fn flying(&self) -> bool {
        self.inner.borrow().input.flight.flying()
    }
    #[wasm_bindgen(js_name = flightAllowed)]
    pub fn flight_allowed(&self) -> bool {
        self.inner.borrow().input.flight.allowed()
    }

    // ----- Settings -----

    #[wasm_bindgen(js_name = setExposure)]
    pub fn set_exposure(&self, exposure: f64) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_exposure(exposure);
    }
    /// The Clouds setting: `off`, `low` or `high` (anything else: high).
    #[wasm_bindgen(js_name = setClouds)]
    pub fn set_clouds(&self, quality: &str) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_clouds(CloudQuality::from_name(quality).unwrap_or(CloudQuality::High));
    }
    /// The clouds' coverage, 0 (clear) to 1 (overcast): for a weather system, or by hand.
    #[wasm_bindgen(js_name = setCloudCoverage)]
    pub fn set_cloud_coverage(&self, coverage: f64) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_cloud_coverage(coverage);
    }
    /// The Shadows setting (0 Off, 1 Low, 2 High; DAY_NIGHT.md §10). A CPU adapter draws Low at
    /// most.
    #[wasm_bindgen(js_name = setShadows)]
    pub fn set_shadows(&self, level: u32) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_shadow_quality(dwell_look::shadows::ShadowQuality::from_level(level));
    }
    /// The Torch shadows setting (0 Off, 1 Low, 2 High; docs/LIGHTS.md §4). A CPU adapter draws
    /// none.
    #[wasm_bindgen(js_name = setTorchShadows)]
    pub fn set_torch_shadows(&self, level: u32) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_torch_shadows(dwell_look::shadows::ShadowQuality::from_level(level));
    }
    /// The water quality (0 Low, 1 Medium, 2 High; WATER_RENDERING.md §4).
    #[wasm_bindgen(js_name = setWaterQuality)]
    pub fn set_water_quality(&self, level: u32) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_water_quality(crate::render::water::WaterQuality::from_level(level));
    }
    /// The water's cost-isolation switches (`?waterdbg=`, a dev tool): names separated by commas,
    /// each turning one piece of the water's shading off (render/water.rs `DEBUG_SWITCHES`).
    #[wasm_bindgen(js_name = setWaterDebug)]
    pub fn set_water_debug(&self, names: &str) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_water_debug(crate::render::water::debug_mask(names));
    }
    /// The ink outline's mode by name (`shared/look/outline.json`; docs/ART_DIRECTION.md §7), as
    /// `?outline=` takes it; `off` or an unknown name turns it off. True when a mode is on.
    #[wasm_bindgen(js_name = setOutline)]
    pub fn set_outline(&self, name: &str) -> bool {
        self.inner.borrow().renderer.borrow_mut().set_outline(name)
    }
    /// The ink outline's mode, or `off`.
    #[wasm_bindgen(js_name = outline)]
    pub fn outline(&self) -> String {
        self.inner
            .borrow()
            .renderer
            .borrow()
            .outline()
            .unwrap_or("off")
            .into()
    }
    /// The pixel-art preview's preset by name (`shared/look/pixelart.json`;
    /// docs/proposals/PIXEL_ART_LOOK.md), as `?pixelart=` takes it; `off` or an unknown name turns
    /// it off. True when a preset is on.
    #[wasm_bindgen(js_name = setPixelArt)]
    pub fn set_pixel_art(&self, name: &str) -> bool {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_pixel_art(name)
    }
    /// The pixel-art preview's preset, or `off`.
    #[wasm_bindgen(js_name = pixelArt)]
    pub fn pixel_art(&self) -> String {
        self.inner
            .borrow()
            .renderer
            .borrow()
            .pixel_art()
            .unwrap_or("off")
            .into()
    }
    /// The motion effects (docs/MOTION_EFFECTS.md): on or off, and the FOV boost's strength 0–1.
    #[wasm_bindgen(js_name = setMotion)]
    pub fn set_motion(&self, enabled: bool, fov_strength: f64) {
        self.inner.borrow_mut().motion = dwell_look::motion::MotionSettings {
            enabled,
            fov_strength: if fov_strength.is_finite() {
                fov_strength.clamp(0.0, 1.0)
            } else {
                1.0
            },
        };
    }
    /// The plants' wind scale (0 still, 1 the data's) and whether cutout meshes are drawn at all
    /// (docs/TRANSPARENT_BLOCKS.md; dev tools: `__dwell.plants`).
    #[wasm_bindgen(js_name = setPlants)]
    pub fn set_plants(&self, wind: f64, shown: bool) {
        let s = self.inner.borrow();
        let mut r = s.renderer.borrow_mut();
        r.set_plant_wind(wind);
        r.set_cutout_shown(shown);
    }
    /// The Field of view setting: the vertical FOV in degrees (60–100, `dwell_lod::base_fov`).
    #[wasm_bindgen(js_name = setFov)]
    pub fn set_fov(&self, degrees: f64) {
        self.inner.borrow_mut().fov_setting = dwell_lod::sanitize_fov_setting(degrees);
    }
    /// The Show your arms setting: your first-person arms and what they hold (CHARACTERS.md §5).
    #[wasm_bindgen(js_name = setShowArms)]
    pub fn set_show_arms(&self, shown: bool) {
        self.inner.borrow_mut().show_arms = shown;
    }
    /// Whether the browser asks for reduced motion: the spyglass then zooms without easing.
    #[wasm_bindgen(js_name = setReducedMotion)]
    pub fn set_reduced_motion(&self, reduced: bool) {
        self.inner.borrow_mut().reduced_motion = reduced;
    }
    /// Zooms the spyglass to `step` (0 the least zoom) or out (docs/SPYGLASS.md; dev tools:
    /// `__dwell.spyglass`). False before the game has started.
    pub fn spyglass(&self, on: bool, step: u32) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.spyglass
            .set(on, step as usize, &dwell_look::spyglass::SPYGLASS);
        true
    }
    /// The game menu opened or closed: the motion effects hold still while it is open.
    #[wasm_bindgen(js_name = setMenuOpen)]
    pub fn set_menu_open(&self, open: bool) {
        self.inner.borrow_mut().menu_open = open;
    }
    /// Sets the time of day this client shows (0–1, 0 midnight), or returns to the world's with
    /// `undefined` (a dev switch: the bench's `time` step; the world's time is the server's).
    #[wasm_bindgen(js_name = setTimeOfDay)]
    pub fn set_time_of_day(&self, t: Option<f64>) {
        let mut s = self.inner.borrow_mut();
        s.options.day.time = t.filter(|v| v.is_finite()).map(|v| v.rem_euclid(1.0));
        let day = s.options.day;
        if let Some(g) = &mut s.game {
            g.day_override = day;
        }
    }
    /// Sets the moon's phase this client shows (0 new, 0.5 full), or the world's with `undefined`.
    #[wasm_bindgen(js_name = setMoonPhase)]
    pub fn set_moon_phase(&self, phase: Option<f64>) {
        let mut s = self.inner.borrow_mut();
        s.options.day.moon = phase.filter(|v| v.is_finite()).map(|v| v.rem_euclid(1.0));
        let day = s.options.day;
        if let Some(g) = &mut s.game {
            g.day_override = day;
        }
    }
    /// The whole-world view's detail (§6.6): distance (m, or negative for none), pixel error and
    /// memory budget (MB).
    #[wasm_bindgen(js_name = setDetail)]
    pub fn set_detail(&self, distance_m: f64, pixel_error: f64, memory_mb: f64) {
        let mut s = self.inner.borrow_mut();
        let distance = (distance_m >= 0.0).then_some(distance_m);
        s.lod_settings.detail_distance_m = distance;
        s.lod_settings.pixel_error = pixel_error;
        s.lod_settings.cache_bytes = (memory_mb * 1_048_576.0) as usize;
        if let Some(lod) = s.game.as_mut().and_then(|g| g.lod()) {
            lod.set_detail_distance(distance);
            lod.set_quality(pixel_error, (memory_mb * 1_048_576.0) as usize);
        }
    }
    #[wasm_bindgen(js_name = setDebugVisible)]
    pub fn set_debug_visible(&self, visible: bool) {
        let mut s = self.inner.borrow_mut();
        s.debug_visible = visible;
        if let Some(g) = &mut s.game {
            g.debug_visible = visible;
        }
    }
    #[wasm_bindgen(js_name = setDebugNote)]
    pub fn set_debug_note(&self, note: String) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            g.debug_note = note;
        }
    }
    #[wasm_bindgen(js_name = setMemoryNote)]
    pub fn set_memory_note(&self, note: String) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            g.memory_note = note;
        }
    }

    // ----- State for the shell -----

    /// Joins without the registry prompt when the server's definitions hash is `hash` (16 hex
    /// digits; "don't ask again for this server", ADR 0038). Call before `connect`.
    #[wasm_bindgen(js_name = trustRegistry)]
    pub fn trust_registry(&self, hash: Option<String>) {
        let hash = hash.and_then(|h| u64::from_str_radix(&h, 16).ok());
        let mut s = self.inner.borrow_mut();
        s.trusted_registry = hash;
        if let Some(session) = &mut s.session {
            session.trust_registry(hash);
        }
    }

    /// The player's answer to the registry prompt (`phase: 'confirming'`): Continue or Cancel.
    #[wasm_bindgen(js_name = answerRegistry)]
    pub fn answer_registry(&self, proceed: bool) {
        let mut s = self.inner.borrow_mut();
        let Some(session) = &mut s.session else {
            return;
        };
        if proceed {
            session.accept_registry();
        } else {
            session.cancel_registry();
        }
    }

    /// `{ phase: 'connecting' | 'handshaking' | 'confirming' | 'joined' | 'rejected' | 'closed',
    /// … }`; `confirming` carries `registry: { hash, different, added, clientOnly }` (ADR 0038).
    #[wasm_bindgen(js_name = sessionState)]
    pub fn session_state(&self) -> JsValue {
        let s = self.inner.borrow();
        let o = Object::new();
        match s.session.as_ref().map(Session::state) {
            None => set(&o, "phase", "connecting"),
            Some(SessionState::Handshaking) => set(&o, "phase", "handshaking"),
            Some(SessionState::Confirming { differences, hash }) => {
                set(&o, "phase", "confirming");
                set(&o, "registry", registry_object(differences, *hash));
            }
            Some(SessionState::Joined {
                player_id,
                world_seed,
                generator_version,
                may_fly,
                ..
            }) => {
                set(&o, "phase", "joined");
                set(&o, "playerId", *player_id);
                set(&o, "worldSeed", js_sys::BigInt::from(*world_seed));
                set(&o, "generatorVersion", *generator_version);
                set(&o, "mayFly", *may_fly);
            }
            Some(SessionState::Rejected { reason, message }) => {
                set(&o, "phase", "rejected");
                set(&o, "reason", *reason as u8);
                set(&o, "message", message.as_str());
            }
            Some(SessionState::Closed { message }) => {
                set(&o, "phase", "closed");
                set(&o, "message", message.as_str());
            }
        }
        if let Some(session) = &s.session {
            let st = session.stats();
            let stats = Object::new();
            set(
                &stats,
                "rttMs",
                st.rtt_ms.map_or(JsValue::NULL, JsValue::from),
            );
            set(
                &stats,
                "datagramRttMs",
                st.datagram_rtt_ms.map_or(JsValue::NULL, JsValue::from),
            );
            set(&stats, "serverTick", st.server_tick);
            set(&stats, "hostPaused", st.host_paused);
            set(&o, "stats", stats);
        }
        set(&o, "transport", s.link.as_ref().map_or("", Link::kind_name));
        o.into()
    }

    /// `{ message, status, health, debug, damaged }` after the latest frame.
    pub fn hud(&self) -> JsValue {
        let mut s = self.inner.borrow_mut();
        let o = Object::new();
        if let Some(g) = &mut s.game {
            set(&o, "message", g.hud.message.as_str());
            set(&o, "status", g.hud.status.as_str());
            set(&o, "health", g.hud.health);
            set(
                &o,
                "debug",
                g.hud.debug.as_deref().map_or(JsValue::NULL, JsValue::from),
            );
            set(&o, "damaged", std::mem::take(&mut g.hud.damaged));
            set(&o, "region", g.region.status().as_str());
            // The brush panel (BRUSHES.md): null while the tool is off.
            match g.brush_panel() {
                Some(p) => {
                    let b = Object::new();
                    set(&b, "preset", p.preset);
                    set(&b, "brush", p.brush);
                    set(&b, "performer", p.performer);
                    set(&b, "size", u32::from(p.size));
                    set(&b, "material", p.material.as_str());
                    set(
                        &b,
                        "replace",
                        p.replace.as_deref().map_or(JsValue::NULL, JsValue::from),
                    );
                    set(&b, "note", p.note.as_str());
                    set(&o, "brush", b);
                }
                None => set(&o, "brush", JsValue::NULL),
            }
            set(&o, "brushes", g.brush.available);
            // The loading screen (ARCHITECTURE.md §6.3): null once open.
            match g.loading_screen() {
                Some(sc) => {
                    let l = Object::new();
                    set(&l, "text", dwell_game::loading::loading_text(&sc).as_str());
                    set(
                        &l,
                        "ready",
                        sc.progress.ready.min(sc.progress.needed) as u32,
                    );
                    set(&l, "needed", sc.progress.needed as u32);
                    set(&o, "loading", l);
                }
                None => set(&o, "loading", JsValue::NULL),
            }
            // The spyglass's vignette (docs/SPYGLASS.md): null while unzoomed.
            let v = g.spyglass_view();
            if v.vignette > 0.0 {
                let t = &dwell_look::spyglass::SPYGLASS.vignette;
                let z = Object::new();
                set(&z, "amount", v.vignette);
                set(&z, "magnification", v.magnification);
                set(&z, "innerRadius", t.inner_radius);
                set(&z, "outerRadius", t.outer_radius);
                set(&z, "opacity", t.opacity);
                let rgb = Array::new();
                for c in t.color {
                    rgb.push(&JsValue::from(c));
                }
                set(&z, "color", rgb);
                set(&o, "spyglass", z);
            } else {
                set(&o, "spyglass", JsValue::NULL);
            }
        }
        o.into()
    }

    /// The `window.__dwell.state()` object (GameDebugState), or null before playing.
    #[wasm_bindgen(js_name = debugState)]
    pub fn debug_state(&self) -> JsValue {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return JsValue::NULL;
        };
        let d = g.debug_state();
        let o = Object::new();
        let v3 = |v: [f64; 3]| v.iter().map(|&x| JsValue::from(x)).collect::<Array>();
        set(&o, "playerId", d.player_id);
        set(&o, "active", d.active);
        set(&o, "feet", v3(d.feet));
        set(&o, "health", d.health);
        set(&o, "dead", d.dead);
        let day = g.day();
        set(&o, "timeOfDay", day.time_of_day);
        set(&o, "day", day.day as f64);
        set(&o, "moonPhase", day.moon_phase);
        let remotes: Array = d
            .remotes
            .iter()
            .map(|(id, feet, dead)| {
                let r = Object::new();
                set(&r, "playerId", *id);
                set(&r, "feet", v3(*feet));
                set(&r, "dead", *dead);
                JsValue::from(r)
            })
            .collect();
        set(&o, "remotes", remotes);
        let bodies: Array = d
            .bodies
            .iter()
            .map(|body| {
                let b = Object::new();
                set(&b, "playerId", body.player_id);
                set(&b, "role", body.role);
                set(
                    &b,
                    "action",
                    body.action.map_or(JsValue::NULL, JsValue::from),
                );
                let played: Array = body.played.iter().map(|&r| JsValue::from(r)).collect();
                set(&b, "played", played);
                set(&b, "plays", body.plays);
                JsValue::from(b)
            })
            .collect();
        set(&o, "bodies", bodies);
        let arms = Object::new();
        set(&arms, "role", d.arms.0);
        set(
            &arms,
            "action",
            d.arms.1.map_or(JsValue::NULL, JsValue::from),
        );
        set(&o, "arms", arms);
        let st = Object::new();
        set(&st, "ticks", d.stats.ticks);
        set(&st, "snapshots", d.stats.snapshots);
        set(&st, "replays", d.stats.replays);
        set(&st, "snaps", d.stats.snaps);
        set(&st, "knockbackReplays", d.stats.knockback_replays);
        set(&st, "resets", d.stats.resets);
        set(&st, "lastCorrection", d.stats.last_correction);
        set(&st, "lastError", d.stats.last_error);
        set(&o, "stats", st);
        let t = Object::new();
        set(&t, "loaded", d.terrain.loaded as u32);
        set(&t, "generating", d.terrain.generating as u32);
        set(&t, "meshed", d.terrain.meshed as u32);
        set(&t, "dirty", d.terrain.dirty as u32);
        set(&t, "meshing", d.terrain.meshing as u32);
        set(&t, "resyncs", d.terrain.resyncs);
        set(&o, "terrain", t);
        set(&o, "terrainReady", d.terrain_ready);
        // The loading screen (ARCHITECTURE.md §6.3): null while open; openedS and timedOut once
        // its last wait ended.
        let l = Object::new();
        set(&l, "enabled", d.loading_enabled);
        set(&l, "shown", d.loading.is_some());
        if let Some(sc) = d.loading {
            set(&l, "ready", sc.progress.ready as u32);
            set(&l, "needed", sc.progress.needed as u32);
        }
        if let Some((how, after)) = d.loading_opened {
            set(&l, "openedS", after);
            set(&l, "timedOut", how == dwell_game::loading::Opened::TimedOut);
        }
        set(&o, "loading", l);
        set(
            &o,
            "target",
            d.target.map_or(JsValue::NULL, |t| {
                let r = Object::new();
                set(
                    &r,
                    "cell",
                    t.cell.iter().map(|&v| JsValue::from(v)).collect::<Array>(),
                );
                set(&r, "face", t.face);
                r.into()
            }),
        );
        set(
            &o,
            "lod",
            d.lod.map_or(JsValue::NULL, |l| {
                let r = Object::new();
                set(
                    &r,
                    "drawn",
                    l.drawn
                        .iter()
                        .map(|&n| JsValue::from(n as u32))
                        .collect::<Array>(),
                );
                set(&r, "chunkSections", l.chunk_sections as u32);
                set(&r, "nodes", l.nodes as u32);
                set(&r, "generating", l.generating as u32);
                set(&r, "meshing", l.meshing as u32);
                set(&r, "asking", l.asking as u32);
                set(&r, "cacheBytes", l.cache_bytes as f64);
                set(&r, "inUseBytes", l.in_use_bytes as f64);
                set(&r, "errorScale", l.error_scale);
                set(&r, "waitingInView", l.waiting[0] as u32);
                set(&r, "waitingOffView", l.waiting[1] as u32);
                r.into()
            }),
        );
        set(&o, "flying", d.flying);
        // The frame's vertical FOV and the spyglass's magnification (docs/SPYGLASS.md).
        set(&o, "fov", g.frame_fov());
        set(&o, "spyglass", g.spyglass_view().magnification);
        o.into()
    }

    /// The last frame's `{ calls, triangles, meshBytes, screenBytes, pixelRatio }`.
    #[wasm_bindgen(js_name = renderStats)]
    pub fn render_stats(&self) -> JsValue {
        let st = self.inner.borrow().renderer.borrow().stats();
        let o = Object::new();
        set(&o, "calls", st.calls);
        set(&o, "triangles", st.triangles as f64);
        set(&o, "meshBytes", st.mesh_bytes as f64);
        set(&o, "screenBytes", st.screen_bytes as f64);
        set(&o, "pixelRatio", st.pixel_ratio);
        // The characters (render/characters.rs): bodies drawn, the arms, their meshes' GPU bytes
        // and this frame's instance upload.
        let c = self.inner.borrow().renderer.borrow().character_stats();
        set(&o, "characters", c.bodies);
        set(&o, "arms", c.arms);
        set(&o, "characterMeshBytes", c.mesh_bytes as f64);
        set(&o, "characterUploadBytes", c.upload_bytes as f64);
        o.into()
    }

    /// The chunk's revision in the client's world (the F3 diff), or -1.
    #[wasm_bindgen(js_name = chunkRevision)]
    pub fn chunk_revision(&self, x: i32, y: i32, z: i32) -> f64 {
        self.inner
            .borrow()
            .game
            .as_ref()
            .and_then(|g| g.chunk_revision([x, y, z]))
            .map_or(-1.0, f64::from)
    }
    /// The chunk's padded voxels in the client's world (the F3 diff).
    #[wasm_bindgen(js_name = paddedChunk)]
    pub fn padded_chunk(&self, x: i32, y: i32, z: i32) -> Vec<u16> {
        use dwell_game::chunk_stream::ChunkStore;
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .map_or_else(Vec::new, |g| g.world().padded_chunk([x, y, z]))
    }

    /// Asks for the F4 map around (x0, z0); `takeMap` returns it once generated.
    #[wasm_bindgen(js_name = requestMap)]
    pub fn request_map(&self, x0: i32, z0: i32, step: i32, n: i32, face: u8) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            g.request_map(x0, z0, step, n, face);
        }
    }
    /// The requested map (3 bytes per sample: height i16 LE, outside flag), or undefined.
    #[wasm_bindgen(js_name = takeMap)]
    pub fn take_map(&self) -> Option<Vec<u8>> {
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .and_then(|g| g.take_map())
    }
    /// Regenerates the chunk under the player and diffs it (shown in the F3 overlay).
    #[wasm_bindgen(js_name = requestDiff)]
    pub fn request_diff(&self) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            let c = dwell_world::chunk_of_pos(g.debug_state().feet);
            g.request_diff([c.x, c.y, c.z]);
        }
    }

    /// Each job worker's WebAssembly memory (bytes), for the F3 memory line.
    #[wasm_bindgen(js_name = workerMemory)]
    pub fn worker_memory(&self) -> Vec<f64> {
        self.inner
            .borrow()
            .game
            .as_ref()
            .map_or_else(Vec::new, |g| g.worker_memory())
    }

    /// Closes the connection.
    pub fn close(&self) {
        let mut s = self.inner.borrow_mut();
        if let Some(l) = s.link.take() {
            l.close();
        }
    }
}

/// Milliseconds on `performance.now()`'s timeline, which `frame`'s timestamps are on.
fn perf_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn touch_button(name: &str) -> Option<TouchButton> {
    Some(match name {
        "edit" => TouchButton::Edit,
        "run" => TouchButton::Run,
        "fly" => TouchButton::Fly,
        "debug" => TouchButton::Debug,
        "crouch" => TouchButton::Crouch,
        "jump" => TouchButton::Jump,
        _ => return None,
    })
}

impl Inner {
    fn edit(&mut self, action: EditAction, now_ms: f64) -> bool {
        self.game.as_mut().is_some_and(|g| g.edit(action, now_ms))
    }

    fn on_input_event(&mut self, e: InputEvent, now_ms: f64) {
        match e {
            InputEvent::Toggle(key) => self.shell_events.push(shell_event("toggle", key)),
            InputEvent::Digit(code) => {
                let slot = code
                    .strip_prefix("Digit")
                    .and_then(|d| d.parse::<i64>().ok())
                    .map(|d| if d == 0 { 9 } else { d - 1 });
                if let (Some(slot), Some(g)) = (slot, &mut self.game)
                    && (slot as usize) < g.interaction.palette().len()
                {
                    let slot = g.interaction.select(slot);
                    self.shell_events.push(shell_event("slot", slot as u32));
                }
            }
            InputEvent::FlightChanged(on) => self.shell_events.push(shell_event("flight", on)),
            InputEvent::Tool(tool) => {
                if let Some(g) = &mut self.game {
                    g.tool(tool, now_ms);
                }
            }
            InputEvent::Brush(action) => {
                if let Some(g) = &mut self.game {
                    g.brush_key(action);
                }
            }
        }
    }

    fn on_touch_event(&mut self, e: TouchEvent, now_ms: f64) {
        match e {
            TouchEvent::Tap => {
                let action = self
                    .game
                    .as_ref()
                    .map_or(EditAction::Break, |g| g.interaction.touch_action);
                self.edit(action, now_ms);
            }
            TouchEvent::PlaceMode(place) => {
                if let Some(g) = &mut self.game {
                    g.interaction.touch_action = if place {
                        EditAction::Place
                    } else {
                        EditAction::Break
                    };
                }
            }
            TouchEvent::Fly => {
                if let Some(v) = self.input.flight.toggle() {
                    self.shell_events.push(shell_event("flight", v));
                }
            }
            TouchEvent::Debug => self.shell_events.push(shell_event("toggle", "F3")),
        }
    }

    /// Moves bytes between the transport and the session (through the simulated link, if any).
    fn pump_network(&mut self, now_ms: f64) {
        let Some(session) = &mut self.session else {
            return;
        };
        let mut random = || js_sys::Math::random();
        let arrived: Vec<TransportEvent> = self.events.borrow_mut().drain(..).collect();
        let arrived = match &mut self.netsim {
            Some(sim) => {
                for e in arrived {
                    let reliable = !matches!(e, TransportEvent::Datagram(_));
                    sim.down.send(now_ms, reliable, e, &mut random);
                }
                sim.down.due(now_ms)
            }
            None => arrived,
        };
        for e in arrived {
            match e {
                TransportEvent::Reliable(channel, bytes) => {
                    session.on_reliable(channel, &bytes, now_ms)
                }
                TransportEvent::Datagram(bytes) => session.on_datagram(&bytes, now_ms),
                TransportEvent::Closed(message) => session.on_close(&message),
            }
        }
        session.tick(now_ms);
        if let Some(sig) = self.signature.borrow_mut().take() {
            match sig {
                Ok(sig) => session.signed(sig),
                Err(e) => session.on_close(&format!("could not sign the join: {e}")),
            }
        }
        let mut outgoing = Vec::new();
        for out in session.take_out() {
            match out {
                Out::Reliable(b) => outgoing.push((true, b)),
                Out::Datagram(b) => outgoing.push((false, b)),
                Out::Sign(transcript) => {
                    let promise = match &self.options.sign {
                        Some(sign) => {
                            sign.call1(&JsValue::NULL, &Uint8Array::from(&transcript[..]))
                        }
                        None => Err("no device key (setIdentity)".into()),
                    };
                    let slot = self.signature.clone();
                    spawn_local(async move {
                        let result = match promise.map(|p| JsFuture::from(Promise::resolve(&p))) {
                            Ok(f) => f.await.map(|v| Uint8Array::new(&v).to_vec()),
                            Err(e) => Err(e),
                        };
                        *slot.borrow_mut() = Some(match result {
                            Ok(v) => v
                                .try_into()
                                .map_err(|_| "a signature is 64 bytes".to_string()),
                            Err(e) => Err(format!("{e:?}")),
                        });
                    });
                }
                Out::Close => {
                    if let Some(l) = self.link.take() {
                        l.close();
                    }
                }
            }
        }
        let outgoing = match &mut self.netsim {
            Some(sim) => {
                for (reliable, b) in outgoing {
                    sim.up.send(now_ms, reliable, (reliable, b), &mut random);
                }
                sim.up.due(now_ms)
            }
            None => outgoing,
        };
        if let Some(link) = &self.link {
            for (reliable, b) in outgoing {
                if reliable {
                    link.send_reliable(&b);
                } else {
                    link.send_datagram(b);
                }
            }
        }
    }

    /// Once joined: the game with its workers, flight as the server allows, the whole-world view.
    fn start_game(&mut self) {
        let Some(SessionState::Joined {
            player_id,
            world_seed,
            generator_version,
            verification_chunk,
            may_fly,
            region_edits,
            brushes,
        }) = self.session.as_ref().map(|s| s.state().clone())
        else {
            return;
        };
        let workers = self.options.workers;
        let jobs = match WebJobs::new(
            &self.options.make_worker,
            workers,
            workers,
            (generator_version, world_seed),
        ) {
            Ok(j) => j,
            Err(e) => {
                web_sys::console::error_2(&"Dwell: workers unavailable".into(), &e);
                return;
            }
        };
        let pool = Pool::new(Box::new(jobs));
        let verification = (!self.options.full_chunks)
            .then(|| dwell_world::ChunkCoord::from_array(verification_chunk));
        let mut game = Game::new(
            player_id,
            pool,
            self.renderer.clone(),
            verification,
            PlayerControllerConfig::default(),
        );
        let size = self.renderer.borrow().canvas_size();
        let ratio = self.renderer.borrow().stats().pixel_ratio.max(0.01);
        game.viewport_css = (f64::from(size.0) / ratio, f64::from(size.1) / ratio);
        game.debug_visible = self.debug_visible;
        game.set_capabilities(region_edits, brushes, generator_version);
        game.day_override = self.options.day;
        game.set_loading_screen(self.options.loading_screen);
        let mapping = self.session.as_ref().and_then(|s| s.mapping().cloned());
        if let Some(m) = mapping.as_ref().filter(|m| !m.differences.is_empty()) {
            // The console keeps the full lists (the F3 overlay shows the summary).
            let d = &m.differences;
            let list = |v: &[dwell_blocks::sync::Difference]| {
                v.iter()
                    .map(|d| format!("{} ({})", d.name, d.fields.join(", ")))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            web_sys::console::info_1(
                &format!(
                    "Dwell: {}. Different on this server: {}. New on this server: {}. Only in this version: {}.",
                    d.summary(),
                    list(&d.different),
                    list(&d.added),
                    d.client_only.join(", ")
                )
                .into(),
            );
        }
        game.set_registry(mapping);
        if let Some(v) = self.input.flight.set_allowed(may_fly) {
            self.shell_events.push(shell_event("flightAllowed", v));
        }
        if self.options.lod {
            let mut settings = self.lod_settings;
            settings.full_mode = self.options.full_chunks;
            game.enable_lod(settings);
        }
        self.game = Some(game);
    }

    fn draw(&mut self) {
        // Not while the GPU is still behind on earlier frames (render/pacing.rs): recording now
        // would block this thread until it caught up.
        if !self
            .renderer
            .borrow_mut()
            .ready_to_draw(telemetry::now_ms())
        {
            telemetry::count("draw.skipped", 1.0);
            return;
        }
        let acquire = telemetry::now_ms();
        let current = self.surface.get_current_texture();
        telemetry::add("draw.acquire", telemetry::now_ms() - acquire);
        let frame = match current {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                let renderer = self.renderer.clone();
                self.surface
                    .configure(renderer.borrow().device(), &self.config);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let game = &self.game;
        telemetry::span("draw.render", || {
            self.renderer.borrow_mut().render(&view, &|c| {
                game.as_ref().map_or(Some(Fade::Full), |g| g.chunk_fade(c))
            });
        });
        let queue = self.renderer.borrow().queue().clone();
        telemetry::span("draw.present", || queue.present(frame));
    }
}
