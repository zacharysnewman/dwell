//! The JS API (`wasm-bindgen`): what the page calls.

pub mod app;
pub mod jobs;
pub mod local_worker;
pub mod march_test;
pub mod transport;

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

/// The GPU marcher against the CPU reference per pixel (`render::march::check`): renders its
/// crafted scenes offscreen and returns the report as JSON (`client/e2e-webgpu/march.spec.ts`).
#[wasm_bindgen(js_name = probeMarch)]
pub async fn probe_march() -> Result<String, JsError> {
    let instance = instance();
    let (adapter, device, queue) = crate::request_device(&instance, None)
        .await
        .map_err(|e| JsError::new(&e))?;
    let r = crate::render::march::check::run(&device, &queue)
        .await
        .map_err(|e| JsError::new(&e))?;
    let strings = |v: &[String]| {
        v.iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    Ok(format!(
        "{{\"adapter\":{:?},\"views\":{},\"pixels\":{},\"hits\":{},\"idMismatches\":{},\
         \"recordMismatches\":{},\"tInexact\":{},\"tUlpsMax\":{},\"maxDepthError\":{},\
         \"maxDirError\":{},\"levelsHit\":{},\"passes\":{},\"lines\":[{}],\"examples\":[{}]}}",
        adapter.get_info().name,
        r.views,
        r.pixels,
        r.hits,
        r.id_mismatches,
        r.record_mismatches,
        r.t_inexact,
        r.t_ulps_max,
        r.max_depth_error,
        r.max_dir_error,
        r.levels_hit,
        r.passes(),
        strings(&r.lines),
        strings(&r.examples),
    ))
}

/// The hotbar picture of palette slot `slot` (TILE × TILE RGBA8).
#[wasm_bindgen(js_name = hotbarSwatch)]
pub fn hotbar_swatch(slot: usize) -> Vec<u8> {
    let palette = dwell_game::block_interaction::palette();
    palette
        .get(slot)
        .map_or_else(Vec::new, |p| dwell_mesh::swatch(p.material))
}

/// Chooses this context's texture set by name (`classic`, `painterly`; docs/PAINTERLY_TEXTURES.md):
/// the page's from `?textures=` (in `Client.create`), each meshing worker's from its init message,
/// before its first job. False for an unknown name, which leaves the set as it was.
#[wasm_bindgen(js_name = setTextureSet)]
pub fn set_texture_set(name: &str) -> bool {
    dwell_mesh::TextureSet::parse(name)
        .map(dwell_mesh::set_texture_set)
        .is_some()
}

/// The texture set in use in this context (its name).
#[wasm_bindgen(js_name = textureSet)]
pub fn texture_set() -> String {
    dwell_mesh::texture_set().name().into()
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
