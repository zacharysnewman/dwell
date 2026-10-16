//! The game client's JS API (Phase 24e): one [`Client`] per page owns the WebGPU canvas and the
//! renderer, the session and its transport, the game loop, the input state machines and the
//! worker jobs. The TS shell keeps the DOM (menus, HUD, settings, the master and rooms), forwards
//! input events, calls `frame` from `requestAnimationFrame` and draws what `hud()` returns.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::input::{EditAction, FLY_KEY, InputEvent, KeyboardMouseInput};
use dwell_game::touch::{TouchButton, TouchControls, TouchEvent};
use dwell_look::clouds::CloudQuality;
use dwell_look::fog::FogSettings;
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::{Channel, TransportKind};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::jobs::WebJobs;
use super::transport::{TransportEvent, WebRtcConn, WebTransportConn};
use crate::game::Scene;
use crate::game::{DayOverride, Game, LodSettings, Send};
use crate::net::netsim::{Direction, NetConditions, parse_net_conditions};
use crate::net::sdp::WebRtcInvite;
use crate::net::session::{Out, Session, SessionState};
use crate::pool::Pool;
use crate::render::Renderer;
use crate::render::compare::CompareView;
use crate::render::seam::{Selection, TerrainKind};
use crate::telemetry;

/// The most cells `Client::voxels` reads at once (about 2 M: a 128³ box).
const MAX_VOXEL_READ: u64 = 128 * 128 * 128;

