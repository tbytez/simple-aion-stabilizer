use super::*;
use aion_cpu_renderer::Rasterizer;
use eframe::egui_wgpu::{self as gpu, wgpu};
use std::{path::PathBuf, time::Instant};
struct Oracle {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: gpu::Renderer,
}
impl Oracle {
    fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("Metal oracle adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let renderer = gpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            gpu::RendererOptions {
                dithering: false,
                ..Default::default()
            },
        );
        Self {
            device,
            queue,
            renderer,
        }
    }
    fn render(
        &mut self,
        size: [u32; 2],
        scale: f32,
        mesh: &[egui::ClippedPrimitive],
        delta: &mut egui::TexturesDelta,
    ) -> Vec<u8> {
        for (id, updates) in delta.set.drain() {
            for d in updates {
                self.renderer
                    .update_texture(&self.device, &self.queue, id, &d);
            }
        }
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let screen = gpu::ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: scale,
        };
        let extra =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, mesh, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), mesh, &screen);
        }
        let stride = (size[0] * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            extent,
        );
        self.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).unwrap();
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        let mut out = Vec::new();
        for y in 0..size[1] as usize {
            out.extend_from_slice(
                &data[y * stride as usize..y * stride as usize + size[0] as usize * 4],
            );
        }
        drop(data);
        buffer.unmap();
        for id in delta.free.drain() {
            self.renderer.free_texture(&id);
        }
        out
    }
}
#[test]
#[ignore = "Explicit local CPU/GPU raster comparison and benchmark; needs Metal"]
fn cpu_gpu_capture_comparison() {
    let outdir = PathBuf::from(std::env::var("CPU_CAPTURE_DIR").expect("CPU_CAPTURE_DIR"));
    std::fs::create_dir_all(&outdir).unwrap();
    let mut timings=String::from("theme,running,scale,width,height,cpu_median_ms,cpu_max_ms,mean_rgb_error,pixels_error_gt8_fraction\n");
    let mut oracle = Oracle::new();
    for (name, logical, scale, dark, running) in [
        ("dark-100", [420., 700.], 1., true, false),
        ("light-100", [420., 700.], 1., false, false),
        ("running-125", [420., 700.], 1.25, true, true),
        ("dark-150", [420., 700.], 1.5, true, false),
        ("dark-200", [420., 700.], 2., true, false),
        ("light-250", [420., 700.], 2.5, false, true),
        ("dark-300", [420., 700.], 3., true, false),
        ("wide-100", [1000., 900.], 1., true, false),
        ("tiny-250", [260., 300.], 2.5, true, false),
        ("max-1440", [2560., 1440.], 1., true, false),
        ("max-1600-125", [2048., 1280.], 1.25, true, false),
        ("max-4k", [1920., 1080.], 2., true, false),
    ] {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx);
        ctx.set_theme(if dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
        let (ctrl, _rx) = Controller::for_ui_test_with_commands();
        let mut app = App::new(ctrl);
        app.status.running = running;
        app.status.game_present = running;
        app.status.watch = true;
        app.status.check = CheckState::UpToDate;
        let mut raster = Rasterizer::default();
        for frame in 0..3 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(logical[0], logical[1]),
                )),
                time: Some(frame as f64 / 30.),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scale);
            let mut output = ctx.run_ui(input, |ui| app.show(ui));
            let mesh = ctx.tessellate(output.shapes, output.pixels_per_point);
            let mut gpu_delta = output.textures_delta.clone();
            raster.update(&mut output.textures_delta).unwrap();
            let size = [(logical[0] * scale) as usize, (logical[1] * scale) as usize];
            raster.render(size, scale, Color32::BLACK, &mesh).unwrap();
            let gpu = oracle.render(
                [size[0] as u32, size[1] as u32],
                scale,
                &mesh,
                &mut gpu_delta,
            );
            raster.free(&mut output.textures_delta);
            if frame == 2 {
                let cpu = raster.image();
                let cpu = cpu.as_raw();
                let mut sum = 0u64;
                let mut bad = 0usize;
                for (a, b) in cpu.as_chunks::<4>().0.iter().zip(gpu.as_chunks::<4>().0) {
                    let e = [
                        a[0].abs_diff(b[0]),
                        a[1].abs_diff(b[1]),
                        a[2].abs_diff(b[2]),
                    ];
                    sum += e.iter().map(|v| u64::from(*v)).sum::<u64>();
                    if e.into_iter().max().unwrap() > 8 {
                        bad += 1;
                    }
                }
                let mean = sum as f64 / (size[0] * size[1] * 3) as f64;
                let fraction = bad as f64 / (size[0] * size[1]) as f64;
                std::fs::write(outdir.join(format!("{name}-cpu.rgba")), cpu).unwrap();
                std::fs::write(outdir.join(format!("{name}-gpu.rgba")), gpu).unwrap();
                std::fs::write(
                    outdir.join(format!("{name}.size")),
                    format!("{} {}", size[0], size[1]),
                )
                .unwrap();
                let mut ms = Vec::new();
                for _ in 0..15 {
                    let t = Instant::now();
                    raster.render(size, scale, Color32::BLACK, &mesh).unwrap();
                    ms.push(t.elapsed().as_secs_f64() * 1000.);
                }
                ms.sort_by(f64::total_cmp);
                let row = format!(
                    "{name},{running},{scale},{},{},{:.3},{:.3},{mean:.5},{fraction:.6}\n",
                    size[0], size[1], ms[7], ms[14]
                );
                print!("{row}");
                timings.push_str(&row);
                std::fs::write(outdir.join("results.csv"), &timings).unwrap();
                assert!(
                    mean < 1.0 && fraction < 0.01,
                    "CPU raster mismatch {name}: mean={mean}, bad fraction={fraction}"
                );
            }
        }
    }
}

#[test]
#[ignore = "Explicit local CPU/GPU resize and font-atlas comparison; needs Metal"]
fn cpu_gpu_resize_sequence() {
    let ctx=egui::Context::default();theme::install_fonts(&ctx);theme::apply(&ctx);
    let (ctrl,_rx)=Controller::for_ui_test_with_commands();let mut app=App::new(ctrl);
    let mut raster=Rasterizer::default();let mut oracle=Oracle::new();let mut frame=0;
    for (w,h,scale) in [(420.,700.,1.),(419.,699.,1.25),(754.,907.,1.),(280.,240.,2.5),(1920.,1080.,2.),(420.,700.,3.),(420.,700.,1.)] {
        for _ in 0..3 {
            frame+=1;
            let mut input=egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(w,h))),time:Some(frame as f64/30.),..Default::default()};
            input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point=Some(scale);
            let mut output=ctx.run_ui(input,|ui|app.show(ui));let meshes=ctx.tessellate(output.shapes,output.pixels_per_point);
            let mut gpu_delta=output.textures_delta.clone();raster.update(&mut output.textures_delta).unwrap();
            let size=[(w*scale) as usize,(h*scale) as usize];raster.render(size,scale,Color32::BLACK,&meshes).unwrap();
            let gpu=oracle.render([size[0] as u32,size[1] as u32],scale,&meshes,&mut gpu_delta);
            let image=raster.image();let mut bad=0;
            for(a,b)in image.as_raw().as_chunks::<4>().0.iter().zip(gpu.as_chunks::<4>().0){if (0..3).any(|i|a[i].abs_diff(b[i])>8){bad+=1;}}
            assert!((bad as f64)/((size[0]*size[1]) as f64)<0.01,"resize raster mismatch {w}x{h}@{scale}: {bad}");
            raster.free(&mut output.textures_delta);
        }
    }
}
