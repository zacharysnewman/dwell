//! Dwell's browser client (ADR 0028): the session and transports (`net`, `web`), local mode
//! (`local`), background jobs (`jobs`, `pool`), the game loop (`game`) and the wgpu renderer
//! (`render`), with the JS API in `web` (wasm-bindgen). This file also holds the WebGPU probe and
//! the renderer's smoke scene, which `client/e2e-webgpu` checks in headless Chromium.

pub mod game;
pub mod jobs;
pub mod local;
pub mod net;
pub mod pool;
pub mod render;
pub mod telemetry;
#[cfg(target_arch = "wasm32")]
mod web;
pub mod world_clock;

/// Side of the probe's square render target, in pixels.
pub const PROBE_SIZE: u32 = 64;
/// The renderer smoke scene's side, pixels.
pub const SCENE_SIZE: u32 = 128;
/// The probe clears to this colour (RGBA8) and draws a triangle in [`PROBE_TRIANGLE`] over it.
pub const PROBE_CLEAR: [u8; 4] = [0, 0, 255, 255];
pub const PROBE_TRIANGLE: [u8; 4] = [255, 128, 0, 255];

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A device on the browser's WebGPU adapter. `None` when WebGPU is unavailable.
pub async fn request_device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no WebGPU adapter: {e}"))?;
    // Timestamp queries where the adapter has them: the renderer's GPU pass timing (DEV_TOOLS.md).
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no WebGPU device: {e}"))?;
    Ok((adapter, device, queue))
}

/// The probe's pipeline for a colour target of `format`.
pub fn triangle_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe"),
        source: wgpu::ShaderSource::Wgsl(include_str!("triangle.wgsl").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Records the probe's pass: clear, then the triangle.
pub fn draw_triangle(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    view: &wgpu::TextureView,
) {
    let [r, g, b, a] = PROBE_CLEAR.map(|c| f64::from(c) / 255.0);
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("probe"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.draw(0..3, 0..1);
}

/// Renders the probe offscreen and reads it back: RGBA8 rows of [`PROBE_SIZE`] pixels.
pub async fn render_offscreen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<Vec<u8>, String> {
    let size = wgpu::Extent3d {
        width: PROBE_SIZE,
        height: PROBE_SIZE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let pipeline = triangle_pipeline(device, FORMAT);
    let mut encoder = device.create_command_encoder(&Default::default());
    draw_triangle(
        &mut encoder,
        &pipeline,
        &texture.create_view(&Default::default()),
    );
    queue.submit([encoder.finish()]);
    read_back(device, queue, &texture).await
}

/// Copies a texture (RGBA8, its width a multiple of 64 px) back to the CPU.
pub async fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Vec<u8>, String> {
    let size = texture.size();
    // A multiple of 64 px × 4 B is a multiple of the required 256 B row alignment.
    let row = size.width * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = futures_channel::oneshot::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    rx.await
        .map_err(|_| "readback dropped".to_string())?
        .map_err(|e| format!("readback failed: {e}"))?;
    let pixels = readback
        .get_mapped_range(..)
        .map_err(|e| format!("readback range: {e}"))?
        .to_vec();
    Ok(pixels)
}

/// The renderer's smoke scene (a stone-and-grass chunk with water, a player and the sky), drawn
/// offscreen at `SCENE_SIZE` square and read back: the browser test checks its regions.
pub async fn render_scene(device: wgpu::Device, queue: wgpu::Queue) -> Result<Vec<u8>, String> {
    use dwell_blocks::require;
    let size = wgpu::Extent3d {
        width: SCENE_SIZE,
        height: SCENE_SIZE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let (stone, grass) = (require("dwell:stone"), require("dwell:grass"));
    let mut padded = vec![0u16; dwell_mesh::PADDED_VOLUME];
    for x in -1..=32 {
        for z in -1..=32 {
            for y in -1..=8 {
                padded[dwell_mesh::padded_index(x, y, z)] = if y == 8 { grass } else { stone };
            }
        }
    }
    let meshes = dwell_mesh::mesh_chunk(&padded, false).map_err(|e| format!("{e:?}"))?;
    let mut renderer = render::Renderer::new(device.clone(), queue.clone(), FORMAT);
    renderer.resize(SCENE_SIZE, SCENE_SIZE, 1.0);
    renderer.set_terrain_chunk(dwell_world::ChunkCoord { x: 0, y: 0, z: 0 }, Some(&meshes));
    renderer.set_player(
        1,
        Some(render::PlayerView {
            feet: [16.0, 9.0, 16.0],
            face: 1.0,
            yaw: 0.0,
            crouched: false,
            dead: false,
            color: 0xff3030,
            radius: 0.4,
            height: 1.8,
        }),
    );
    // A dead player lying beside it, and one turned to face the camera's right.
    for (id, feet, yaw, dead) in [
        (2, [13.0, 9.0, 15.0], 0.0, true),
        (3, [19.0, 9.0, 16.0], 90.0, false),
    ] {
        renderer.set_player(
            id,
            Some(render::PlayerView {
                feet,
                face: 1.0,
                yaw,
                crouched: false,
                dead,
                color: 0x3060ff,
                radius: 0.4,
                height: 1.8,
            }),
        );
    }
    // Looking along −Z from above the chunk's +Z edge, the horizon across the middle.
    renderer.set_camera([16.0, 11.0, 24.0], 180.0, -12.0, 1.0, 0.0);
    renderer.render(&texture.create_view(&Default::default()), &|_| true);
    read_back(&device, &queue, &texture).await
}
