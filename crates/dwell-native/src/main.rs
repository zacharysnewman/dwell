//! Dwell's native performance harness (docs/DEV_TOOLS.md "Native harness"; ARCHITECTURE.md §2):
//! the same Rust client, renderer and local-mode server as the browser build, in a native window
//! (winit + wgpu on Metal), to compare native with Chrome. A dev tool, never shipped: no menus, no
//! HUD, no audio.
//!
//! What the browser splits across the page and its workers runs here on threads: the game and the
//! renderer on the main thread (`web/app.rs`'s `Client` one for one), the local server on its own
//! (`server.rs`, the local-mode worker), the client's jobs on generation and meshing threads
//! (`jobs.rs`, the job workers), the water grid and the server's generation on the server's native
//! threads. The same telemetry is recorded (`dwell_client::telemetry`), a perf line is printed and
//! shown in the window's title every second, and `--scenario` runs a bench scenario from
//! `client/bench/scenarios.json` and writes the browser bench's `report.json` shape.

mod jobs;
mod scenario;
mod server;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dwell_client::game::{Game, LodSettings, Send};
use dwell_client::net::session::{Out, Session, SessionState};
use dwell_client::pool::Pool;
use dwell_client::render::Renderer;
use dwell_client::telemetry;
use dwell_game::build_drag::Holder;
use dwell_game::input::{EditAction, InputEvent, KeyboardMouseInput};
use dwell_physics::config::PlayerControllerConfig;
use ed25519_dalek::{Signer, SigningKey};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::server::{Down, ServerStats, ServerThread, Up};

const USAGE: &str = "dwell-native: Dwell's native performance harness (docs/DEV_TOOLS.md)

  --seed N            the world's seed (default 7, the bench's)
  --at X,Y,Z          spawn the player's feet there (the dev link ?at=)
  --world FILE        the world file (default: the temp directory's dwell-native-<seed>.redb)
  --fresh             start a new world (delete the world file first)
  --flat              the flat world (?world=flat)
  --size WxH          the window's size in points (default 1280x720, the bench's)
  --pixel-ratio R     drawing-buffer pixels per point (default 1: the bench's Chrome, DPR 1)
  --present MODE      fifo (vsync, default), mailbox or immediate (the bench's --unthrottled)
  --workers N         client job threads of each kind (default min(4, cores - 2), as the page)
  --server-threads N  the server's generation and LOD threads (default 1: its one jobs worker)
  --scenario NAME     run a bench scenario, write OUT/NAME/report.json and quit
  --scenarios FILE    the scenarios file (default client/bench/scenarios.json)
  --out DIR           where reports go (default client/bench/out/native-<time>)

Keys as in the browser (WASD, Space, Shift, F flies, digits pick, clicks break and place).
Click the window to capture the mouse; Esc releases it.";

#[derive(Clone, Debug)]
pub struct Args {
    pub seed: u64,
    pub at: Option<[f64; 3]>,
    pub world: Option<PathBuf>,
    pub fresh: bool,
    pub flat: bool,
    pub size: (u32, u32),
    pub pixel_ratio: f64,
    pub present: wgpu::PresentMode,
    pub workers: usize,
    pub server_threads: usize,
    pub scenario: Option<String>,
    pub scenarios: PathBuf,
    pub out: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut a = Args {
        seed: 7,
        at: None,
        world: None,
        fresh: false,
        flat: false,
        size: (1280, 720),
        pixel_ratio: 1.0,
        present: wgpu::PresentMode::Fifo,
        workers: 4.min(cores.saturating_sub(2)).max(1),
        server_threads: 1,
        scenario: None,
        scenarios: repo_root().join("client/bench/scenarios.json"),
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or_else(|| format!("{k} needs a value"));
        match k.as_str() {
            "--seed" => a.seed = v()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--at" => {
                let p: Vec<f64> = v()?
                    .split(',')
                    .map(|s| s.trim().parse::<f64>())
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("--at: {e}"))?;
                a.at = Some(p.try_into().map_err(|_| "--at: x,y,z")?);
            }
            "--world" => a.world = Some(v()?.into()),
            "--fresh" => a.fresh = true,
            "--flat" => a.flat = true,
            "--size" => {
                let s = v()?;
                let (w, h) = s.split_once('x').ok_or("--size: WxH")?;
                a.size = (
                    w.parse().map_err(|e| format!("--size: {e}"))?,
                    h.parse().map_err(|e| format!("--size: {e}"))?,
                );
            }
            "--pixel-ratio" => {
                a.pixel_ratio = v()?.parse().map_err(|e| format!("--pixel-ratio: {e}"))?;
            }
            "--present" => {
                a.present = match v()?.as_str() {
                    "fifo" => wgpu::PresentMode::Fifo,
                    "mailbox" => wgpu::PresentMode::Mailbox,
                    "immediate" => wgpu::PresentMode::Immediate,
                    m => return Err(format!("--present: unknown mode {m}")),
                }
            }
            "--workers" => a.workers = v()?.parse().map_err(|e| format!("--workers: {e}"))?,
            "--server-threads" => {
                a.server_threads = v()?.parse().map_err(|e| format!("--server-threads: {e}"))?;
            }
            "--scenario" => a.scenario = Some(v()?),
            "--scenarios" => a.scenarios = v()?.into(),
            "--out" => a.out = Some(v()?.into()),
            "--help" | "-h" => return Err(USAGE.into()),
            _ => return Err(format!("unknown option {k}\n\n{USAGE}")),
        }
    }
    Ok(a)
}

