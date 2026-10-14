//! The game client's JS API (Phase 24e): one [`Client`] per page owns the WebGPU canvas and the
//! renderer, the session and its transport, the game loop, the input state machines and the
//! worker jobs. The TS shell keeps the DOM (menus, HUD, settings, the master and rooms), forwards
//! input events, calls `frame` from `requestAnimationFrame` and draws what `hud()` returns.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::input::{EditAction, InputEvent, KeyboardMouseInput};
use dwell_game::touch::{TouchButton, TouchControls, TouchEvent};
use dwell_look::fog::FogSettings;
use dwell_look::sky_frame::{SkySwitches, sky_angle, sky_frame, sky_switches};
use dwell_physics::config::PlayerControllerConfig;
use dwell_protocol::{Channel, TransportKind};
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::jobs::WebJobs;
use super::transport::{TransportEvent, WebRtcConn, WebTransportConn};
use crate::game::{Game, LodSettings, Send};
use crate::net::netsim::{Direction, NetConditions, parse_net_conditions};
use crate::net::sdp::WebRtcInvite;
use crate::net::session::{Out, Session, SessionState};
use crate::pool::Pool;
use crate::render::Renderer;

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
    sky: SkySwitches,
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
    sky_setting_deg: f64,
    started_ms: Option<f64>,
    /// Input events for the shell (toggles, hotbar changes, flight), drained by `takeEvents`.
    shell_events: Vec<JsValue>,
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
    /// workers, lod, lodColors, fullChunks, netsim, search }`.
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
            sky: sky_switches(&get("search").as_string().unwrap_or_default()),
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
        let mut renderer = Renderer::with_samples(
            device,
            queue,
            format,
            crate::render::samples_for(adapter.get_info().device_type),
        );
        renderer.resize(width, height, 1.0);
        renderer.set_lod_level_colors(options.lod_colors);
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
            sky_setting_deg: 0.0,
            started_ms: None,
            shell_events: Vec::new(),
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
        let mut s = self.inner.borrow_mut();
        let s = &mut *s;
        s.pump_network(now_ms);
        if s.game.is_none() {
            s.start_game();
        }
        if let (Some(game), Some(session)) = (&mut s.game, &mut s.session) {
            while let Some((m, bytes)) = session.next_game() {
                game.on_game_message(m, bytes.len(), now_ms);
            }
            let stats = session.stats();
            game.rtt_ms = stats.datagram_rtt_ms.or(stats.rtt_ms);
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
        s.pump_network(now_ms);
        let start = *s.started_ms.get_or_insert(now_ms);
        s.renderer.borrow_mut().set_sky_frame(sky_frame(sky_angle(
            s.sky_setting_deg,
            s.options.sky,
            (now_ms - start) / 1000.0,
        )));
        s.draw();
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
        let k = s.input.key_down(code, repeat, ctrl, meta, time_stamp_ms);
        for e in k.events {
            s.on_input_event(e);
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
    #[wasm_bindgen(js_name = mouseDown)]
    pub fn mouse_down(&self, button: i16, now_ms: f64) {
        let mut s = self.inner.borrow_mut();
        if let Some(action) = s.input.mouse_down(button) {
            s.edit(action, now_ms);
        }
    }
    pub fn wheel(&self, delta_y: f64) {
        let mut s = self.inner.borrow_mut();
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
            s.touch.button_down(button, now_ms)
        } else {
            s.touch.button_up(button, now_ms)
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
    pub fn fly(&self, on: bool) {
        let mut s = self.inner.borrow_mut();
        if let Some(v) = s.input.flight.set(on) {
            s.shell_events.push(shell_event("flight", v));
        }
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
    #[wasm_bindgen(js_name = setSkyRotation)]
    pub fn set_sky_rotation(&self, degrees: f64) {
        self.inner.borrow_mut().sky_setting_deg = degrees;
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
        if let Some(g) = &mut self.inner.borrow_mut().game {
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

    fn on_input_event(&mut self, e: InputEvent) {
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
            TouchEvent::JumpPress(t) => {
                if let Some(v) = self.input.flight.jump_pressed(t) {
                    self.shell_events.push(shell_event("flight", v));
                }
            }
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
        let frame = match self.surface.get_current_texture() {
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
        self.renderer
            .borrow_mut()
            .render(&view, &|c| game.as_ref().is_none_or(|g| g.chunk_visible(c)));
        let queue = self.renderer.borrow().queue().clone();
        queue.present(frame);
    }
}
