//! The game client's JS API (Phase 24e): one [`Client`] per page owns the WebGPU canvas and the
//! renderer, the session and its transport, the game loop, the input state machines and the
//! worker jobs. The TS shell keeps the DOM (menus, HUD, settings, the master and rooms), forwards
//! input events, calls `frame` from `requestAnimationFrame` and draws what `hud()` returns.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use dwell_game::build_drag::Holder;
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
    /// The LOD colour map preview (`?lodmap=on`; ARCHITECTURE.md §6.6 "Colour map"): its rings on
    /// from the start, and the river valleys in the workers' LOD sections.
    lod_map: bool,
    netsim: Option<NetConditions>,
    /// `?time=`, `?moon=`, `?timespeed=` (the day/night cycle's dev switches).
    day: DayOverride,
    /// The page's query, for the dev switches read from it (`?outline=`).
    search: String,
    /// The loading screen on joining (ARCHITECTURE.md §6.3); the shell turns it off for
    /// `?loading=off` and the `dwell.loading` store.
    loading_screen: bool,
    /// The scene's samples per pixel asked for (the Anti-aliasing setting; `samples_for`).
    samples: u32,
    /// `prepareWorld(worldSeed: bigint, generatorVersion, worldgen: string, radiusScale: number) →
    /// Promise<Uint8Array[] | null>`: the world's maps (ADR 0049), from the shell's cache or built in
    /// workers; none: each instance computes its own. `worldgen` is the world's config as its server
    /// sent it (ADR 0074 B7; `""`: the build's ground, or the shell's own for its local world);
    /// `radiusScale` its size's (BIG_MAP.md M2b, 1 today's).
    prepare_world: Option<Function>,
}

/// The shell's answer to `prepareWorld`, once it arrives.
type MapsSlot = Rc<RefCell<Option<Result<Vec<Uint8Array>, String>>>>;

/// The world's maps on the way (ADR 0049): asked of the shell once joined, the game started when
/// they arrive ("Shaping the world…").
#[derive(Default)]
enum Shaping {
    #[default]
    NotAsked,
    Waiting(MapsSlot),
    /// Installed here; the bytes go to the workers.
    Ready(Vec<Uint8Array>),
}

