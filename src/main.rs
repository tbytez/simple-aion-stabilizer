#![cfg_attr(windows, windows_subsystem = "windows")]

mod controller;
mod core;
mod engine;
mod procs;
#[cfg(windows)]
mod sys;
mod theme;
#[cfg(windows)]
mod tray;
mod ui;
#[cfg(windows)]
mod update;

use controller::{Cmd, Config, Controller};
use std::path::PathBuf;
use std::sync::Arc;

pub const TITLE: &str = "Simple Aion Stabilizer";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let start_tray = args.iter().any(|a| a == "--tray" || a == "--minimized");

    #[cfg(windows)]
    {
        // the updater relaunches us while the old process may still be
        // exiting - retry the mutex briefly before giving up
        let start_updated = args.iter().any(|a| a == "--updated");
        let mut ok = sys::acquire_single_instance();
        for _ in 0..40 {
            if ok || !start_updated {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
            ok = sys::acquire_single_instance();
        }
        if !ok {
            sys::focus_existing_instance();
            return;
        }
        update::cleanup_old_files();
    }

    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let config_text = std::fs::read_to_string(app_dir.join("config.ini")).unwrap_or_default();
    let cfg = Config::load(&config_text);

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../icon.png"))
        .expect("embedded icon must decode");

    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([380.0, 580.0])
        .with_min_inner_size([380.0, 580.0])
        .with_max_inner_size([380.0, 580.0])
        .with_resizable(false)
        .with_decorations(false)
        .with_visible(!start_tray)
        .with_icon(icon);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let cfg_for_app = cfg;
    let start_engine = cfg_for_app.autostart_engine;

    let _ = eframe::run_native(
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
}
