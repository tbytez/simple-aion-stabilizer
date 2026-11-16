//! Self-update against GitHub releases: check, download, swap, relaunch.
//!
//! A running exe cannot be overwritten on Windows, but it CAN be renamed.
//! Apply order is chosen so a crash at any point leaves a usable install:
//! stage the new exe as `*.new` first, rename the running exe aside to
//! `*.old`, rename `*.new` into place, then copy the rest. The relaunched
//! process deletes the leftovers on startup.

#![cfg(windows)]

use crate::core;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const REPO: &str = "tbytez/simple-aion-stabilizer";
const APP_EXE: &str = "AionStabilizer.exe";

/// Files the user may have edited - never overwritten by an update.
const KEEP: &[&str] = &["config.ini", "profile.txt"];

fn agent(timeout_secs: u64) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .user_agent(concat!("aion-stabilizer/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// Latest GitHub release with a `*-win64.zip` asset newer than the
/// running version. Ok(None) = up to date or no usable asset.
pub fn check_latest() -> Result<Option<(String, String)>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = agent(15).get(&url).call().map_err(|e| e.to_string())?;
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(core::release_from_json(&json, env!("CARGO_PKG_VERSION")))
}

/// Download `url` to a temp zip file, reporting 0..=100 via `progress`.
pub fn download(url: &str, progress: &mut dyn FnMut(u8)) -> Result<PathBuf, String> {
    let mut resp = agent(300).get(url).call().map_err(|e| e.to_string())?;
    let total = resp.body().content_length().unwrap_or(0);

    let dest = std::env::temp_dir().join("aion-stabilizer-update.zip");
    let mut out = fs::File::create(&dest).map_err(|e| e.to_string())?;

    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut got = 0u64;
    let mut last_pct = 255u8;
    progress(0);
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if total > 0 {
            let pct = (got * 100 / total).min(100) as u8;
            if pct != last_pct {
                last_pct = pct;
                progress(pct);
            }
        }
    }
    if total > 0 && got != total {
        return Err(format!("download truncated ({got}/{total} bytes)"));
    }
    Ok(dest)
}

/// Extract the release zip into a fresh temp dir and return the folder
/// that contains AionStabilizer.exe (top level or single subdir).
pub fn extract(zip_path: &Path) -> Result<PathBuf, String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let dest = std::env::temp_dir().join(format!("aion-stabilizer-update-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // skips absolute paths and .. traversal
        };
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut w = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut w).map_err(|e| e.to_string())?;
        }
    }

    if dest.join(APP_EXE).is_file() {
        return Ok(dest);
    }
    if let Ok(rd) = fs::read_dir(&dest) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join(APP_EXE).is_file() {
                return Ok(p);
            }
        }
    }
    Err(format!("{APP_EXE} not found in the downloaded package"))
}

/// Swap `new_dir` contents into `app_dir`. Stages the new exe as `*.new`,
/// renames the running exe to `*.old`, renames `*.new` into place, then
/// copies everything else except KEEP files. On failure the running exe
/// is restored.
pub fn apply(app_dir: &Path, new_dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old_exe = exe.with_extension("exe.old");
    let staged = app_dir.join(format!("{APP_EXE}.new"));
    let final_exe = app_dir.join(APP_EXE);

    let _ = fs::remove_file(&old_exe);
    let _ = fs::remove_file(&staged);

    copy_retry(&new_dir.join(APP_EXE), &staged)?;
    fs::rename(&exe, &old_exe).map_err(|e| format!("cannot move running exe: {e}"))?;
    if let Err(e) = fs::rename(&staged, &final_exe) {
        let _ = fs::rename(&old_exe, &exe);
        return Err(format!("cannot place new exe: {e}"));
    }

    if let Err(e) = copy_tree(new_dir, app_dir) {
        // try to leave a working install: the old exe is still intact
        let _ = fs::rename(&old_exe, &exe);
        return Err(e);
    }
    Ok(())
}

/// Spawn the freshly installed exe and let the caller exit.
pub fn relaunch(app_dir: &Path) -> Result<(), String> {
    Command::new(app_dir.join(APP_EXE))
        .arg("--updated")
        .spawn()
        .map_err(|e| format!("cannot start new version: {e}"))?;
    Ok(())
}

/// Delete the leftover files the updater may leave next to the exe.
/// Only removes our own known names, never user files.
pub fn cleanup_old_files() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let mut names = vec![
        format!("{APP_EXE}.old"),
        format!("{APP_EXE}.new"),
    ];
    // the running exe may have been renamed before the update
    if let Some(stem) = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
    {
        names.push(format!("{stem}.exe.old"));
    }
    for n in names {
        let _ = fs::remove_file(dir.join(n));
    }
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    copy_tree_level(src, dst, true)
}

/// `top` marks the app root, where user-editable KEEP files are skipped.
fn copy_tree_level(src: &Path, dst: &Path, top: bool) -> Result<(), String> {
    let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in rd {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name();
        let s = e.path();
        let d = dst.join(&name);
        if s.is_dir() {
            fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            copy_tree_level(&s, &d, false)?;
            continue;
        }
        let fname = name.to_string_lossy();
        // at the app root skip user-editable files and the already-staged exe
        if top && (KEEP.contains(&fname.as_ref()) || fname == APP_EXE) {
            continue;
        }
        copy_retry(&s, &d)?;
    }
    Ok(())
}

/// Copy with retries - WinDivert files can stay locked briefly after the
/// engine stops.
fn copy_retry(s: &Path, d: &Path) -> Result<(), String> {
    let mut last_err = String::new();
    for _ in 0..8 {
        match fs::copy(s, d) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = e.to_string();
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(format!("cannot write {}: {last_err}", d.display()))
}
