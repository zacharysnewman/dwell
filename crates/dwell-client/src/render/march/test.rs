//! Native tests of the GPU marcher. The per-pixel comparison with the reference (`check.rs`)
//! needs a real adapter: on macOS the tests build wgpu's Metal backend (a dev-dependency feature,
//! never in a build); elsewhere, or without a GPU, it is skipped with a note — CI runs the same
//! comparison in headless Chromium on SwiftShader (`client/e2e-webgpu/march.spec.ts`). The rest
//! run on wgpu's noop device.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use super::check;
use super::{MarchFrame, Marcher};

/// Runs a future to completion, polling the device while it waits (map callbacks fire there).
pub(super) fn block_on<F: Future>(device: &wgpu::Device, f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }
}

pub(super) fn real_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(desc);
    let adapter = block_on_simple(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let info = adapter.get_info();
    if info.device_type == wgpu::DeviceType::Cpu && std::env::var_os("DWELL_GPU_TEST_CPU").is_none()
    {
        return None;
    }
    let (device, queue) = block_on_simple(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
        ..Default::default()
    }))
    .ok()?;
    Some((device, queue, format!("{} ({:?})", info.name, info.backend)))
}

fn block_on_simple<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn noop_device() -> (wgpu::Device, wgpu::Queue) {
    wgpu::Device::noop(&Default::default())
}

#[test]
fn the_gpu_marcher_matches_the_reference_per_pixel() {
    let Some((device, queue, name)) = real_device() else {
        eprintln!(
            "SKIPPED: no GPU adapter for the native GPU-vs-reference test (it runs in the browser \
             probe on SwiftShader: npm run e2e:webgpu)"
        );
        return;
    };
    let report = block_on(&device, check::run(&device, &queue)).expect("rendered");
    eprintln!("adapter: {name}");
    for l in &report.lines {
        eprintln!("  {l}");
    }
    for e in &report.examples {
        eprintln!("  mismatch: {e}");
    }
    eprintln!(
        "{} views, {} pixels, {} hits, levels hit {:#b}; ids differ {}, records differ {}, t inexact {} (max {} ULP), max depth \
         error {:.2e}, max direction error {:.2e}",
        report.views,
        report.pixels,
        report.hits,
        report.levels_hit,
        report.id_mismatches,
        report.record_mismatches,
        report.t_inexact,
        report.t_ulps_max,
        report.max_depth_error,
        report.max_dir_error
    );
    eprintln!(
        "beam: {} rays started late, {} past a hit, {} changed outcome",
        report.started_late, report.start_past_hit, report.start_changed
    );
    assert!(
        report.started_late > report.pixels / 4,
        "the beam starts rays late"
    );
    assert!(
        report.levels_hit & 0b1111 == 0b1111,
        "chunks and three LOD levels hit"
    );
    assert!(report.passes(), "{report:?}");
}

#[test]
fn draws_on_the_noop_device_and_takes_terrain_through_the_seam() {
    let (device, queue) = noop_device();
    let mut m = Marcher::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let table = m.gpu.scene.table().clone();
    let mut v = vec![0u16; dwell_world::CHUNK_VOLUME];
    v[100] = dwell_blocks::require("dwell:stone");
    let b = dwell_march::build_chunk(&v, &table);
    let c = dwell_world::ChunkCoord::new(0, 0, 0);
    m.set_chunk(c, Some(&b));
    let targets = m.targets((32, 16));
    let frame = MarchFrame {
        uniforms: vec![0.0; crate::render::FRAME_FLOATS],
        eye: [0.5, 10.0, 0.5],
        forward: [0.0, -1.0, 0.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 0.0, -1.0],
        tan: [0.5, 0.5],
    };
    let out = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 32,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    for _ in 0..3 {
        let mut e = device.create_command_encoder(&Default::default());
        m.draw(&mut e, &frame, &targets, None);
        m.composite(
            &mut e,
            &targets,
            &out.create_view(&Default::default()),
            None,
        );
        queue.submit([e.finish()]);
        m.submitted();
        m.collect();
    }
    assert!(m.gpu.uploaded_bytes > 0);
    let after = m.gpu.uploaded_bytes;
    // Nothing changed: nothing is uploaded again.
    let mut e = device.create_command_encoder(&Default::default());
    m.draw(&mut e, &frame, &targets, None);
    queue.submit([e.finish()]);
    assert_eq!(m.gpu.uploaded_bytes, after);
    // An edit uploads its node and bricks only.
    v[200] = dwell_blocks::require("dwell:dirt");
    m.set_chunk(c, Some(&dwell_march::build_chunk(&v, &table)));
    let mut e = device.create_command_encoder(&Default::default());
    m.draw(&mut e, &frame, &targets, None);
    queue.submit([e.finish()]);
    let edit = m.gpu.uploaded_bytes - after;
    assert!(edit > 0 && edit < 8 * 1024, "{edit} bytes for one edit");
    m.set_chunk(c, None);
    assert_eq!(m.gpu.scene.live_nodes(), 0);
}

#[test]
fn the_page_query_switches_the_beam_prepass() {
    let mut s = super::MarchSettings::default();
    assert!(s.beam);
    s.apply_query("?local=1&marchbeam=0&seed=7");
    assert!(!s.beam);
    s.apply_query("?marchbeam=1");
    assert!(s.beam);
}
