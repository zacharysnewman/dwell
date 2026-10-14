//! The JS API (`wasm-bindgen`): what the page calls.

pub mod local_worker;
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
