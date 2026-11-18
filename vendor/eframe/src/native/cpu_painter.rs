//! Unpublished CPU/GDI painter. Reuses the original eframe event/input loop.
#![allow(clippy::unnecessary_wraps)] // Match the native painter interface.
use aion_cpu_renderer::{
    diagnostics::{self, Frame, Span},
    gdi::Presenter,
    Rasterizer,
};
use egui::{Context, ViewportId, ViewportIdSet};
use egui_wgpu::{RenderState, RendererOptions, SurfaceConfig, WgpuConfiguration};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{num::NonZeroU32, sync::Arc, time::Instant};
use winit::window::Window;

pub struct Painter {
    raster: Rasterizer,
    surface: SurfaceConfig,
    presenter: Presenter,
    frame_id: u64,
}
impl Painter {
    pub async fn new(
        _ctx: Context,
        config: WgpuConfiguration,
        transparent: bool,
        _options: RendererOptions,
    ) -> Self {
        assert!(
            !transparent,
            "CPU/GDI prototype supports opaque windows only"
        );
        if let Ok(exe) = std::env::current_exe() && std::env::args().any(|a| a == "--cpu-metrics") {
            let mode = diagnostics::mode();
            let _ = std::fs::write(exe.with_file_name(format!("CPU-GDI-{mode}-renderer.txt")), format!("renderer=CPU egui mesh rasterizer + GDI\npresentation={mode}\nGPU device created=false\nDXGI swapchain created=false\nphysical_resolution=true\nrenderer_build=cpu-gdi-v1\n"));
        }
        Self {
            raster: Rasterizer::default(),
            surface: config.surface,
            presenter: Presenter::default(),
            frame_id: 0,
        }
    }
    pub fn render_state(&self) -> Option<RenderState> {
        None
    }
    pub fn max_texture_side(&self) -> Option<usize> {
        Some(16384)
    }
    pub fn set_surface_config(&mut self, value: SurfaceConfig) {
        self.surface = value;
    }
    pub async fn set_window(
        &mut self,
        _id: ViewportId,
        window: Option<Arc<Window>>,
    ) -> Result<(), crate::Error> {
        if let Some(w) = window {
            if !matches!(
                w.window_handle().map(|h| h.as_raw()),
                Ok(RawWindowHandle::Win32(_))
            ) {
                return Err(crate::Error::AppCreation(Box::new(std::io::Error::other(
                    "CPU/GDI needs a Win32 window",
                ))));
            }
        }
        Ok(())
    }
    pub fn on_window_resize_state_change(&mut self, _id: ViewportId, _resizing: bool) {}
    pub fn on_window_resized(&mut self, _id: ViewportId, w: NonZeroU32, h: NonZeroU32) {
        let _timing = Span::new("resize", w.get(), h.get());
        // No device/surface reconfiguration. Raster buffer resized on paint.
    }
    pub fn gc_viewports(&mut self, _active: &ViewportIdSet) {}
    pub fn destroy(&mut self) {
        self.raster = Rasterizer::default();
        self.presenter = Presenter::default();
    }
    #[allow(clippy::too_many_arguments)]
    #[expect(unsafe_code)] // The only FFI boundary; live winit HWND and validated pixel buffer.
    pub fn paint_and_update_textures(
        &mut self,
        _id: ViewportId,
        scale: f32,
        clear: [f32; 4],
        primitives: &[egui::ClippedPrimitive],
        delta: &mut egui::TexturesDelta,
        captures: Vec<egui::ScreenshotCallback>,
        window: &Arc<Window>,
    ) -> f32 {
        let size = window.inner_size();
        let _paint = Span::new("paint_total", size.width, size.height);
        self.frame_id += 1;
        let started = Instant::now();
        let mut frame = Frame {
            id: self.frame_id,
            pixels_per_point: scale,
            width: size.width,
            height: size.height,
            ..Default::default()
        };
        let result = (|| -> Result<(), String> {
            let start = Instant::now();
            self.raster.update(delta)?;
            frame.texture_us = start.elapsed().as_micros() as u64;
            let color = egui::Color32::from(egui::Rgba::from_rgba_premultiplied(
                clear[0], clear[1], clear[2], clear[3],
            ));
            {
                let _raster = Span::new("raster", size.width, size.height);
                let start = Instant::now();
                self.raster.render(
                    [size.width as usize, size.height as usize],
                    scale,
                    color,
                    primitives,
                )?;
                frame.raster_us = start.elapsed().as_micros() as u64;
            }
            if size.width > 0 && size.height > 0 {
                let present_start = Instant::now();
                let _blit = Span::new("gdi_present", size.width, size.height);
                let handle = window.window_handle().map_err(|e| e.to_string())?;
                let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                    return Err("No Win32 HWND".into());
                };
                window.pre_present_notify();
                // SAFETY: winit owns the live window on this UI thread throughout the call.
                if diagnostics::mode() == "direct" {
                    unsafe {
                        aion_cpu_renderer::gdi::present(
                            handle.hwnd.get() as _,
                            self.raster.size(),
                            self.raster.pixels(),
                        )
                    }
                    .map_err(|e| e.to_string())?;
                } else {
                    let timing = unsafe {
                        self.presenter.present(
                            handle.hwnd.get() as _,
                            self.raster.size(),
                            self.raster.pixels(),
                        )
                    }
                    .map_err(|e| e.to_string())?;
                    frame.prepare_us = timing.prepare_us;
                    frame.copy_us = timing.copy_us;
                    frame.blit_us = timing.blit_us;
                    frame.flush_us = timing.flush_us;
                }
                frame.present_us = present_start.elapsed().as_micros() as u64;
            }
            if !captures.is_empty() {
                let image = Arc::new(self.raster.image());
                for c in captures {
                    c.complete(image.clone());
                }
            }
            Ok(())
        })();
        self.raster.free(delta);
        delta.clear();
        if let Err(error) = result {
            // Fail visibly; never silently fall back to the GPU or present stale pixels.
            panic!("CPU/GDI prototype rendering failed: {error}");
        }
        frame.total_us = started.elapsed().as_micros() as u64;
        diagnostics::frame(frame);
        0.0 // No vsync time to subtract from frame CPU time.
    }
}
