//! The JS API (`wasm-bindgen`): what the page calls.

pub mod app;
pub mod chisel;
pub mod jobs;
pub mod local_worker;
pub mod server_jobs;
pub mod transport;
pub mod water_worker;

use wasm_bindgen::prelude::*;

fn instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::BROWSER_WEBGPU;
    wgpu::Instance::new(desc)
}

/// Renders the probe offscreen and returns its pixels (RGBA8, `PROBE_SIZE` square), or rejects
/// with why WebGPU is unavailable.
#[wasm_bindgen(js_name = probeOffscreen)]
pub async fn probe_offscreen() -> Result<Vec<u8>, JsError> {
    let instance = instance();
    let (_, device, queue) = crate::request_device(&instance, None)
        .await
        .map_err(|e| JsError::new(&e))?;
    crate::render_offscreen(&device, &queue)
        .await
        .map_err(|e| JsError::new(&e))
}

/// Draws the probe into a canvas once. Returns the adapter's description.
#[wasm_bindgen(js_name = probeCanvas)]
pub async fn probe_canvas(canvas: web_sys::HtmlCanvasElement) -> Result<String, JsError> {
    let (width, height) = (canvas.width(), canvas.height());
    let instance = instance();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
        .map_err(|e| JsError::new(&e.to_string()))?;
    let (adapter, device, queue) = crate::request_device(&instance, Some(&surface))
        .await
        .map_err(|e| JsError::new(&e))?;
    let config = surface
        .get_default_config(&adapter, width, height)
        .ok_or_else(|| JsError::new("canvas not supported by the adapter"))?;
    surface.configure(&device, &config);
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        other => return Err(JsError::new(&format!("no canvas texture: {other:?}"))),
    };
    let pipeline = crate::triangle_pipeline(&device, config.format);
    let mut encoder = device.create_command_encoder(&Default::default());
    crate::draw_triangle(
        &mut encoder,
        &pipeline,
        &frame.texture.create_view(&Default::default()),
    );
    queue.submit([encoder.finish()]);
    queue.present(frame);
    Ok(format!("{:?}", adapter.get_info()))
}

/// Renders the renderer's smoke scene offscreen and returns its pixels (RGBA8, `SCENE_SIZE`
/// square).
#[wasm_bindgen(js_name = probeScene)]
pub async fn probe_scene() -> Result<Vec<u8>, JsError> {
    let instance = instance();
    let (_, device, queue) = crate::request_device(&instance, None)
        .await
        .map_err(|e| JsError::new(&e))?;
    crate::render_scene(device, queue)
        .await
        .map_err(|e| JsError::new(&e))
}

/// The hotbar picture of palette slot `slot` (TILE × TILE RGBA8): a ½ m piece of a full block
/// shows as the world draws a ½ m cell, its tile's top half (`dwell_mesh::half_swatch`).
/// Draws the fog preset `name` (shared/look/fog.json `presets`; `?haze=<name>`, a dev tool to
/// compare them) instead of the fog every client draws. Called before the client is created (the
/// shaders bake the fog in); returns whether `name` is a preset.
#[wasm_bindgen(js_name = setHaze)]
pub fn set_haze(name: &str) -> bool {
    dwell_look::fog::select(name)
}

#[wasm_bindgen(js_name = hotbarSwatch)]
pub fn hotbar_swatch(slot: usize) -> Vec<u8> {
    let palette = dwell_game::block_interaction::palette();
    palette.get(slot).map_or_else(Vec::new, |p| {
        if p.tool.is_some() {
            return tool_swatch(p.item);
        }
        let mut px = dwell_mesh::swatch(p.material);
        let full = dwell_blocks::info(p.material).shape == dwell_blocks::VoxelShape::Full;
        if p.rows == 1 && full {
            dwell_mesh::half_swatch(&mut px, dwell_mesh::TILE);
        }
        px
    })
}

