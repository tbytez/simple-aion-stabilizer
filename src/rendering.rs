//! DirectX rendering with an integrated-GPU preference and WARP fallback.

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
        // Prefer the integrated GPU to reduce competition with the game.
        // Keep WARP available on machines without a compatible GPU.
        // AION_SOFTWARE_RENDERING=1 explicitly selects WARP for troubleshooting.
        let software = std::env::var_os("AION_SOFTWARE_RENDERING").is_some();
        setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
            adapters
                .iter()
                .filter(|adapter| surface.is_none_or(|s| adapter.is_surface_supported(s)))
                .filter(|adapter| {
                    !software || adapter.get_info().device_type == wgpu::DeviceType::Cpu
                })
                .min_by_key(|adapter| match adapter.get_info().device_type {
                    wgpu::DeviceType::IntegratedGpu => 0,
                    wgpu::DeviceType::DiscreteGpu => 1,
                    wgpu::DeviceType::VirtualGpu => 2,
                    wgpu::DeviceType::Other => 3,
                    wgpu::DeviceType::Cpu => 4,
                })
                .cloned()
                .ok_or_else(|| "No compatible DirectX 12 renderer available".into())
        }));
        config.wgpu_setup = WgpuSetup::CreateNew(setup);
    }
    config
}
