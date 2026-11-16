//! Keep the Windows utility off the game GPU and its OpenGL overlay hooks.

pub fn configuration() -> eframe::egui_wgpu::WgpuConfiguration {
    #[allow(unused_mut)]
    let mut config = eframe::egui_wgpu::WgpuConfiguration::default();
    config.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
    #[cfg(windows)]
    {
        use eframe::egui_wgpu::{wgpu, WgpuSetup, WgpuSetupCreateNew};
        use std::sync::Arc;

        let mut setup = WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        // Use the system shader compiler; do not probe PATH for dxcompiler.dll.
        setup
            .instance_descriptor
            .backend_options
            .dx12
            .shader_compiler = wgpu::Dx12Compiler::Fxc;
        // DX12 exposes Microsoft's WARP CPU adapter. Never silently fall back
        // to the NVIDIA/AMD device: that would reintroduce game overlay hooks.
        setup.native_adapter_selector = Some(Arc::new(|adapters, surface| {
            adapters
                .iter()
                .find(|adapter| {
                    adapter.get_info().device_type == wgpu::DeviceType::Cpu
                        && surface.is_none_or(|s| adapter.is_surface_supported(s))
                })
                .cloned()
                .ok_or_else(|| {
                    "Microsoft WARP renderer unavailable (Windows 10 or newer required)".into()
                })
        }));
        config.wgpu_setup = WgpuSetup::CreateNew(setup);
    }
    config
}