/// A tool's picture (ADR 0067): a handle across the tile, bottom left to top right, and a head in
/// the item's colour at its top.
pub fn tool_swatch(item: &str) -> Vec<u8> {
    // A tool with a model look (the tool models) is drawn as its model, as everywhere else.
    if let Some(look) = dwell_protocol::inventory::item(item).and_then(|d| d.look) {
        return dwell_mesh::look_swatch(&look);
    }
    let n = dwell_mesh::TILE;
    let mut px = vec![0u8; n * n * 4];
    let head = dwell_protocol::inventory::item(item).map_or([125, 116, 104], |d| d.color);
    let s = n as f64;
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f64 + 0.5) / s, 1.0 - (y as f64 + 0.5) / s);
            // The handle: within 0.06 of the diagonal u = v, from 0.12 to 0.85.
            let on_handle = (u - v).abs() < 0.085 && (0.12..0.85).contains(&(0.5 * (u + v)));
            // The head: a block across the handle's top, to its upper left.
            let (a, b) = (0.5 * (u + v), 0.5 * (v - u));
            let on_head = (0.66..0.86).contains(&a) && (-0.04..0.26).contains(&b);
            let c = if on_head {
                head.map(|c| (f64::from(c) * (1.1 - 0.5 * b)).min(255.0) as u8)
            } else if on_handle {
                [122, 84, 50]
            } else {
                continue;
            };
            let i = (y * n + x) * 4;
            px[i..i + 3].copy_from_slice(&c);
            px[i + 3] = 255;
        }
    }
    px
}

/// An inventory item's picture (ADR 0066): a block item's swatch, a non-block item's shaped look
/// (`dwell_mesh::look_swatch`) or else its colour (a tool's drawn as one); empty for an item this build doesn't know.
#[wasm_bindgen(js_name = itemSwatch)]
pub fn item_swatch(item: &str) -> Vec<u8> {
    if dwell_protocol::chopping::tool(item).is_some() {
        return tool_swatch(item);
    }
    if let Some(b) = dwell_blocks::find_block(item) {
        let m = (b.first_state..b.first_state + b.state_count)
            .find(|&m| dwell_world::edit::placeable(m))
            .unwrap_or(b.first_state);
        return dwell_mesh::swatch(m);
    }
    let Some(def) = dwell_protocol::inventory::item(item) else {
        return Vec::new();
    };
    // An item with a shaped look is drawn as that look: a stick as its rod, a rock as its lump.
    if let Some(look) = def.look {
        return dwell_mesh::look_swatch(&look);
    }
    let n = dwell_mesh::TILE;
    let mut px = vec![0u8; n * n * 4];
    // A rounded blob in its colour, so it doesn't read as a block.
    let r = n as f64 * 0.32;
    let c = (n as f64 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f64 - c).powi(2) + (y as f64 - c).powi(2)).sqrt();
            if d <= r {
                let shade = 1.0 - 0.35 * (d / r);
                let i = (y * n + x) * 4;
                for k in 0..3 {
                    px[i + k] = (f64::from(def.color[k]) * shade) as u8;
                }
                px[i + 3] = 255;
            }
        }
    }
    px
}

/// The hotbar pictures' side, pixels.
#[wasm_bindgen(js_name = swatchSize)]
pub fn swatch_size() -> usize {
    dwell_mesh::TILE
}

/// Pings a dedicated server for the lobby list (§8.3): opens a connection (WebTransport, or WebRTC
/// when `webrtc` is `[ip, port, ufrag, pwd]`), times `StatusRequest` → `StatusResponse` and closes
/// it. Resolves with the round trip in ms (the connection's setup not included).
#[wasm_bindgen(js_name = pingServer)]
pub async fn ping_server(
    url: String,
    cert_hash: Vec<u8>,
    webrtc: Option<js_sys::Array>,
    timeout_ms: f64,
) -> Result<f64, JsError> {
    use dwell_protocol::Message;
    use transport::{TransportEvent, WebRtcConn, WebTransportConn};
    let hash: [u8; 32] = cert_hash
        .try_into()
        .map_err(|_| JsError::new("certificate hash must be 32 bytes"))?;
    let now = || js_sys::Date::now();
    let deadline = now() + timeout_ms;
    enum Conn {
        Wt(WebTransportConn),
        Rtc(WebRtcConn),
    }
    let conn = match WebTransportConn::connect(&url, hash).await {
        Ok(c) => Conn::Wt(c),
        Err(e) => {
            let Some(w) = webrtc else {
                return Err(JsError::new(&e));
            };
            let invite = crate::net::sdp::WebRtcInvite {
                ip: w.get(0).as_string().unwrap_or_default(),
                port: w.get(1).as_f64().unwrap_or(0.0) as u16,
                ufrag: w.get(2).as_string().unwrap_or_default(),
                pwd: w.get(3).as_string().unwrap_or_default(),
            };
            Conn::Rtc(
                WebRtcConn::connect(&invite, hash)
                    .await
                    .map_err(|e| JsError::new(&e))?,
            )
        }
    };
    let (events, request) = (
        match &conn {
            Conn::Wt(c) => c.events(),
            Conn::Rtc(c) => c.events(),
        },
        Message::StatusRequest.encode(),
    );
    let close = |c: &Conn| match c {
        Conn::Wt(c) => c.close(),
        Conn::Rtc(c) => c.close(),
    };
    let sent = now();
    match &conn {
        Conn::Wt(c) => c.send_reliable(&request),
        Conn::Rtc(c) => c.send_reliable(&request),
    }
    while now() < deadline {
        let next = events.borrow_mut().pop_front();
        match next {
            Some(TransportEvent::Reliable(dwell_protocol::Channel::Control, bytes)) => {
                match Message::decode(&bytes) {
                    Some(Message::StatusResponse(_)) => {
                        let rtt = now() - sent;
                        close(&conn);
                        return Ok(rtt);
                    }
                    Some(_) => {}
                    None => {
                        close(&conn);
                        return Err(JsError::new("The server sent a malformed status."));
                    }
                }
            }
            Some(TransportEvent::Closed(m)) => {
                return Err(JsError::new(if m.is_empty() {
                    "The connection closed."
                } else {
                    &m
                }));
            }
            Some(_) => {}
            None => transport::sleep_ms(5).await,
        }
    }
    close(&conn);
    Err(JsError::new("The server did not answer."))
}

