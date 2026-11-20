#![cfg_attr(windows, windows_subsystem = "windows")]

mod controller;
mod core;
mod draw;
mod engine;
mod manifest;
mod procs;
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
mod profile;
mod rendering;
#[cfg(windows)]
mod sys;
mod theme;
#[cfg(windows)]
mod tray;
mod ui;
#[cfg(windows)]
mod update;
#[cfg(any(windows, test))]
mod update_install;

use controller::{Cmd, Config, Controller};
use std::path::PathBuf;
use std::sync::Arc;

pub const TITLE: &str = "Simple Aion Stabilizer";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let start_tray = args.iter().any(|a| a == "--tray" || a == "--minimized");

    #[cfg(windows)]
    let _instance;
    #[cfg(windows)]
    {
        // headless update-helper mode: the staged new exe finishes the
        // file swap after the old app exits - no mutex, no window
        if let Some(i) = args.iter().position(|a| a == "--finish-update") {
            let code = args
                .get(i + 1)
                .map(|d| update::finish_update(std::path::Path::new(d)))
                .unwrap_or(1);
            std::process::exit(code);
        }

        // headless rollback-helper mode: restores the previous exe
        // parked as *.exe.old after the app exits - no mutex, no window
        if args.iter().any(|a| a == "--finish-rollback") {
            std::process::exit(update::finish_rollback());
        }

        // Take ownership, not merely a new handle to the named object.
        // Keep the guard on the main thread until the event loop exits.
        let start_updated = args.iter().any(|a| a == "--updated");
        let timeout = if start_updated {
            std::time::Duration::from_secs(120)
        } else {
            std::time::Duration::ZERO
        };
        let Some(instance) = sys::InstanceMutex::acquire(timeout) else {
            sys::focus_existing_instance();
            return;
        };
        _instance = instance;
        // on the --updated relaunch keep the previous exe (*.exe.old) -
        // it is the rollback source offered by the update banner
        update::cleanup_old_files(start_updated);
        update::cleanup_stale_stages();
    }

    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let config_text = std::fs::read_to_string(app_dir.join("config.ini")).unwrap_or_default();
    let cfg = Config::load(&config_text);

    // Rollback is offered once, on the relaunch right after a successful
    // update, while the previous exe is still parked as *.exe.old.
    let rollback_offer = args.iter().any(|a| a == "--updated")
        && app_dir
            .join(format!("{}.old", manifest::APP_EXE))
            .is_file();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../icon.png"))
        .expect("embedded icon must decode");

    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([420.0, 830.0])
        .with_min_inner_size([360.0, 700.0])
        // Do not force a logical minimum larger than a small high-DPI
        // display. The UI scrolls in both axes below its normal layout width.
        .with_clamp_size_to_monitor_size(true)
        .with_resizable(true)
        .with_decorations(true)
        .with_visible(!start_tray)
        .with_icon(icon);

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: rendering::configuration(),
        ..Default::default()
    };

    let cfg_for_app = cfg;
    let start_engine = cfg_for_app.autostart_engine;

    let result = eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx);
            // explicit override for testing: AION_UI=light|dark
            if let Ok(t) = std::env::var("AION_UI") {
                cc.egui_ctx.set_theme(if t == "light" {
                    egui::ThemePreference::Light
                } else {
                    egui::ThemePreference::Dark
                });
            }

            let ctx = cc.egui_ctx.clone();
            let ctrl = Controller::new(
                app_dir,
                cfg_for_app,
                rollback_offer,
                Arc::new(move || ctx.request_repaint()),
            );

            #[cfg(windows)]
            {
                sys::apply_window_style();
                tray::spawn(ctrl.clone());
            }

            if start_engine {
                ctrl.send(Cmd::Start { auto: false });
            }

            Ok(Box::new(ui::App::new(ctrl)))
        }),
    );
    if let Err(error) = result {
        eprintln!("Unable to start {TITLE}: {error}");
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let message: Vec<u16> = format!("Unable to start {TITLE}: {error}\n\nWindows 10 or newer is required.")
                .encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = TITLE.encode_utf16().chain(Some(0)).collect();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
    }
}

// Exercise the exact portable policy used by the vendored Windows handler
// on the macOS build host too; no duplicate model of the native decision.
#[cfg(test)]
#[path = "../vendor/winit/src/platform_impl/windows/dpi_resize_policy.rs"]
mod native_dpi_regression_tests;

#[cfg(test)]
#[path = "../vendor/winit/src/platform_impl/windows/size_constraints.rs"]
mod native_minimum_regression_tests;

#[cfg(test)]
#[path = "../vendor/winit/src/platform_impl/windows/icon_pixels.rs"]
mod native_icon_regression_tests;