struct Inner {
    /// Kept for the page's lifetime: Chromium's devices stop working once their instance is gone.
    _instance: wgpu::Instance,
    _adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Render scale and its upscaler (render/upscale.rs; shared/render/upscale.json): the scale
    /// per axis, the kind and RCAS's sharpness; and the last `resize`'s CSS size and pixel ratio.
    render_scale: (f64, crate::render::upscale::Upscaler, f32),
    last_resize: Option<(f64, f64, f64)>,
    renderer: Rc<RefCell<Renderer>>,
    input: KeyboardMouseInput,
    touch: TouchControls,
    touch_used: bool,
    options: Options,
    link: Option<Link>,
    events: Events,
    session: Option<Session>,
    /// The minimap and the explored cells it has covered (ADR 0068).
    minimap: dwell_game::minimap::Minimap,
    /// A registry definitions hash the player said not to ask about for this server (ADR 0038).
    trusted_registry: Option<u64>,
    netsim: Option<NetSim>,
    signature: SignatureSlot,
    game: Option<Game<Renderer>>,
    lod_settings: LodSettings,
    /// The Drifting flecks setting (docs/WATER_SURFACE_GRID.md §2c), for each game made.
    flow_particles: bool,
    /// The Waterfalls setting (docs/WATER_SURFACE_GRID.md §2e), for each game made.
    falls_shown: bool,
    /// The Explosions settings (docs/EXPLOSIVES.md §4): flash, smoke and dust, screen shake.
    blast_settings: crate::blasts::BlastSettings,
    /// Input events for the shell (toggles, hotbar changes, flight), drained by `takeEvents`.
    shell_events: Vec<JsValue>,
    /// The F3 overlay is on: kept here so it holds for a game started later (`?debug=1`).
    debug_visible: bool,
    /// The motion effects' settings and whether the game menu is open (docs/MOTION_EFFECTS.md).
    motion: dwell_look::motion::MotionSettings,
    menu_open: bool,
    /// The last controller frame's time (ms), for its look's dt.
    gamepad_ms: Option<f64>,
    /// The last place `find` found (docs/DEV_TOOLS.md "Finding places"), for `viewFound`.
    found: Option<crate::find::Found>,
    /// The Field of view setting (degrees, vertical).
    fov_setting: f64,
    /// The browser asks for reduced motion (the spyglass zooms at once).
    reduced_motion: bool,
    /// The Show your arms setting (docs/CHARACTERS.md §5).
    show_arms: bool,
    /// The Camera sway setting (Settings → Motion).
    camera_sway: bool,
    /// How long the camera rolls upright across the midplane (s; 0 snaps): `?crossingroll=` or
    /// `setCrossingRoll`, kept here for a game started later.
    crossing_roll: f64,
    /// The world's maps, before the game starts.
    shaping: Shaping,
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
    /// When the last `frame` ended (telemetry's `frame.gap`).
    last_end: std::cell::Cell<Option<f64>>,
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
            lod_map: lod_map_query(&get("search").as_string().unwrap_or_default()),
            netsim: parse_net_conditions(get("netsim").as_string().as_deref()),
            day: DayOverride::from_query(&get("search").as_string().unwrap_or_default()),
            search: get("search").as_string().unwrap_or_default(),
            loading_screen: get("loadingScreen").as_bool().unwrap_or(true),
            // The Anti-aliasing setting's samples per pixel (shared/render/antialias.json), fixed
            // for the renderer's life; absent: multisampled.
            samples: get("samples")
                .as_f64()
                .map_or(crate::render::SAMPLES, |v| v as u32),
            prepare_world: get("prepareWorld").dyn_into().ok(),
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
        crate::telemetry::mark("dwell:create:device");
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
            crate::render::samples_for(device_type, options.samples),
        );
        crate::telemetry::mark("dwell:create:renderer");
        renderer.set_adapter(device_type);
        renderer.resize(width, height, 1.0);
        renderer.set_lod_level_colors(options.lod_colors);
        renderer.set_lod_debug(&lod_debug_query(&options.search));
        // `?outline=<mode>` (docs/ART_DIRECTION.md §7): the ink outline, off without it.
        renderer.set_outline_query(&options.search);
        // `?pixelart=<preset>` (docs/proposals/PIXEL_ART_LOOK.md): the pixel-art preview, off
        // without it.
        renderer.set_pixel_art_query(&options.search);
        // `?lodshadows=off` (docs/DAY_NIGHT.md §12): the distant shadows off, for their cost.
        renderer.set_lod_shadows_query(&options.search);
        // `?crossingroll=<s>|snap` (docs/DEV_TOOLS.md): the crossing roll's duration.
        let crossing_roll = dwell_game::face_flip::roll_seconds_from_query(&options.search)
            .unwrap_or(dwell_game::face_flip::ROLL_SECONDS);
        let inner = Inner {
            _instance: instance,
            _adapter: adapter,
            surface,
            config,
            render_scale: (1.0, crate::render::upscale::Upscaler::Stretch, 1.0),
            last_resize: None,
            renderer: Rc::new(RefCell::new(renderer)),
            input: KeyboardMouseInput::new(),
            touch: TouchControls::new(),
            touch_used: false,
            options,
            link: None,
            events: Rc::default(),
            session: None,
            minimap: dwell_game::minimap::Minimap::default(),
            trusted_registry: None,
            netsim: None,
            signature: Rc::default(),
            game: None,
            flow_particles: true,
            falls_shown: true,
            blast_settings: crate::blasts::BlastSettings::default(),
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
            gamepad_ms: None,
            found: None,
            fov_setting: dwell_lod::frustum::VERTICAL_FOV,
            reduced_motion: false,
            show_arms: true,
            camera_sway: true,
            crossing_roll,
            shaping: Shaping::NotAsked,
        };
        Ok(Client {
            last_end: std::cell::Cell::new(None),
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    /// Resolves once the GPU has done the work sent so far: on a cold shader cache the browser
    /// compiles the renderer's pipelines for seconds after `create` (bug #71), the loading screen's
    /// 'shaders' stage (`client/src/ui/startStages.ts`).
    #[wasm_bindgen(js_name = gpuSettled)]
    pub fn gpu_settled(&self) -> js_sys::Promise {
        let queue = self.inner.borrow().renderer.borrow().queue.clone();
        js_sys::Promise::new(&mut |resolve, _reject| {
            queue.on_submitted_work_done(move || {
                let _ = resolve.call0(&JsValue::NULL);
            });
            // A fence even with nothing submitted yet: the callback answers after the pipelines.
            queue.submit([]);
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

    /// One animation frame: network, the game's ticks, and drawing. Whether it drew: a frame is
    /// skipped while the GPU is behind (render/pacing.rs), and the status line's frame rate counts
    /// only the drawn ones (it counted every animation frame, so a phone drawing 30 a second read
    /// 60, owner 2026-11-06).
    pub fn frame(&self, now_ms: f64) -> bool {
        let t0 = telemetry::now_ms();
        telemetry::with(|t| t.begin_frame(now_ms));
        // How late the animation frame's callback ran after its timestamp (`frame.late`), and the
        // time from the end of the last `frame` to this one's start (`frame.gap`: other script on
        // the main thread, and idle) (docs/DEV_TOOLS.md).
        telemetry::add("frame.late", (t0 - now_ms).max(0.0));
        if let Some(end) = self.last_end.get() {
            telemetry::add("frame.gap", t0 - end);
        }
        let mut s = self.inner.borrow_mut();
        let s = &mut *s;
        telemetry::span("net", || s.pump_network(now_ms));
        if s.game.is_none() {
            s.start_game();
        }
        // A still hold on the view in Place mode places and builds on (docs/ITEMS.md §5).
        if let Some(e) = s.touch.hold_due(now_ms) {
            s.on_touch_event(e, now_ms);
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
            // The host changed the world's game mode (ADR 0065): flight and brushes follow it.
            if session.take_abilities_changed()
                && let SessionState::Joined {
                    may_fly,
                    region_edits,
                    brushes,
                    ..
                } = session.state().clone()
            {
                game.set_capabilities(region_edits, brushes);
                game.set_vitals_rules(session.vitals_rules());
                game.set_instant_breaks(session.instant_breaks());
                game.set_structures(session.structures());
                game.set_structure_ops(
                    session.capability(dwell_protocol::capability_names::STRUCTURES_PLACE),
                );
                if let Some(v) = s.input.flight.set_allowed(may_fly) {
                    s.shell_events.push(shell_event("flightAllowed", v));
                }
            }
            let stats = session.stats();
            game.rtt_ms = stats.datagram_rtt_ms.or(stats.rtt_ms);
            game.motion_settings = s.motion;
            game.menu_open = s.menu_open;
            game.fov_setting = s.fov_setting;
            game.reduced_motion = s.reduced_motion;
            game.show_arms = s.show_arms;
            game.camera_sway = s.camera_sway;
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
        // The clouds drift by the world's clock once the server's ticks have arrived, read at the
        // render time (frozen while the look is paused, render_clock.rs).
        let look_ms = crate::render_clock::at(now_ms);
        let world = s.game.as_ref().and_then(|g| g.clock.seconds(look_ms));
        s.renderer
            .borrow_mut()
            .set_cloud_time(world.unwrap_or(look_ms / 1000.0));
        let drew = s.draw();
        let end = telemetry::now_ms();
        telemetry::add("frame.cpu", end - t0);
        self.last_end.set(Some(end));
        drew
    }

    /// The status line's performance figures (owner, 2026-11-06: what is chugging, not one number):
    /// the CPU's time per frame (`frame.cpu`, the last 60 frames' mean) and the GPU's (`gpu.frame`,
    /// the last 30 timed frames' mean; NaN where the browser can't time the GPU) in ms; then the
    /// share of animation frames skipped because the GPU was behind and for want of a surface, and
    /// the GPU's submit-to-done answer time (`pacing.done`, ms).
    pub fn perf(&self) -> Vec<f64> {
        telemetry::with(|t| {
            let cpu = t.recent_frame_mean("frame.cpu", 60).unwrap_or(f64::NAN);
            let gpu = t.recent_sample_mean("gpu.frame", 30).unwrap_or(f64::NAN);
            // The share of the last 60 animation frames skipped, by reason: the GPU behind
            // (pacing) and no surface texture; and the submit-to-done latency (ms).
            let paced = t.recent_frame_mean("draw.paced", 60).unwrap_or(0.0);
            let no_surface = t.recent_frame_mean("draw.no_surface", 60).unwrap_or(0.0);
            let done = t.recent_sample_mean("pacing.done", 30).unwrap_or(f64::NAN);
            vec![cpu, gpu, paced, no_surface, done]
        })
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

    /// The waterfalls near the eye as this client found them (docs/WATER_SURFACE_GRID.md §2e;
    /// DEV_TOOLS.md): 8 numbers each, its high column's x and z, its way (0 +x, 1 −x, 2 +z, 3 −z),
    /// width (columns), lip and landing (world y), flow per metre (m²/s) and current over the lip (m/s).
    #[wasm_bindgen(js_name = waterFalls)]
    pub fn water_falls(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return Vec::new();
        };
        g.falls()
            .iter()
            .flat_map(|f| {
                [
                    f64::from(f.x),
                    f64::from(f.z),
                    f64::from(f.dir),
                    f64::from(f.width),
                    f.face.y(f.lip),
                    f.face.y(f.land),
                    f.flow,
                    f.speed,
                ]
            })
            .collect()
    }

    /// The water's current (m/s along x and z) at a world point as this client has it, and how many
    /// flecks drift near the eye (docs/WATER_SURFACE_GRID.md §2c; DEV_TOOLS.md).
    #[wasm_bindgen(js_name = waterFlowAt)]
    pub fn water_flow_at(&self, x: f64, y: f64, z: f64) -> Vec<f64> {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else {
            return Vec::new();
        };
        let [u, v] = g.current_at([x, y, z]);
        let mut out = vec![f64::from(u), f64::from(v), g.fleck_count() as f64];
        out.extend(g.fleck_positions().into_iter().take(3).flatten());
        out
    }

    /// The nearest loaded cell matching `query` (`crate::find`; docs/DEV_TOOLS.md "Finding
    /// places") from world point (x, y, z) within `radius` m: a block or fluid by name
    /// (`dwell:sand`, `water`) or `fluid`; with `surface` open above it, at least `min_depth` m
    /// deep, `min_area` cells joined at its row and its current `min_current` m/s; on face `face`
    /// (1 face A, -1 face B, 0 the start point's, 2 either). `{ cell, at, surfaceY, face, block,
    /// depth, area, distance, current, flow }` (`at` its surface over the cell's centre; `face` 1
    /// or -1), or null; with `keep`, kept for `viewFound`. Throws for a query naming no block.
    #[allow(clippy::too_many_arguments)]
    pub fn find(
        &self,
        query: &str,
        x: f64,
        y: f64,
        z: f64,
        radius: f64,
        surface: bool,
        min_depth: f64,
        min_area: u32,
        min_current: f64,
        face: i32,
        keep: bool,
    ) -> Result<JsValue, JsError> {
        use crate::find::{Matcher, Query};
        let what = Matcher::parse(query).map_err(err)?;
        let q = Query {
            radius_m: radius,
            surface,
            min_depth_m: min_depth,
            min_area: min_area.max(1),
            min_current: min_current as f32,
            face: match face {
                0 => Some(dwell_world::FaceFrame::at_height(y)),
                1 | -1 => Some(dwell_world::FaceFrame::of_sign(f64::from(face))),
                _ => None,
            },
            ..Query::new(what)
        };
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else {
            return Ok(JsValue::NULL);
        };
        let found = crate::find::find(g.world(), [x, y, z], &q);
        if keep {
            s.found = found;
        }
        let Some(f) = found else {
            return Ok(JsValue::NULL);
        };
        let o = Object::new();
        let arr = |v: &[f64]| Array::from_iter(v.iter().map(|&n| JsValue::from(n)));
        set(&o, "cell", arr(&f.cell.map(f64::from)));
        set(&o, "at", arr(&f.point()));
        set(&o, "surfaceY", f.surface_y);
        set(&o, "face", f.face.sign());
        set(&o, "block", dwell_blocks::info(f.state).name);
        set(&o, "depth", f.depth_m);
        set(&o, "area", f.area);
        set(&o, "distance", f.distance_m);
        set(&o, "current", arr(&f.current.map(f64::from)));
        set(&o, "flow", arr(&f.flow.map(f64::from)));
        Ok(o.into())
    }

    /// Places the camera `height` m over the last place `find` found (along its face's up), looking
    /// `aim` (`downstream`, `upstream`, `across` its flow; `toward` the point (tx, ty, tz); else
    /// heading `yaw`), turned `turn` degrees more, at `pitch` (degrees toward the face's up; NaN:
    /// toward the point with `toward`, else 0). `{ position, yaw, pitch }`; throws with nothing found, or for a flow still there.
    #[wasm_bindgen(js_name = viewFound)]
    #[allow(clippy::too_many_arguments)]
    pub fn view_found(
        &self,
        height: f64,
        aim: &str,
        yaw: f64,
        tx: f64,
        ty: f64,
        tz: f64,
        pitch: f64,
        turn: f64,
    ) -> Result<JsValue, JsError> {
        use crate::find::Aim;
        let mut s = self.inner.borrow_mut();
        let found = s
            .found
            .ok_or_else(|| JsError::new("viewFound: nothing found yet"))?;
        let how = match aim {
            "toward" => Aim::Toward([tx, ty, tz]),
            "" | "yaw" => Aim::Yaw(yaw),
            other => Aim::parse(other).ok_or_else(|| {
                JsError::new(&format!(
                    "viewFound: unknown aim {other:?} (downstream, upstream, across, toward, yaw)"
                ))
            })?,
        };
        let eye = found.over(height);
        let (yaw, toward_pitch) = crate::find::aim(&found, eye, how).map_err(err)?;
        let yaw = (yaw + turn).rem_euclid(360.0);
        let pitch = if pitch.is_nan() {
            toward_pitch.unwrap_or(0.0)
        } else {
            pitch
        };
        let Some(g) = &mut s.game else {
            return Err(JsError::new("viewFound: not playing"));
        };
        g.set_view(Some(crate::game::ViewOverride {
            eye,
            yaw_deg: yaw,
            pitch_deg: pitch,
        }));
        let o = Object::new();
        set(
            &o,
            "position",
            Array::from_iter(eye.iter().map(|&n| JsValue::from(n))),
        );
        set(&o, "yaw", yaw);
        set(&o, "pitch", pitch);
        Ok(o.into())
    }

    /// The placed camera (`placeCamera`, `viewFound`): `{ position, yaw, pitch }` (degrees, as
    /// `placeCamera` takes them), or null while the view is the player's eyes.
    pub fn camera(&self) -> JsValue {
        let s = self.inner.borrow();
        let Some(v) = s.game.as_ref().and_then(|g| g.view()) else {
            return JsValue::NULL;
        };
        let o = Object::new();
        set(
            &o,
            "position",
            Array::from_iter(v.eye.iter().map(|&n| JsValue::from(n))),
        );
        set(&o, "yaw", v.yaw_deg);
        set(&o, "pitch", v.pitch_deg);
        o.into()
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
    /// With a render scale under 100 % the browser's stretch draws a smaller buffer, and an
    /// upscale pass a full-size one over a smaller scene (`render::upscale::sizes`).
    pub fn resize(&self, css_width: f64, css_height: f64, pixel_ratio: f64) {
        let mut s = self.inner.borrow_mut();
        s.last_resize = Some((css_width, css_height, pixel_ratio));
        let adapter = s.renderer.borrow().adapter();
        let ratio = crate::render::pixel_ratio_for(adapter, pixel_ratio);
        let (scale, upscaler, sharpness) = s.render_scale;
        let sizes = crate::render::upscale::sizes(
            (css_width, css_height),
            ratio,
            scale,
            upscaler,
            crate::render::is_cpu(adapter),
        );
        let (w, h) = sizes.canvas;
        if (w, h) != (s.config.width, s.config.height) {
            s.config.width = w;
            s.config.height = h;
            let renderer = s.renderer.clone();
            s.surface.configure(renderer.borrow().device(), &s.config);
        }
        let mut r = s.renderer.borrow_mut();
        r.resize(w, h, sizes.canvas_ratio);
        r.set_upscale(upscaler, sizes.render, sharpness);
        drop(r);
        if let Some(g) = &mut s.game {
            g.viewport_css = (css_width, css_height);
        }
    }

    /// Render scale (0.5–1 per axis) and its upscaler by name (shared/render/upscale.json's ids,
    /// or `stretch-pass`), RCAS's strength in stops; resizes with the last size. False (and
    /// nothing changed) for an unknown upscaler.
    #[wasm_bindgen(js_name = setRenderScale)]
    pub fn set_render_scale(&self, scale: f64, upscaler: &str, sharpen_stops: f64) -> bool {
        let Some(kind) = crate::render::upscale::Upscaler::from_name(upscaler) else {
            return false;
        };
        let last = {
            let mut s = self.inner.borrow_mut();
            s.render_scale = (
                crate::render::upscale::clamp_scale(scale),
                kind,
                crate::render::upscale::sharpness(sharpen_stops),
            );
            s.last_resize
        };
        if let Some((w, h, r)) = last {
            self.resize(w, h, r);
        }
        true
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
        let mut s = self.inner.borrow_mut();
        s.input.blur();
        // A button released while the window was away sends no mouseup here.
        if let Some(g) = &mut s.game {
            g.release_place(Holder::Mouse);
        }
    }
    /// Presses or releases a key programmatically (tests and automation).
    pub fn press(&self, code: &str, down: bool) {
        self.inner.borrow_mut().input.set_key(code, down);
    }
    /// The chopping prototype's crosshair aim (dev `?chop=v2`, docs/WORK_QUEUE.md item 7).
    #[wasm_bindgen(js_name = setChopAim)]
    pub fn set_chop_aim(&self, on: bool) {
        crate::chop::set_crosshair_aim(on);
    }
    #[wasm_bindgen(js_name = setPointerLocked)]
    pub fn set_pointer_locked(&self, locked: bool) {
        let mut s = self.inner.borrow_mut();
        s.input.pointer_locked = locked;
        if !locked && let Some(g) = &mut s.game {
            g.release_place(Holder::Mouse);
        }
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
            s.edit_held(action, Holder::Mouse, now_ms);
        }
    }
    /// A `mouseup`: letting go of Place stops building on (docs/ITEMS.md §5).
    #[wasm_bindgen(js_name = mouseUp)]
    pub fn mouse_up(&self, button: i16) {
        let mut s = self.inner.borrow_mut();
        if s.input.mouse_down(button) == Some(EditAction::Place)
            && let Some(g) = &mut s.game
        {
            g.release_place(Holder::Mouse);
        }
    }
    /// The build mode's label (`Line`, `Flat`; data), for the HUD and the touch Build button.
    #[wasm_bindgen(js_name = buildMode)]
    pub fn build_mode(&self) -> String {
        self.inner.borrow().game.as_ref().map_or_else(
            || dwell_game::build_drag::BUILD_MODES[0].label.to_owned(),
            |g| g.drag.mode().label.to_owned(),
        )
    }
    /// A `wheel` event: the hotbar, or with Alt held and the brush tool on, the brush's size, or
    /// while the spyglass is zoomed its steps (up zooms in; docs/SPYGLASS.md).
    pub fn wheel(&self, delta_x: f64, delta_y: f64, alt: bool, now_ms: f64) {
        let mut s = self.inner.borrow_mut();
        let s = &mut *s;
        let (step_y, step_x) = s.input.wheel(delta_x, delta_y, now_ms);
        let Some(g) = &mut s.game else { return };
        // A sideways swipe (a touchpad's `deltaX`) only steps the hotbar (docs/ITEMS.md §6).
        let step = match (step_y, step_x) {
            (Some(step), _) => {
                if !alt && g.spyglass.on() {
                    if g.spyglass.cycle(-step, &dwell_look::spyglass::SPYGLASS) {
                        g.sound(dwell_game::sound::SoundKind::Tool);
                    }
                    return;
                }
                if alt && g.brush_wheel(step) {
                    return;
                }
                step
            }
            (None, Some(step)) => step,
            (None, None) => return,
        };
        if let Some(slot) = g.scroll_hotbar(step) {
            s.shell_events.push(shell_event("slot", slot as u32));
        }
    }
    /// A controller frame (docs/GAMEPAD.md): the standard mapping's button values and axes, read
    /// by the shell from `navigator.getGamepads()` every animation frame. While playing (in a world,
    /// no menu open) its sticks move and turn and its presses act; returns the names of the actions
    /// pressed this frame that are the shell's to do (`pause`).
    pub fn gamepad(&self, buttons: &[f32], axes: &[f32], now_ms: f64) -> Vec<String> {
        use dwell_game::gamepad::PlayAction as A;
        let mut s = self.inner.borrow_mut();
        // A long gap (the tab hidden, the first frame) turns nothing.
        let dt = s
            .gamepad_ms
            .map_or(0.0, |t| ((now_ms - t) / 1000.0).clamp(0.0, 0.1));
        s.gamepad_ms = Some(now_ms);
        let playing = s.game.is_some() && !s.menu_open;
        let scale = s
            .game
            .as_ref()
            .map_or(1.0, |g| g.spyglass_view().look_scale);
        let actions = s.input.gamepad_frame(buttons, axes, dt, playing, scale);
        let mut shell = Vec::new();
        for a in actions {
            match a {
                A::Break => {
                    s.edit(EditAction::Break, now_ms);
                }
                A::Place => {
                    s.edit_held(EditAction::Place, Holder::Gamepad, now_ms);
                }
                A::BuildMode => s.cycle_build_mode(),
                A::HotbarNext | A::HotbarPrev => {
                    let step = if a == A::HotbarNext { 1 } else { -1 };
                    if let Some(g) = &mut s.game
                        && let Some(slot) = g.scroll_hotbar(step)
                    {
                        s.shell_events.push(shell_event("slot", slot as u32));
                    }
                }
                A::Fly => {
                    if let Some(on) = s.input.flight.toggle() {
                        s.on_input_event(InputEvent::FlightChanged(on), now_ms);
                    }
                }
                A::FlyFaster | A::FlySlower => {
                    let step = if a == A::FlyFaster { 1.0 } else { -1.0 };
                    let level = f64::from(s.input.flight.speed_level()) + step;
                    if s.input.flight.flying()
                        && let Some(l) = s.input.flight.set_speed_level(level)
                    {
                        s.shell_events.push(shell_event("flySpeed", l));
                    }
                }
                A::Spyglass => {
                    if let Some(g) = s.game.as_mut() {
                        g.spyglass.toggle(&dwell_look::spyglass::SPYGLASS);
                        g.sound(dwell_game::sound::SoundKind::Tool);
                    }
                }
                // The big map is the shell's (ADR 0068), as M is; the F4 debug map stays a key.
                A::Map | A::Pause | A::Inventory => shell.push(a.name().to_owned()),
                A::Jump | A::Crouch | A::Run => {}
            }
        }
        // Place let go (or a menu opened): building on stops.
        if (!playing || !s.input.gamepad.down(A::Place))
            && let Some(g) = &mut s.game
        {
            g.release_place(Holder::Gamepad);
        }
        shell
    }
    /// The controller went away: nothing it held is held.
    #[wasm_bindgen(js_name = gamepadDisconnected)]
    pub fn gamepad_disconnected(&self) {
        let mut s = self.inner.borrow_mut();
        s.input.gamepad.disconnect();
        s.gamepad_ms = None;
    }
    /// Puts a play action on a button (Settings → Controller; names from shared/input/gamepad.json):
    /// the action that had the button takes the old one. False for an unknown name.
    #[wasm_bindgen(js_name = setGamepadBinding)]
    pub fn set_gamepad_binding(&self, action: &str, button: &str) -> bool {
        self.inner.borrow_mut().input.gamepad.bind(action, button)
    }
    /// Every play action back on its default button.
    #[wasm_bindgen(js_name = resetGamepadBindings)]
    pub fn reset_gamepad_bindings(&self) {
        self.inner.borrow_mut().input.gamepad.reset_bindings();
    }
    /// The player's look speed (× the data's) and whether up on the stick looks down.
    #[wasm_bindgen(js_name = setGamepadLook)]
    pub fn set_gamepad_look(&self, speed: f64, invert_y: bool) {
        let mut s = self.inner.borrow_mut();
        s.input.gamepad.look_speed = speed.clamp(0.1, 4.0);
        s.input.gamepad.invert_y = invert_y;
    }
    /// The button a play action is on (its name), for Settings.
    #[wasm_bindgen(js_name = gamepadBinding)]
    pub fn gamepad_binding(&self, action: &str) -> Option<String> {
        let a = dwell_game::gamepad::PlayAction::by_name(action)?;
        Some(self.inner.borrow().input.gamepad.binding(a).name.to_owned())
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
    /// Holds the axe's swing (`vertical` or level) at clip time `t` s on your arms and every
    /// stand-in body for the dev tools (`__dwell.holdSwing`, the bench's `clip-strip`); a negative
    /// `t` releases it.
    #[wasm_bindgen(js_name = holdSwing)]
    pub fn hold_swing(&self, vertical: bool, t: f64) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.hold_swing((t >= 0.0).then_some((vertical, t)));
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
    /// No finger is on the screen (or the page lost focus): fingers whose lift never arrived are
    /// released (BUGS #57, `TouchControls::all_lifted`).
    #[wasm_bindgen(js_name = touchesLifted)]
    pub fn touches_lifted(&self) {
        self.inner.borrow_mut().touch.all_lifted();
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
    /// The listener's surroundings for the ambience beds (docs/AUDIO.md §2c): 9 numbers in
    /// `dwell_game::ambience::INPUTS` order; empty before the game runs.
    pub fn ambience(&self) -> Vec<f64> {
        self.inner
            .borrow()
            .game
            .as_ref()
            .map_or_else(Vec::new, |g| g.ambience().to_vec())
    }

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
    /// Presses (`on`) or lets go of Place as a held right button does: pressed, it places and builds
    /// on in the build mode while held (docs/ITEMS.md §5); true if it placed.
    #[wasm_bindgen(js_name = holdPlace)]
    pub fn hold_place(&self, on: bool, now_ms: f64) -> bool {
        let mut s = self.inner.borrow_mut();
        if on {
            return s.edit_held(EditAction::Place, Holder::Mouse, now_ms);
        }
        if let Some(g) = &mut s.game {
            g.release_place(Holder::Mouse);
        }
        false
    }
    /// The next build mode, as its key does: its label.
    #[wasm_bindgen(js_name = cycleBuildMode)]
    pub fn cycle_build_mode(&self) -> String {
        let mut s = self.inner.borrow_mut();
        s.cycle_build_mode();
        s.game
            .as_ref()
            .map_or_else(String::new, |g| g.drag.mode().label.to_owned())
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
            // A world that uses items (ADR 0066): the hotbar is the inventory's first slots.
            let slot = g.select_hotbar(i64::from(slot));
            s.shell_events.push(shell_event("slot", slot as u32));
        }
    }
    /// The palette's item names (`shared/items/items.json`), in `palette()`'s order.
    #[wasm_bindgen(js_name = paletteItems)]
    pub fn palette_items(&self) -> Vec<String> {
        dwell_game::block_interaction::palette()
            .into_iter()
            .map(|p| p.item.to_owned())
            .collect()
    }
    /// The creative hotbar's picks (docs/ITEMS.md §6a): each slot's palette index.
    #[wasm_bindgen(js_name = creativeHotbar)]
    pub fn creative_hotbar(&self) -> Vec<u32> {
        self.inner
            .borrow()
            .game
            .as_ref()
            .map_or_else(Vec::new, |g| {
                g.creative.picks().iter().map(|&i| i as u32).collect()
            })
    }
    /// Puts palette item `palette_index` in creative hotbar slot `slot` (the creative inventory
    /// view); false if either is out of range.
    #[wasm_bindgen(js_name = setCreativeSlot)]
    pub fn set_creative_slot(&self, slot: u32, palette_index: u32) -> bool {
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .is_some_and(|g| g.set_creative_slot(slot as usize, palette_index as usize))
    }
    /// Moves items between inventory slots (the backpack screen; ADR 0066): `count` 0 moves all.
    #[wasm_bindgen(js_name = moveItems)]
    pub fn move_items(&self, from: u8, to: u8, count: u16) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            g.move_items(from, to, count);
        }
    }
    /// Makes recipe `name` once (the backpack's Combine section); false if the server doesn't make
    /// recipes (an older server) or there is no game.
    pub fn combine(&self, name: &str) -> bool {
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .is_some_and(|g| g.combine(name))
    }
    /// Whether the server makes recipes (shows the Combine section).
    pub fn combines(&self) -> bool {
        self.inner
            .borrow()
            .game
            .as_ref()
            .is_some_and(|g| g.combines())
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
    /// Whether the world's mode places saved structures (`structures`, creative only for now):
    /// the creative inventory's Structures tab shows only then (STRUCTURES.md §9).
    #[wasm_bindgen(js_name = structuresAllowed)]
    pub fn structures_allowed(&self) -> bool {
        self.inner
            .borrow()
            .session
            .as_ref()
            .is_some_and(|s| s.structures())
    }
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
    /// The Anti-aliasing setting's FXAA pass and the Anisotropic filtering setting (the block
    /// atlas's maxAnisotropy, 1–16), live (shared/render/antialias.json; render/mod.rs
    /// `set_antialias`).
    #[wasm_bindgen(js_name = setAntialias)]
    pub fn set_antialias(&self, fxaa: bool, anisotropy: u32) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_antialias(fxaa, anisotropy.min(u32::from(u16::MAX)) as u16);
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
    /// The Model detail setting: a level of shared/models/models.json's `detail.levels`, by id
    /// (anything else: the data's default; ADR 0070 §10).
    #[wasm_bindgen(js_name = setModelDetail)]
    pub fn set_model_detail(&self, level: &str) {
        let k = dwell_models::detail_level(
            dwell_models::DETAIL_LEVELS,
            level,
            dwell_models::DETAIL_DEFAULT,
        );
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_model_detail(k);
    }
    /// Your first-person arms from the rigid parts (`true`, `?fparms=rigid`: a dev switch for
    /// comparison) or the player model (docs/CHARACTERS.md §11.10).
    #[wasm_bindgen(js_name = setRigidArms)]
    pub fn set_rigid_arms(&self, rigid: bool) {
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_rigid_arms(rigid);
    }
    /// The Light shafts setting: `off`, `low` or `high` (anything else: high; ADR 0064).
    #[wasm_bindgen(js_name = setShafts)]
    pub fn set_shafts(&self, quality: &str) {
        use dwell_look::shafts::ShaftQuality;
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_shafts(ShaftQuality::from_name(quality).unwrap_or(ShaftQuality::High));
    }
    /// The Bloom setting: `off`, `low` or `high` (anything else: high; WORK_QUEUE item 19).
    #[wasm_bindgen(js_name = setBloom)]
    pub fn set_bloom(&self, quality: &str) {
        use dwell_look::bloom::BloomQuality;
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_bloom(BloomQuality::from_name(quality).unwrap_or(BloomQuality::High));
    }
    /// The run-time plumes' dev tools (`__dwell.plume`, docs/CLOUDS.md §8.7;
    /// `dwell_look::plumes::dev`): `list`, `add` [x, y, z, radius?, height?, density?, riseMps?],
    /// `near` [dx, dz], `move` [x, y, z], `remove`, `clear`; JSON back.
    #[wasm_bindgen(js_name = plumeDev)]
    pub fn plume_dev(&self, op: &str, name: &str, args: Vec<f64>) -> String {
        let inner = self.inner.borrow();
        let mut r = inner.renderer.borrow_mut();
        let eye = r.eye_position();
        dwell_look::plumes::dev(&mut r.plumes, op, name, &args, eye)
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
    /// Whether this device is a phone (the shell's `prefersTouch`): the water's reflections draw
    /// the phone's cheaper tiers (`shared/look/reflections.json` `phone`).
    #[wasm_bindgen(js_name = setPhone)]
    pub fn set_phone(&self, phone: bool) {
        self.inner.borrow().renderer.borrow_mut().set_phone(phone);
    }
    /// How moving water shows its flow (docs/WATER_SURFACE_GRID.md §2c, §2e, the Water settings):
    /// ripples carried by the current, foam streaks, drifting particles, waterfalls.
    #[wasm_bindgen(js_name = setWaterFlow)]
    pub fn set_water_flow(&self, ripples: bool, foam: bool, particles: bool, falls: bool) {
        let s = self.inner.borrow();
        let switches = (u32::from(ripples) * crate::render::flow::FLOW_RIPPLES)
            | (u32::from(foam) * crate::render::flow::FLOW_FOAM);
        s.renderer.borrow_mut().set_flow_switches(switches);
        drop(s);
        let mut s = self.inner.borrow_mut();
        if let Some(g) = &mut s.game {
            g.set_flow_particles(particles);
            g.set_falls_shown(falls);
        }
        s.flow_particles = particles;
        s.falls_shown = falls;
    }
    /// Which blast effects show (docs/EXPLOSIVES.md §4, the Explosions settings): the flash, smoke
    /// and dust, the screen shake.
    #[wasm_bindgen(js_name = setExplosions)]
    pub fn set_explosions(&self, flash: bool, smoke: bool, shake: bool) {
        let mut s = self.inner.borrow_mut();
        s.blast_settings = crate::blasts::BlastSettings {
            flash,
            smoke,
            shake,
            cpu: false,
        };
        let now = s.blast_settings_now();
        if let Some(g) = &mut s.game {
            g.set_blast_settings(now);
        }
    }
    /// The blasts whose effects are still going (docs/EXPLOSIVES.md; DEV_TOOLS.md): 4 numbers each,
    /// its place (world x, y, z) and its age (s).
    /// The creatures drawn (ADR 0073; DEV_TOOLS.md): 6 numbers each, the feet (world x, y, z),
    /// the heading (degrees), the speed (m/s) and 1 for an NPC (else 0).
    #[wasm_bindgen(js_name = creatures)]
    pub fn creatures(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return Vec::new();
        };
        g.creatures
            .shown()
            .into_iter()
            .flat_map(|(p, yaw, speed, npc)| {
                [
                    p[0],
                    p[1],
                    p[2],
                    yaw,
                    f64::from(speed),
                    f64::from(u8::from(npc)),
                ]
            })
            .collect()
    }
    /// The far ecology the server last told of (docs/ECOLOGY.md §9; DEV_TOOLS.md): `{ day, groups:
    /// [{ id, face: "a" | "b", species, x, z, size }] }` (face-local metres), or null before the
    /// first (a server without an ecology, or an older one, never sends one).
    #[wasm_bindgen(js_name = ecology)]
    pub fn ecology(&self) -> JsValue {
        let s = self.inner.borrow();
        let Some(m) = s.game.as_ref().and_then(|g| g.ecology.as_ref()) else {
            return JsValue::NULL;
        };
        let groups: Array = m
            .groups
            .iter()
            .map(|g| {
                let o = Object::new();
                set(&o, "id", g.id);
                set(&o, "face", if g.face == 0 { "a" } else { "b" });
                set(&o, "species", g.species.as_str());
                set(&o, "x", g.x);
                set(&o, "z", g.z);
                set(&o, "size", g.size);
                JsValue::from(o)
            })
            .collect();
        let o = Object::new();
        set(&o, "day", m.day);
        set(&o, "groups", groups);
        o.into()
    }
    #[wasm_bindgen(js_name = blasts)]
    pub fn blasts(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return Vec::new();
        };
        g.blasts(telemetry::now_ms())
            .into_iter()
            .flat_map(|(p, age)| [p[0], p[1], p[2], age])
            .collect()
    }
    /// Fixed points (x, y, z triples, m) the streaming probe reports how they are drawn
    /// (`probe.target`, then `probe.control`; the bench's `probe` step). True when in a world.
    #[wasm_bindgen(js_name = setProbe)]
    pub fn set_probe(&self, points: Vec<f64>) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.probe_points = points.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
        true
    }
    /// The distant terrain (LOD) drawn or not, a debug switch (`?lod=off`, `__dwell.setLod`): the
    /// full-detail chunks alone show what they hold. True when in a world.
    #[wasm_bindgen(js_name = setLod)]
    pub fn set_lod(&self, on: bool) -> bool {
        let mut s = self.inner.borrow_mut();
        let Some(g) = &mut s.game else { return false };
        g.lod_drawn = on;
        true
    }
    /// The LOD colour map preview on or off (`__dwell.setLodMap`, the bench; `?lodmap=on` starts
    /// it on). Off on a CPU adapter. The river valleys follow `?lodmap=on` only (the workers' LOD).
    #[wasm_bindgen(js_name = setLodMap)]
    pub fn set_lod_map(&self, on: bool) -> bool {
        let mut inner = self.inner.borrow_mut();
        let on = on && inner.lod_map_supported();
        if let Some(game) = inner.game.as_mut() {
            game.set_lod_map(on);
        }
        on
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
    /// Forces the near water's GPU heightfield on (1) or off (0) whatever the tier says, or back
    /// to the tier's (any other value): `?heightfield=`, a dev tool (docs/DEV_TOOLS.md). Chunks
    /// meshed after it follow it, so it is set before the world streams in.
    #[wasm_bindgen(js_name = setHeightfield)]
    pub fn set_heightfield(&self, mode: i32) {
        let on = match mode {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_heightfield_override(on);
    }
    /// Forces the leaf cards on (1) or off (0), or back to the tier's (any other value): `?cards=`,
    /// a dev tool (docs/DEV_TOOLS.md). Chunks meshed after it follow it, so it is set before the
    /// world streams in.
    #[wasm_bindgen(js_name = setCards)]
    pub fn set_cards(&self, mode: i32) {
        let on = match mode {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        self.inner
            .borrow()
            .renderer
            .borrow_mut()
            .set_cards_override(on);
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
    /// How long the camera rolls upright after crossing the midplane (s; 0 snaps at once), as
    /// `?crossingroll=` sets it: the experiment switch (docs/DEV_TOOLS.md). False (unchanged) for a
    /// value outside 0..=5 s.
    #[wasm_bindgen(js_name = setCrossingRoll)]
    pub fn set_crossing_roll(&self, seconds: f64) -> bool {
        let mut s = self.inner.borrow_mut();
        if dwell_game::face_flip::valid_roll_seconds(seconds).is_none() {
            return false;
        }
        s.crossing_roll = seconds;
        if let Some(g) = &mut s.game {
            g.set_crossing_roll(seconds);
        }
        true
    }
    /// The crossing roll's duration (s).
    #[wasm_bindgen(js_name = crossingRoll)]
    pub fn crossing_roll(&self) -> f64 {
        self.inner.borrow().crossing_roll
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
    /// The Camera sway setting (Settings → Motion; docs/CHARACTERS.md §5.2): the view's sway with
    /// a swing of the axe. Off: the view holds still (the aim never sways either way).
    #[wasm_bindgen(js_name = setCameraSway)]
    pub fn set_camera_sway(&self, on: bool) {
        self.inner.borrow_mut().camera_sway = on;
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
    /// Pauses or resumes the look (a dev switch: `?pause=1`, `__dwell.pause`, Settings → Debug):
    /// every drawn animation stands still at one instant (render_clock.rs).
    #[wasm_bindgen(js_name = setLookPaused)]
    pub fn set_look_paused(&self, on: bool) {
        crate::render_clock::set_paused(on);
    }
    /// Whether the look is paused.
    #[wasm_bindgen(js_name = lookPaused)]
    pub fn look_paused(&self) -> bool {
        crate::render_clock::paused()
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
            // Stamina and air (ADR 0069), shares of full: null in a mode without them.
            match g.hud.vitals {
                Some(v) => {
                    let b = Object::new();
                    let share = |x: Option<f32>| x.map_or(JsValue::NULL, JsValue::from);
                    set(&b, "stamina", share(v.stamina));
                    set(&b, "oxygen", share(v.oxygen));
                    set(&b, "exhausted", v.exhausted);
                    set(&o, "vitals", b);
                }
                None => set(&o, "vitals", JsValue::NULL),
            }
            set(
                &o,
                "debug",
                g.hud.debug.as_deref().map_or(JsValue::NULL, JsValue::from),
            );
            set(&o, "damaged", std::mem::take(&mut g.hud.damaged));
            // The burning edge (shared/fluids/lava.json `burnEdge`): its opacity now and colour.
            let edge = dwell_lava::DATA.burn_edge;
            set(&o, "burn", g.hud.burn * f64::from(edge.opacity));
            set(&o, "burnColor", edge.color);
            set(&o, "region", g.region.status().as_str());
            // The held axe's swing hint (ADR 0067): null without one.
            set(
                &o,
                "chop",
                g.chop_hint().map_or(JsValue::NULL, JsValue::from),
            );
            // The swing's line through the crosshair (`level`, `vertical`; null: no tool held).
            set(
                &o,
                "swing",
                g.swing_line().map_or(JsValue::NULL, JsValue::from),
            );
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
            // The inventory (ADR 0066): null from a server without, or in a mode with unlimited
            // blocks; else every slot ({ item, count } or null), the held one and the wallet.
            match g.loose.inventory.as_ref().filter(|(uses, _)| *uses) {
                Some((_, inv)) => {
                    let i = Object::new();
                    let slots: Array = inv
                        .slots
                        .iter()
                        .map(|s| match s {
                            Some(s) => {
                                let o = Object::new();
                                set(&o, "item", s.item.as_str());
                                set(&o, "count", u32::from(s.count));
                                set(&o, "wear", u32::from(s.wear));
                                JsValue::from(o)
                            }
                            None => JsValue::NULL,
                        })
                        .collect();
                    set(&i, "slots", slots);
                    set(&i, "held", g.inventory_slot as u32);
                    set(&i, "money", u32::from(inv.money));
                    set(&i, "capacity", u32::from(inv.capacity()));
                    set(&i, "combines", g.combines());
                    set(&o, "inventory", i);
                }
                None => set(&o, "inventory", JsValue::NULL),
            }
            // The loading screen (ARCHITECTURE.md §6.3): null once open.
            match g.loading_screen() {
                Some(sc) => {
                    let l = Object::new();
                    set(&l, "text", dwell_game::loading::loading_text(&sc).as_str());
                    // Its stage for the shell's list (shared/look/loading.json `stages`).
                    set(&l, "stage", dwell_game::loading::loading_stage(&sc));
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
            // How many times it has opened: the shell fades the view in from white on each.
            set(&o, "loadingOpens", g.loading_gate().opens());
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
        } else if s.shaping_world() && s.options.loading_screen {
            // Joined, the world's maps on the way (ADR 0049): the loading screen says so.
            let l = Object::new();
            set(&l, "text", "Shaping the world…");
            set(&l, "stage", "worldMap");
            set(&l, "ready", 0u32);
            set(&l, "needed", 1u32);
            set(&o, "loading", l);
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
            "bodyTarget",
            d.body_target.map_or(JsValue::NULL, |(body, cell, face)| {
                let r = Object::new();
                set(&r, "body", body);
                set(
                    &r,
                    "cell",
                    cell.iter().map(|&v| JsValue::from(v)).collect::<Array>(),
                );
                set(&r, "face", face);
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
                set(&r, "waitingNearOffView", l.waiting[2] as u32);
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

    /// The streaming report's game part as JSON (`Game::streaming_report`, docs/DEV_TOOLS.md), or
    /// "null" before playing: the shell adds the device, settings and world and copies it.
    #[wasm_bindgen(js_name = streamingReport)]
    pub fn streaming_report(&self) -> String {
        let s = self.inner.borrow();
        s.game.as_ref().map_or_else(
            || "null".to_string(),
            |g| g.streaming_report(crate::telemetry::now_ms()).to_string(),
        )
    }

    /// Whether the GPU adapter is a CPU (software) one: the shell's quick loading fade (§6.3).
    #[wasm_bindgen(js_name = cpuAdapter)]
    pub fn cpu_adapter(&self) -> bool {
        self.inner.borrow().renderer.borrow().cpu_adapter()
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

    /// The minimap (ADR 0068): brought up to date for the player's place and heading; its RGBA
    /// disc (`minimapPixels()`² pixels, the heading up) when drawn again, else undefined.
    #[wasm_bindgen(js_name = minimapFrame)]
    pub fn minimap_frame(&self, now_ms: f64, shading: bool) -> Option<Vec<u8>> {
        let mut guard = self.inner.borrow_mut();
        let s = &mut *guard;
        let yaw = s.input.look.yaw;
        let g = s.game.as_mut()?;
        let pos = g.position();
        let t0 = crate::telemetry::now_ms();
        let up = s.minimap.update(
            g.world(),
            pos,
            yaw,
            now_ms / 1000.0,
            shading,
            &dwell_game::minimap::MAP,
            &|m| (m != dwell_world::M.air).then(|| dwell_mesh::lod_color(m, 0)),
        );
        // Explored cells the terrain has not shown take the generator's colours, a tile at a
        // time in the job workers (at most `revealed.jobsInFlight` at once; never generated here).
        while let Some((key, rgba)) = g.take_explore_tile() {
            s.minimap
                .explored
                .fill(key, rgba, &dwell_game::minimap::MAP);
        }
        for key in s.minimap.explored.ask(&dwell_game::minimap::MAP) {
            g.request_explore_tile(key);
        }
        // Its cost (DEV_TOOLS.md): a read of the tops (with its draw), or a draw alone.
        let ms = crate::telemetry::now_ms() - t0;
        if up.scanned {
            crate::telemetry::sample("minimap.scan", ms);
        } else if up.drawn {
            crate::telemetry::sample("minimap.draw", ms);
        }
        up.drawn.then(|| s.minimap.image.clone())
    }
    /// The generated 1 m tiles the minimap wants under its disc (`dwell_game::minimap::Generated`,
    /// the big map's closest level's): [face, ti, tk] each, the nearest first; the shell hands
    /// each over with `minimapTile` from its tile cache, building those it does not have first.
    #[wasm_bindgen(js_name = minimapWants)]
    pub fn minimap_wants(&self) -> Vec<i32> {
        let mut guard = self.inner.borrow_mut();
        let s = &mut *guard;
        let Some(g) = &s.game else {
            return Vec::new();
        };
        let pos = g.position();
        let f = dwell_world::FaceFrame::at_height(pos[1]);
        let local = f.point(pos);
        let face = u8::from(f.is_b());
        s.minimap
            .generated
            .wants(face, local[0], local[2], &dwell_game::minimap::MAP)
            .into_iter()
            .flat_map(|(face, ti, tk)| [i32::from(face), ti, tk])
            .collect()
    }
    /// A generated 1 m tile for the minimap: its RGBA and its height codes (`takeReveal`'s).
    #[wasm_bindgen(js_name = minimapTile)]
    pub fn minimap_tile(&self, face: u8, ti: i32, tk: i32, rgba: Vec<u8>, heights: Vec<i16>) {
        let mut bytes = rgba;
        for h in heights {
            bytes.extend_from_slice(&h.to_le_bytes());
        }
        self.inner
            .borrow_mut()
            .minimap
            .generated
            .fill((face, ti, tk), bytes);
    }
    /// The minimap's size in pixels (its texels across).
    #[wasm_bindgen(js_name = minimapPixels)]
    pub fn minimap_pixels(&self) -> u32 {
        dwell_game::minimap::Minimap::pixels(&dwell_game::minimap::MAP) as u32
    }
    /// The other players on the minimap: [right, up] metres from the player each, on the player's
    /// face and within the disc.
    #[wasm_bindgen(js_name = minimapDots)]
    pub fn minimap_dots(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return Vec::new();
        };
        let pos = g.position();
        let f = dwell_world::FaceFrame::at_height(pos[1]);
        let local = f.point(pos);
        let frame = dwell_game::minimap::MapFrame::new(f, local[0], local[2], s.input.look.yaw);
        let r = dwell_game::minimap::MAP.minimap_radius_m;
        let mut out = Vec::new();
        for feet in g.remote_feet() {
            if dwell_world::FaceFrame::at_height(feet[1]) != f {
                continue;
            }
            let p = f.point(feet);
            let [sx, sy] = frame.to_screen([p[0], p[2]]);
            if sx * sx + sy * sy <= r * r {
                out.extend([sx, sy]);
            }
        }
        out
    }
    /// The player on the explored map: [face (0 A, 1 B), x, z (face-local m), heading (degrees)].
    #[wasm_bindgen(js_name = mapPose)]
    pub fn map_pose(&self) -> Vec<f64> {
        let s = self.inner.borrow();
        let Some(g) = &s.game else {
            return Vec::new();
        };
        let pos = g.position();
        let f = dwell_world::FaceFrame::at_height(pos[1]);
        vec![
            f64::from(u8::from(f.is_b())),
            pos[0],
            pos[2],
            s.input.look.yaw,
        ]
    }
    /// The explored cells new or changed since the last call: [face, i, k, 0xRRGGBB, height code]
    /// each (`dwell_game::minimap::height_code`; `NO_HEIGHT`, i32::MIN, for none).
    #[wasm_bindgen(js_name = takeExplored)]
    pub fn take_explored(&self) -> Vec<i32> {
        let mut s = self.inner.borrow_mut();
        let mut out = Vec::new();
        for (key @ (face, i, k), c) in s.minimap.explored.take() {
            let code = s.minimap.explored.height_of(key);
            out.extend([i32::from(face), i, k, c as i32, code]);
        }
        out
    }
    /// The big map's cells tinted by their heights (`dwell_game::minimap::shade_grid`, the
    /// minimap's look too when its toggle is on): a `w` × `h` grid in screen order inside a one-cell border, colours
    /// (top byte 0xff: nothing there) and height codes; out, the cells' RGBA.
    #[wasm_bindgen(js_name = shadeMap)]
    pub fn shade_map(&self, colours: Vec<u32>, codes: Vec<i32>, w: u32, h: u32) -> Vec<u8> {
        let t = &dwell_game::minimap::MAP;
        dwell_game::minimap::shade_grid(t, &colours, &codes, w as usize, h as usize)
    }
    /// Whether the world's game mode shows the maps (the `map` ability, ADR 0068): a server that
    /// names no mode shows them.
    #[wasm_bindgen(js_name = mapAllowed)]
    pub fn map_allowed(&self) -> bool {
        self.inner
            .borrow()
            .session
            .as_ref()
            .and_then(|s| s.world_settings())
            .is_none_or(|w| w.allows(dwell_protocol::modes::Ability::Map))
    }

    /// Whether the big map shows the whole world (the `mapRevealed` ability, ADR 0068): creative.
    /// A server that names no mode does not reveal it.
    #[wasm_bindgen(js_name = mapRevealed)]
    pub fn map_revealed(&self) -> bool {
        self.inner
            .borrow()
            .session
            .as_ref()
            .and_then(|s| s.world_settings())
            .is_some_and(|w| w.allows(dwell_protocol::modes::Ability::MapRevealed))
    }
    /// Asks for tile (`ti`, `tk`) of the big map's zoom level of `cell_m` cells on `face`
    /// (`revealed.tileCells`² of them; a level of `bigMap.levels`); `takeReveal` returns it once
    /// generated. Another cell size is ignored.
    #[wasm_bindgen(js_name = requestReveal)]
    pub fn request_reveal(&self, face: u8, ti: i32, tk: i32, cell_m: i32) {
        if let Some(g) = &mut self.inner.borrow_mut().game {
            // The world's own levels (BIG_MAP.md M2b: a bigger world's coarser cells).
            let size = g.world_bounds().size;
            if crate::map_reveal::is_level_sized(cell_m, &dwell_game::minimap::MAP, size) {
                g.request_reveal(face, ti, tk, cell_m);
            }
        }
    }
    /// The joined world's size (BIG_MAP.md M2b): its radius scale, 1 for today's (and before the
    /// join's settings arrive), for the big map's levels and disc.
    #[wasm_bindgen(js_name = worldRadiusScale)]
    pub fn world_radius_scale(&self) -> u32 {
        self.inner
            .borrow()
            .session
            .as_ref()
            .map_or(1, |s| s.world_size().radius_scale)
    }
    /// The big map's zoom levels for the joined world's size, closest first, as
    /// [cellM, spanM, cellM, spanM, …] (`MapTunables::big_map_levels_sized`; at 1× map.json's).
    #[wasm_bindgen(js_name = bigMapLevels)]
    pub fn big_map_levels(&self) -> Vec<f64> {
        let size = self
            .inner
            .borrow()
            .session
            .as_ref()
            .map_or(Default::default(), |s| s.world_size());
        dwell_game::minimap::MAP
            .big_map_levels_sized(size)
            .into_iter()
            .flat_map(|(c, s)| [c, s])
            .collect()
    }
    /// Tile (`ti`, `tk`) of the big map's level of `cell_m` cells on `face` read now from the
    /// loaded terrain (`crate::map_reveal::live_tile`: the closest levels' real blocks, ADR 0068),
    /// in `takeReveal`'s bytes; undefined for a level not drawn so or nothing loaded there.
    #[wasm_bindgen(js_name = liveTile)]
    pub fn live_tile(&self, face: u8, ti: i32, tk: i32, cell_m: i32) -> Option<Vec<u8>> {
        let mut s = self.inner.borrow_mut();
        let g = s.game.as_mut()?;
        let pos = g.position();
        let me = dwell_world::FaceFrame::at_height(pos[1]);
        let f = if face == 1 {
            dwell_world::FaceFrame::B
        } else {
            dwell_world::FaceFrame::A
        };
        // The eye's face-local height on its own face; on the other face its ground round the
        // midplane's distance is the same (a tile there is rarely loaded).
        let eye_y = me.point(pos)[1];
        let t0 = crate::telemetry::now_ms();
        let out = crate::map_reveal::live_tile(
            g.world(),
            f,
            ti,
            tk,
            cell_m,
            eye_y,
            &dwell_game::minimap::MAP,
        );
        crate::telemetry::sample("map.liveTile", crate::telemetry::now_ms() - t0);
        out
    }
    /// A finished tile of the big map, or undefined: face, ti, tk and its level's cell side (i32
    /// LE), then RGBA and height codes (none: no terrain to reveal from).
    #[wasm_bindgen(js_name = takeReveal)]
    pub fn take_reveal(&self) -> Option<Vec<u8>> {
        self.inner
            .borrow_mut()
            .game
            .as_mut()
            .and_then(|g| g.take_reveal())
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
        "build" => TouchButton::Build,
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

    /// [`edit`](Self::edit) with the button held by `holder`: a place builds on while it is held.
    fn edit_held(&mut self, action: EditAction, holder: Holder, now_ms: f64) -> bool {
        self.game
            .as_mut()
            .is_some_and(|g| g.edit_held(action, holder, now_ms))
    }

    /// The next build mode, told to the shell (the HUD and the touch Build button).
    fn cycle_build_mode(&mut self) {
        if let Some(g) = &mut self.game {
            let label = g.cycle_build_mode();
            self.shell_events.push(shell_event("buildMode", label));
        }
    }

    fn on_input_event(&mut self, e: InputEvent, now_ms: f64) {
        match e {
            InputEvent::Toggle(key) => self.shell_events.push(shell_event("toggle", key)),
            InputEvent::Digit(code) => {
                if let Some(g) = &mut self.game
                    && let Some(slot) =
                        dwell_game::creative_hotbar::slot_for_digit(&code, g.hotbar_len())
                {
                    let slot = g.select_hotbar(slot);
                    self.shell_events.push(shell_event("slot", slot as u32));
                }
            }
            InputEvent::HotbarStep(step) => {
                if let Some(g) = &mut self.game
                    && let Some(slot) = g.scroll_hotbar(step)
                {
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
            InputEvent::BuildMode => self.cycle_build_mode(),
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
            TouchEvent::Hold => {
                self.edit_held(EditAction::Place, Holder::Touch, now_ms);
            }
            TouchEvent::HoldEnd => {
                if let Some(g) = &mut self.game {
                    g.release_place(Holder::Touch);
                }
            }
            TouchEvent::BuildMode => self.cycle_build_mode(),
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
            ..
        }) = self.session.as_ref().map(|s| s.state().clone())
        else {
            return;
        };
        let Some(maps) = self.world_maps(world_seed, generator_version) else {
            return;
        };
        let world_size = self
            .session
            .as_ref()
            .map_or(Default::default(), |s| s.world_size());
        let workers = self.options.workers;
        let jobs = match WebJobs::new(
            &self.options.make_worker,
            workers,
            workers,
            (generator_version, world_seed, world_size),
            &maps,
            self.options.lod_map,
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
        game.set_capabilities(region_edits, brushes);
        game.set_world(generator_version, world_seed, world_size);
        // The moon in the sky from its real terrain (MOON_WORLD.md §5, M-2): its face A baked once
        // at the join (~0.1 s natively at 256²), every world its own moon.
        if let Some(bake) = crate::render::moon::bake_for(world_seed) {
            self.renderer.borrow_mut().set_moon(&bake);
        }
        game.vent_plumes = crate::vent_plumes::VentPlumes::from_worldgen(
            self.session.as_ref().map_or("", |s| s.worldgen_config()),
        );
        game.day_override = self.options.day;
        if let Some(w) = self.session.as_ref().and_then(|s| s.world_settings()) {
            game.set_world_settings(&w);
        }
        if let Some(s) = self.session.as_ref() {
            game.set_vitals_rules(s.vitals_rules());
            game.set_instant_breaks(s.instant_breaks());
            game.set_structures(s.structures());
            game.set_structure_ops(
                s.capability(dwell_protocol::capability_names::STRUCTURES_PLACE),
            );
        }
        game.set_loading_screen(self.options.loading_screen);
        game.set_crossing_roll(self.crossing_roll);
        game.set_respawn_preload(dwell_game::loading::respawn_preload_from_query(
            &self.options.search,
        ));
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
        if self.options.lod_map && self.lod_map_supported() {
            game.set_lod_map(true);
        }
        game.set_flow_particles(self.flow_particles);
        game.set_falls_shown(self.falls_shown);
        game.set_blast_settings(self.blast_settings_now());
        self.game = Some(game);
    }

    /// The Explosions settings with the adapter's tier (a CPU adapter draws fewer puffs).
    fn blast_settings_now(&self) -> crate::blasts::BlastSettings {
        crate::blasts::BlastSettings {
            cpu: self.renderer.borrow().cpu_adapter(),
            ..self.blast_settings
        }
    }

    /// Whether this adapter draws the LOD colour map (not a CPU adapter: the cheapest tier there).
    fn lod_map_supported(&self) -> bool {
        self.renderer.borrow().lod_map_supported()
    }

    /// The world's maps (ADR 0049), once they are here and installed in this instance: asks the
    /// shell for them the first time, `None` while they are on the way. A world without terrain,
    /// a shell without `prepareWorld` or a failure: no maps (each instance computes its own).
    fn world_maps(&mut self, world_seed: u64, generator_version: u32) -> Option<Vec<Uint8Array>> {
        if !dwell_world::has_terrain(generator_version) {
            return Some(Vec::new());
        }
        match &self.shaping {
            Shaping::Ready(maps) => return Some(maps.clone()),
            Shaping::Waiting(slot) => {
                let done = slot.borrow_mut().take()?;
                let maps = match done {
                    Ok(maps) => maps
                        .into_iter()
                        .filter(|m| match dwell_worldgen::worldmap::install(&m.to_vec()) {
                            Ok(_) => true,
                            Err(e) => {
                                web_sys::console::warn_1(
                                    &format!("Dwell: a world map was refused ({e})").into(),
                                );
                                false
                            }
                        })
                        .collect(),
                    Err(e) => {
                        web_sys::console::warn_1(
                            &format!("Dwell: the world maps are unavailable ({e})").into(),
                        );
                        Vec::new()
                    }
                };
                self.shaping = Shaping::Ready(maps.clone());
                return Some(maps);
            }
            Shaping::NotAsked => {}
        }
        let Some(prepare) = &self.options.prepare_world else {
            self.shaping = Shaping::Ready(Vec::new());
            return Some(Vec::new());
        };
        let slot: MapsSlot = Rc::default();
        let worldgen = self
            .session
            .as_ref()
            .map_or("", |s| s.worldgen_config())
            .to_owned();
        // And its size (BIG_MAP.md M2b): a bigger world's maps are its own.
        let scale = self
            .session
            .as_ref()
            .map_or(1, |s| s.world_size().radius_scale);
        let promise = prepare.apply(
            &JsValue::NULL,
            &Array::of4(
                &js_sys::BigInt::from(world_seed).into(),
                &generator_version.into(),
                &worldgen.into(),
                &scale.into(),
            ),
        );
        let into = slot.clone();
        spawn_local(async move {
            let result = match promise.map(|p| JsFuture::from(Promise::resolve(&p))) {
                Ok(f) => f.await.map(|v| {
                    if v.is_null() || v.is_undefined() {
                        Vec::new()
                    } else {
                        Array::from(&v)
                            .iter()
                            .filter_map(|m| m.dyn_into::<Uint8Array>().ok())
                            .collect()
                    }
                }),
                Err(e) => Err(e),
            };
            *into.borrow_mut() = Some(result.map_err(|e| format!("{e:?}")));
        });
        self.shaping = Shaping::Waiting(slot);
        None
    }

    /// Joined and waiting for the world's maps: the loading screen's phase.
    fn shaping_world(&self) -> bool {
        self.game.is_none() && matches!(self.shaping, Shaping::Waiting(_))
    }

    /// Draws the frame; whether it did (not while the GPU is behind, nor without a surface).
    fn draw(&mut self) -> bool {
        // Not until the pipelines are made, a few a frame (render/pipeline.rs, bug #71): on a cold
        // shader cache each compiles for a while in the GPU process, which paints no frame then.
        if !self.renderer.borrow_mut().warm_pipelines() {
            telemetry::count("draw.warming", 1.0);
            return false;
        }
        // Not while the GPU is still behind on earlier frames (render/pacing.rs): recording now
        // would block this thread until it caught up.
        if !self
            .renderer
            .borrow_mut()
            .ready_to_draw(telemetry::now_ms())
        {
            telemetry::count("draw.skipped", 1.0);
            telemetry::add("draw.paced", 1.0);
            return false;
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
                telemetry::add("draw.no_surface", 1.0);
                return false;
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
        true
    }
}

/// Whether the page's query asks for the LOD colour map preview (`lodmap=on`).
/// `?loddebug=a,b` (the renderer's LOD debug switches, `set_lod_debug`).
fn lod_debug_query(search: &str) -> String {
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix("loddebug="))
        .unwrap_or_default()
        .replace("%2C", ",")
}

fn lod_map_query(search: &str) -> bool {
    search
        .trim_start_matches('?')
        .split('&')
        .any(|kv| kv == "lodmap=on" || kv == "lodmap=1")
}