/// Runs when the module loads (on the page and in every worker): panics are logged with their
/// message and location, rather than surfacing only as `RuntimeError: unreachable`.
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("Dwell panicked: {info}").into());
    }));
}

/// The app version that last saved a world file (its bytes), or undefined (debug tools, e2e).
#[wasm_bindgen(js_name = worldFileVersion)]
pub fn world_file_version(bytes: Vec<u8>) -> Option<String> {
    crate::local::world_file_version(bytes)
}

/// A world file's bytes rewritten to say `version` last saved it (debug tools, e2e: a world
/// "saved by another version", RELEASES.md §6).
#[wasm_bindgen(js_name = setWorldFileVersion)]
pub fn set_world_file_version(bytes: Vec<u8>, version: &str) -> Result<Vec<u8>, JsError> {
    crate::local::with_world_file_version(bytes, version).map_err(|e| JsError::new(&e))
}

/// The cache keys of a terrain world's maps, faces A then B (ADR 0049): the build's map fingerprint
/// and each face's seed, so a map kept by another build is never read.
#[wasm_bindgen(js_name = worldMapKeys)]
/// A test world's worldgen config (`worldgen`, `chisel::worldgen_height`) keys its maps apart.
/// A bigger world's (`radius_scale`, BIG_MAP.md M2b: its `WorldSettings` size) keys its maps apart
/// too; none or 1 is today's key.
pub fn world_map_keys(
    world_seed: u64,
    worldgen: Option<String>,
    radius_scale: Option<u32>,
) -> Vec<String> {
    crate::local::world_map_keys(
        world_seed,
        &worldgen.unwrap_or_default(),
        dwell_protocol::world_size::WorldSize::playable(radius_scale),
    )
}

/// Computes face `face`'s (0: A, 1: B) world map of a terrain world and returns its bytes (a
/// builder worker's job: ~1.3 s).
#[wasm_bindgen(js_name = buildWorldMap)]
/// A test world's (`worldgen`) is built over its height field; throws when the config is bad.
/// A bigger world's map (`radius_scale`, BIG_MAP.md M2b) is built over its own disc.
pub fn build_world_map(
    world_seed: u64,
    face: u8,
    worldgen: Option<String>,
    radius_scale: Option<u32>,
) -> Result<Vec<u8>, JsError> {
    let face = if face == 1 {
        dwell_world::Face::B
    } else {
        dwell_world::Face::A
    };
    dwell_worldgen::config::worldgen_map_bytes_sized(
        dwell_worldgen::bifacial::face_seed(world_seed, face),
        &worldgen.unwrap_or_default(),
        dwell_protocol::world_size::WorldSize::playable(radius_scale),
    )
    .map_err(|e| JsError::new(&e))
}

/// Installs a world map's bytes in this instance, so its generators use it rather than computing
/// it; throws when the bytes are damaged or from another build.
#[wasm_bindgen(js_name = installWorldMap)]
pub fn install_world_map(bytes: &[u8]) -> Result<(), JsError> {
    dwell_worldgen::worldmap::install(bytes)
        .map(|_| ())
        .map_err(|e| JsError::new(&e))
}