type Events = Rc<RefCell<VecDeque<TransportEvent>>>;
/// The device key's signature of the join transcript, once the shell's promise settles.
type SignatureSlot = Rc<RefCell<Option<Result<[u8; 64], String>>>>;

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
    /// The renderer switch (`?renderer=`, compare mode): local builds only (the shell's build-time
    /// flag, RAY_MARCHED_RENDERER.md §3); deployed builds keep the mesher.
    renderer_switch: bool,
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
    /// workers, lod, lodColors, fullChunks, netsim, search, rendererSwitch, renderer, compare }`
    /// (`renderer`, `compare`: `?renderer=` and `?compare=`, taken only with `rendererSwitch`).
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
            renderer_switch: get("rendererSwitch").as_bool().unwrap_or(false),
        };
        let selection = options
            .renderer_switch
            .then(|| {
                let renderer = get("renderer").as_string()?;
                Selection::parse(&renderer, get("compare").as_string().as_deref())
            })
            .flatten()
            .unwrap_or_default();
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
        let mut renderer = Renderer::with_samples(
            device,
            queue,
            format,
            crate::render::samples_for(adapter.get_info().device_type),
        );
        renderer.resize(width, height, 1.0);
        renderer.set_lod_level_colors(options.lod_colors);
        renderer.select(selection);
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
                game.on_game_message(m, bytes.len(), now_ms);
            }
            let stats = session.stats();
            game.rtt_ms = stats.datagram_rtt_ms.or(stats.rtt_ms);
            game.motion_settings = s.motion;
            game.menu_open = s.menu_open;
            game.fov_setting = s.fov_setting;
            let touch = s.touch_used.then_some(s.touch.state);
            let perf = web_sys::window().and_then(|w| w.performance());
            let mut clock = || perf.as_ref().map_or(0.0, |p| p.now());
            game.frame(now_ms, &mut s.input, touch.as_ref(), &mut clock);
            for out in game.take_outbox() {
                match out {
                    Send::Control(m) => session.send_control(&m),
                    Send::Datagram(m) => session.send_game_datagram(&m),
                }
            }
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

    /// The drawing buffer's size: CSS pixels × the pixel ratio.
    pub fn resize(&self, css_width: f64, css_height: f64, pixel_ratio: f64) {
        let mut s = self.inner.borrow_mut();
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
        // Compare mode's keys (controls.ts): F7 the next view, F8 blink.
        if matches!(code, "F7" | "F8") && s.renderer.borrow().selection().compare() {
            if !repeat {
                let mut r = s.renderer.borrow_mut();
                let mut c = r.compare();
                if code == "F7" {
                    c.cycle();
                } else {
                    c.blink();
                }
                r.set_compare(c);
                drop(r);
                if let Some(g) = s.game.as_mut() {
                    g.sound(dwell_game::sound::SoundKind::Tool);
                }
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
        self.inner.borrow_mut().input.mouse_move(dx, dy);
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
    /// A `wheel` event: the hotbar, or with Alt held and the brush tool on, the brush's size.
    pub fn wheel(&self, delta_y: f64, alt: bool) {
        let mut s = self.inner.borrow_mut();
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
            .is_some_and(|g| g.tool(tool, now_ms))
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

    #[wasm_bindgen(js_name = setFog)]
    pub fn set_fog(&self, distance_m: f64, density: f64, height_m: f64) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_fog(FogSettings {
                distance_m,
                density,
                height_m,
            });
    }
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
    /// The water quality (0 Low, 1 Medium, 2 High; WATER_RENDERING.md §4).
    #[wasm_bindgen(js_name = setWaterQuality)]
    pub fn set_water_quality(&self, level: u32) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_water_quality(crate::render::water::WaterQuality::from_level(level));
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
    /// The Field of view setting: the vertical FOV in degrees (60–100, `dwell_lod::base_fov`).
    #[wasm_bindgen(js_name = setFov)]
    pub fn set_fov(&self, degrees: f64) {
        self.inner.borrow_mut().fov_setting = dwell_lod::sanitize_fov_setting(degrees);
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
    // ----- The renderer seam and compare mode (docs/RAY_MARCHED_RENDERER.md §3, §5) -----

    /// The terrain renderers this build offers for `?renderer=` (`bench --compare` reads it):
    /// `raster` everywhere; `march` with the renderer switch once the marcher draws terrain.
    pub fn renderers(&self) -> Array {
        let s = self.inner.borrow();
        let mut names = vec![TerrainKind::Raster.name()];
        if s.options.renderer_switch && crate::render::march::BUILT {
            names.push(TerrainKind::March.name());
        }
        names.into_iter().map(JsValue::from).collect()
    }
    /// The upscalers it offers: none yet beyond the browser's own stretch (A5).
    pub fn upscalers(&self) -> Array {
        ["none", "bilinear"]
            .into_iter()
            .map(JsValue::from)
            .collect()
    }
    /// What draws the terrain: `raster`, `march` or `compare raster | march`.
    pub fn renderer(&self) -> String {
        self.inner.borrow().renderer.borrow().selection().label()
    }
    /// Whether the renderer can be switched (a local build).
    #[wasm_bindgen(js_name = rendererSwitch)]
    pub fn renderer_switch(&self) -> bool {
        self.inner.borrow().options.renderer_switch
    }
    /// Switches renderer at runtime (`raster`, `march`, `compare`; `compare` names compare mode's
    /// right side, default `march`): the terrain is rebuilt from the client's data, no reload.
    /// False without the switch or for an unknown name.
    #[wasm_bindgen(js_name = setRenderer)]
    pub fn set_renderer(&self, name: &str, compare: Option<String>) -> bool {
        let mut s = self.inner.borrow_mut();
        if !s.options.renderer_switch {
            return false;
        }
        let Some(selection) = Selection::parse(name, compare.as_deref()) else {
            return false;
        };
        match &mut s.game {
            Some(g) => g.set_renderer(selection),
            None => {
                s.renderer.borrow_mut().select(selection);
            }
        }
        true
    }
    /// Compare mode's state: `{ renderer, view, divider, blinkRight, overlay: [left, right],
    /// difference: { pixels, coverage, depth, colour } | null }`, or null outside compare mode.
    #[wasm_bindgen(js_name = compareState)]
    pub fn compare_state(&self) -> JsValue {
        let s = self.inner.borrow();
        let r = s.renderer.borrow();
        let Some(overlay) = r.compare_overlay() else {
            return JsValue::NULL;
        };
        let c = r.compare();
        let o = Object::new();
        set(&o, "renderer", r.selection().label());
        set(&o, "view", c.view.name());
        set(&o, "divider", c.divider);
        set(&o, "blinkRight", c.blink_right);
        set(
            &o,
            "overlay",
            overlay
                .iter()
                .map(|l| JsValue::from(l.as_str()))
                .collect::<Array>(),
        );
        set(
            &o,
            "difference",
            r.difference_counts().map_or(JsValue::NULL, |d| {
                let v = Object::new();
                set(&v, "pixels", d.pixels);
                set(&v, "coverage", d.coverage);
                set(&v, "depth", d.depth);
                set(&v, "colour", d.colour);
                v.into()
            }),
        );
        o.into()
    }
    /// Sets compare mode's view (`split`, `blink`, `difference`; unchanged if unknown) and the
    /// divider (a share of the width; unchanged if not a number).
    #[wasm_bindgen(js_name = setCompare)]
    pub fn set_compare(&self, view: Option<String>, divider: Option<f64>) {
        let s = self.inner.borrow();
        let mut r = s.renderer.borrow_mut();
        let mut c = r.compare();
        if let Some(v) = view.as_deref().and_then(CompareView::parse) {
            c.view = v;
        }
        if let Some(d) = divider {
            c.set_divider(d);
        }
        r.set_compare(c);
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

    /// `{ phase: 'connecting' | 'handshaking' | 'joined' | 'rejected' | 'closed', … }`.
    #[wasm_bindgen(js_name = sessionState)]
    pub fn session_state(&self) -> JsValue {
        let s = self.inner.borrow();
        let o = Object::new();
        match s.session.as_ref().map(Session::state) {
            None => set(&o, "phase", "connecting"),
            Some(SessionState::Handshaking) => set(&o, "phase", "handshaking"),
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
                r.into()
            }),
        );
        set(&o, "flying", d.flying);
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
            let f = g.debug_state().feet.map(|v| v.floor() as i32);
            g.request_diff(f.map(|v| v.div_euclid(dwell_world::CHUNK_SIZE)));
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
        let size = self.renderer.borrow().size();
        let ratio = self.renderer.borrow().stats().pixel_ratio.max(0.01);
        game.viewport_css = (f64::from(size.0) / ratio, f64::from(size.1) / ratio);
        game.debug_visible = self.debug_visible;
        game.set_capabilities(region_edits, brushes, generator_version);
        game.day_override = self.options.day;
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
            self.renderer
                .borrow_mut()
                .render(&view, &|c| game.as_ref().is_none_or(|g| g.chunk_visible(c)));
        });
        let queue = self.renderer.borrow().queue().clone();
        telemetry::span("draw.present", || queue.present(frame));
    }
}