/// The repository's root (this crate is `crates/dwell-native`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Rc<RefCell<Renderer>>,
    adapter: wgpu::AdapterInfo,
}

/// The game client: `web/app.rs`'s `Inner` without the DOM.
pub struct Harness {
    pub args: Args,
    gfx: Gfx,
    server: ServerThread,
    session: Session,
    key: SigningKey,
    pub game: Option<Game<Renderer>>,
    pub input: KeyboardMouseInput,
    pub lod_settings: LodSettings,
    /// When the harness started (telemetry clock, ms), and when the player was first playing.
    pub started_ms: f64,
    pub playing_ms: Option<f64>,
    /// The server's last second, and every second since the last `mark`.
    pub server_last: ServerStats,
    pub server_seconds: Vec<ServerStats>,
    /// Frames drawn since the perf line was last printed.
    drawn: u32,
    perf_at: f64,
    /// This frame's wait for the surface (ms): the vsync wait, natively.
    acquire_ms: f64,
    pub failed: Option<String>,
}

impl Harness {
    fn new(args: Args, window: Arc<Window>) -> Result<Self, String> {
        let started_ms = telemetry::now_ms();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("no surface: {e}"))?;
        let (adapter, device, queue) =
            pollster::block_on(dwell_client::request_device(&instance, Some(&surface)))?;
        let info = adapter.get_info();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let (w, h) = drawing_size(&window, args.pixel_ratio);
        let present = if caps.present_modes.contains(&args.present) {
            args.present
        } else {
            eprintln!(
                "dwell-native: {:?} is not offered; using Fifo",
                args.present
            );
            wgpu::PresentMode::Fifo
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode: present,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
            eprintln!("dwell-native: GPU error: {e}");
        }));
        let mut renderer = Renderer::with_samples(
            device,
            queue,
            format,
            dwell_client::render::samples_for(info.device_type),
        );
        renderer.set_adapter(info.device_type);
        renderer.resize(w, h, args.pixel_ratio);
        // The shell's desktop defaults (client/src/ui: shadowSettings, waterSettings,
        // viewSettings, detailSettings): what a player gets in Chrome on a Mac.
        renderer.set_shadow_quality(dwell_look::shadows::ShadowQuality::from_level(2));
        renderer.set_torch_shadows(dwell_look::shadows::ShadowQuality::from_level(1));
        renderer.set_water_quality(dwell_client::render::water::WaterQuality::default());
        renderer.set_clouds(dwell_look::clouds::CloudQuality::High);
        let lod_settings = LodSettings {
            pixel_error: dwell_protocol::lod::PIXEL_ERROR_DESKTOP as f64,
            cache_bytes: (dwell_protocol::lod::CACHE_MB_DESKTOP as usize) << 20,
            detail_distance_m: Some(256.0),
            full_mode: false,
        };

        let world = args.world.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("dwell-native-{}.redb", args.seed))
        });
        if args.fresh {
            let _ = std::fs::remove_file(&world);
        }
        let server = ServerThread::start(server::Options {
            seed: args.seed,
            generator: if args.flat {
                dwell_world::GENERATOR_FLAT
            } else {
                dwell_world::GENERATOR_TERRAIN
            },
            at: args.at,
            world_file: Some(world),
            threads: args.server_threads,
        });
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut session = Session::new(
            [0; 32],
            key.verifying_key().to_bytes(),
            "Native",
            dwell_sim::app_version::APP_VERSION,
            1000.0,
        );
        session.start(telemetry::now_ms());
        Ok(Self {
            args,
            gfx: Gfx {
                window,
                surface,
                config,
                renderer: Rc::new(RefCell::new(renderer)),
                adapter: info,
            },
            server,
            session,
            key,
            game: None,
            input: KeyboardMouseInput::new(),
            lod_settings,
            started_ms,
            playing_ms: None,
            server_last: ServerStats::default(),
            server_seconds: Vec::new(),
            drawn: 0,
            perf_at: started_ms,
            acquire_ms: 0.0,
            failed: None,
        })
    }

    pub fn adapter(&self) -> &wgpu::AdapterInfo {
        &self.gfx.adapter
    }

    pub fn renderer(&self) -> std::cell::RefMut<'_, Renderer> {
        self.gfx.renderer.borrow_mut()
    }

    /// Moves bytes between the server thread and the session (`Inner::pump_network`).
    fn pump_network(&mut self, now_ms: f64) {
        while let Ok(d) = self.server.down.try_recv() {
            match d {
                Down::Reliable(c, b) => self.session.on_reliable(c, &b, now_ms),
                Down::Datagram(b) => self.session.on_datagram(&b, now_ms),
                Down::Stats(s) => {
                    self.server_seconds.push(s.clone());
                    self.server_last = s;
                }
                Down::Failed(m) => self.failed = Some(m),
            }
        }
        self.session.tick(now_ms);
        for out in self.session.take_out() {
            match out {
                Out::Reliable(b) => {
                    let _ = self.server.up.send(Up::Reliable(b));
                }
                Out::Datagram(b) => {
                    let _ = self.server.up.send(Up::Datagram(b));
                }
                Out::Sign(t) => self.session.signed(self.key.sign(&t).to_bytes()),
                Out::Close => {}
            }
        }
    }

    /// Once joined: the game with its job threads (`Inner::start_game`).
    fn start_game(&mut self) {
        let SessionState::Joined {
            player_id,
            world_seed,
            generator_version,
            verification_chunk,
            may_fly,
            region_edits,
            brushes,
        } = self.session.state().clone()
        else {
            return;
        };
        let jobs = jobs::ThreadJobs::new(
            self.args.workers,
            self.args.workers,
            (generator_version, world_seed),
        );
        let pool = Pool::new(Box::new(jobs));
        let mut game = Game::new(
            player_id,
            pool,
            self.gfx.renderer.clone(),
            Some(dwell_world::ChunkCoord::from_array(verification_chunk)),
            PlayerControllerConfig::default(),
        );
        let (w, h) = self.gfx.renderer.borrow().canvas_size();
        let r = self.args.pixel_ratio;
        game.viewport_css = (f64::from(w) / r, f64::from(h) / r);
        game.set_capabilities(region_edits, brushes);
        game.set_world(generator_version, world_seed);
        if let Some(w) = self.session.world_settings() {
            game.set_world_clock(w.clock);
        }
        // The bench's pages show the loading screen as players do.
        game.set_loading_screen(true);
        game.set_registry(self.session.mapping().cloned());
        self.input.flight.set_allowed(may_fly);
        game.enable_lod(self.lod_settings);
        self.game = Some(game);
    }

    /// One frame: network, the game's ticks, drawing (`Client::frame`). Whether it drew.
    fn frame(&mut self) -> bool {
        let now_ms = telemetry::now_ms();
        let t0 = now_ms;
        telemetry::with(|t| t.begin_frame(now_ms));
        telemetry::span("net", || self.pump_network(now_ms));
        if self.game.is_none() {
            self.start_game();
        }
        if let Some(game) = &mut self.game {
            while let Some((m, bytes)) = self.session.next_game() {
                if let dwell_protocol::Message::EditAck(a) = &m {
                    let done = self.session.edit_results(a);
                    game.on_edit_results(&done);
                    continue;
                }
                game.on_game_message(m, bytes.len(), now_ms);
            }
            let stats = self.session.stats();
            game.rtt_ms = stats.datagram_rtt_ms.or(stats.rtt_ms);
            game.frame(now_ms, &mut self.input, None, &mut telemetry::now_ms);
            for out in game.take_outbox() {
                match out {
                    Send::Control(m) => self.session.send_control(&m),
                    Send::Datagram(m) => self.session.send_game_datagram(&m),
                    Send::Edit(m, id) => {
                        self.session.send_edit(&m, id);
                    }
                }
            }
            let dropped: Vec<(u32, bool)> = self
                .session
                .take_dropped_edits()
                .into_iter()
                .map(|id| (id, true))
                .collect();
            game.on_edit_results(&dropped);
            if self.playing_ms.is_none() && game.debug_state().terrain_ready {
                self.playing_ms = Some(now_ms);
                println!(
                    "dwell-native: playing {:.2} s after start",
                    (now_ms - self.started_ms) / 1000.0
                );
            }
        }
        telemetry::span("net", || self.pump_network(now_ms));
        let world = self.game.as_ref().and_then(|g| g.clock.seconds(now_ms));
        self.gfx
            .renderer
            .borrow_mut()
            .set_cloud_time(world.unwrap_or(now_ms / 1000.0));
        self.acquire_ms = 0.0;
        let drew = self.draw();
        let cpu = telemetry::now_ms() - t0;
        telemetry::add("frame.cpu", cpu);
        // With vsync the surface's acquire waits here for the display (the browser's does not:
        // its compositor presents), so the frame's own work is `frame.cpu` without that wait.
        telemetry::add("frame.busy", cpu - self.acquire_ms);
        // Natively the GPU's callbacks (the pacer's completions, the timestamp readbacks) run
        // when the device is polled; the browser runs them from its event loop.
        let _ = self
            .gfx
            .renderer
            .borrow()
            .device()
            .poll(wgpu::PollType::Poll);
        if drew {
            self.drawn += 1;
        }
        self.perf_line(telemetry::now_ms());
        drew
    }

    /// `Inner::draw`.
    fn draw(&mut self) -> bool {
        if !self
            .gfx
            .renderer
            .borrow_mut()
            .ready_to_draw(telemetry::now_ms())
        {
            telemetry::count("draw.skipped", 1.0);
            return false;
        }
        let acquire = telemetry::now_ms();
        let current = self.gfx.surface.get_current_texture();
        self.acquire_ms = telemetry::now_ms() - acquire;
        telemetry::add("draw.acquire", self.acquire_ms);
        let frame = match current {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => {
                let renderer = self.gfx.renderer.clone();
                self.gfx
                    .surface
                    .configure(renderer.borrow().device(), &self.gfx.config);
                return false;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let game = &self.game;
        telemetry::span("draw.render", || {
            self.gfx.renderer.borrow_mut().render(&view, &|c| {
                game.as_ref()
                    .map_or(Some(dwell_lod::fade::Fade::Full), |g| g.chunk_fade(c))
            });
        });
        let queue = self.gfx.renderer.borrow().queue().clone();
        telemetry::span("draw.present", || queue.present(frame));
        true
    }

    /// The perf line (`ui/perfLine.ts`) once a second: printed and in the window's title.
    fn perf_line(&mut self, now_ms: f64) {
        if now_ms - self.perf_at < 1000.0 {
            return;
        }
        let fps = f64::from(self.drawn) * 1000.0 / (now_ms - self.perf_at);
        self.drawn = 0;
        self.perf_at = now_ms;
        let (cpu, gpu) = telemetry::with(|t| {
            (
                t.recent_frame_mean("frame.busy", 60).unwrap_or(f64::NAN),
                t.recent_sample_mean("gpu.frame", 30).unwrap_or(f64::NAN),
            )
        });
        let s = &self.server_last;
        let line = format!(
            "{fps:.0} fps  CPU {cpu:.1} ms  GPU {gpu:.1} ms  server {} ticks/s {:.2} ms",
            s.ticks, s.tick_mean_ms
        );
        if self.args.scenario.is_none() {
            println!("{line}");
        }
        self.gfx.window.set_title(&format!("Dwell native — {line}"));
    }

    fn resize(&mut self) {
        let (w, h) = drawing_size(&self.gfx.window, self.args.pixel_ratio);
        if (w, h) != (self.gfx.config.width, self.gfx.config.height) {
            self.gfx.config.width = w;
            self.gfx.config.height = h;
            let renderer = self.gfx.renderer.clone();
            self.gfx
                .surface
                .configure(renderer.borrow().device(), &self.gfx.config);
        }
        self.gfx
            .renderer
            .borrow_mut()
            .resize(w, h, self.args.pixel_ratio);
        if let Some(g) = &mut self.game {
            let r = self.args.pixel_ratio;
            g.viewport_css = (f64::from(w) / r, f64::from(h) / r);
        }
    }

    /// Captures or releases the mouse (the page's pointer lock).
    fn set_pointer_locked(&mut self, locked: bool) {
        let w = &self.gfx.window;
        if locked {
            if w.set_cursor_grab(CursorGrabMode::Locked).is_err() {
                let _ = w.set_cursor_grab(CursorGrabMode::Confined);
            }
            w.set_cursor_visible(false);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
        }
        self.input.pointer_locked = locked;
        if !locked && let Some(g) = &mut self.game {
            g.release_place(Holder::Mouse);
        }
    }

    /// A key press (`Client::key_down`).
    pub fn key_down(&mut self, code: &str, repeat: bool, ctrl: bool, meta: bool) {
        let now_ms = telemetry::now_ms();
        if self.input.pointer_locked
            && dwell_look::spyglass::is_spyglass_key(
                &dwell_look::spyglass::SPYGLASS,
                code,
                ctrl,
                meta,
            )
        {
            if !repeat && let Some(g) = self.game.as_mut() {
                g.spyglass.toggle(&dwell_look::spyglass::SPYGLASS);
            }
            return;
        }
        let k = self.input.key_down(code, repeat, ctrl, meta);
        for e in k.events {
            self.on_input_event(e, now_ms);
        }
    }

    fn on_input_event(&mut self, e: InputEvent, now_ms: f64) {
        let Some(g) = &mut self.game else { return };
        match e {
            InputEvent::Toggle(key) => println!("dwell-native: {key} (no overlay in the harness)"),
            InputEvent::Digit(code) => {
                let slot = code
                    .strip_prefix("Digit")
                    .and_then(|d| d.parse::<i64>().ok())
                    .map(|d| if d == 0 { 9 } else { d - 1 });
                if let Some(slot) = slot
                    && (slot as usize) < g.interaction.palette().len()
                {
                    g.interaction.select(slot);
                }
            }
            InputEvent::HotbarStep(step) => {
                g.interaction.scroll(step);
            }
            InputEvent::FlightChanged(on) => println!("dwell-native: flying {on}"),
            InputEvent::Tool(tool) => {
                g.tool(tool, now_ms);
            }
            InputEvent::Brush(action) => g.brush_key(action),
            InputEvent::BuildMode => {
                g.cycle_build_mode();
            }
        }
    }

    fn mouse_button(&mut self, button: i16, down: bool) {
        let now_ms = telemetry::now_ms();
        if !self.input.pointer_locked {
            if down {
                self.set_pointer_locked(true);
            }
            return;
        }
        let action = self.input.mouse_down(button);
        let Some(g) = &mut self.game else { return };
        if down {
            if button == 1 && g.brush_pick() {
                return;
            }
            if let Some(action) = action {
                g.edit_held(action, Holder::Mouse, now_ms);
            }
        } else if action == Some(EditAction::Place) {
            g.release_place(Holder::Mouse);
        }
    }
}

/// The drawing buffer for the window: its size in points × the pixel ratio.
fn drawing_size(window: &Window, pixel_ratio: f64) -> (u32, u32) {
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    (
        ((size.width * pixel_ratio).round() as u32).max(1),
        ((size.height * pixel_ratio).round() as u32).max(1),
    )
}

struct App {
    args: Args,
    harness: Option<Harness>,
    scenario: Option<scenario::Run>,
    modifiers: winit::keyboard::ModifiersState,
    exit_code: i32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.harness.is_some() {
            return;
        }
        let mut size = self.args.size;
        if let Some(s) = &self.scenario {
            size = s.window_size();
        }
        let attrs = Window::default_attributes()
            .with_title("Dwell native")
            .with_inner_size(winit::dpi::LogicalSize::new(size.0, size.1));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("dwell-native: no window: {e}");
                self.exit_code = 1;
                event_loop.exit();
                return;
            }
        };
        let mut args = self.args.clone();
        if let Some(s) = &self.scenario {
            s.configure(&mut args);
        }
        match Harness::new(args, window) {
            Ok(mut h) => {
                if let Some(s) = &mut self.scenario {
                    s.started(&mut h);
                }
                h.gfx.window.request_redraw();
                self.harness = Some(h);
            }
            Err(e) => {
                eprintln!("dwell-native: {e}");
                self.exit_code = 1;
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(h) = &mut self.harness else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => h.resize(),
            WindowEvent::Focused(false) => {
                h.input.blur();
                h.set_pointer_locked(false);
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                // winit's key codes are named as the DOM's `KeyboardEvent.code`s.
                let name = format!("{code:?}");
                if self.scenario.is_some() {
                    if name == "Escape" && event.state == ElementState::Pressed {
                        event_loop.exit();
                    }
                    return;
                }
                match event.state {
                    ElementState::Pressed if name == "Escape" => h.set_pointer_locked(false),
                    ElementState::Pressed => h.key_down(
                        &name,
                        event.repeat,
                        self.modifiers.control_key(),
                        self.modifiers.super_key(),
                    ),
                    ElementState::Released => h.input.key_up(&name),
                }
            }
            WindowEvent::MouseInput { state, button, .. } if self.scenario.is_none() => {
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => return,
                };
                h.mouse_button(b, state == ElementState::Pressed);
            }
            WindowEvent::RedrawRequested => {
                h.frame();
                if let Some(m) = h.failed.take() {
                    eprintln!("dwell-native: the world did not start: {m}");
                    self.exit_code = 1;
                    event_loop.exit();
                    return;
                }
                if let Some(s) = &mut self.scenario {
                    match s.update(h) {
                        scenario::Status::Running => {}
                        scenario::Status::Done => event_loop.exit(),
                        scenario::Status::Failed(e) => {
                            eprintln!("dwell-native: {e}");
                            self.exit_code = 1;
                            event_loop.exit();
                        }
                    }
                }
                h.gfx.window.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        let Some(h) = &mut self.harness else { return };
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && h.input.pointer_locked
        {
            let k = h
                .game
                .as_ref()
                .map_or(1.0, |g| g.spyglass_view().look_scale);
            h.input.mouse_move(dx * k, dy * k);
        }
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let scenario = match &args.scenario {
        Some(name) => match scenario::Run::load(&args, name) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("dwell-native: {e}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        args,
        harness: None,
        scenario,
        modifiers: Default::default(),
        exit_code: 0,
    };
    let started = Instant::now();
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("dwell-native: {e}");
        app.exit_code = 1;
    }
    // Stops the server (saving the world) and the job threads.
    drop(app.harness.take());
    eprintln!("dwell-native: ran {:.1} s", started.elapsed().as_secs_f64());
    std::process::exit(app.exit_code);
}
