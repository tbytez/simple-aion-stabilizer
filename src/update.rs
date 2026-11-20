//! Self-update against GitHub releases: check, download, verify, swap,
//! relaunch.
//!
//! A running exe cannot be overwritten on Windows, but it CAN be renamed.
//! The swap is delegated to the staged new exe itself: `run_update` copies
//! it next to the install as `*.new`, spawns it with `--finish-update`,
//! and exits. The helper is a separate headless process, so closing or
//! killing the app mid-update can no longer break the install. It waits
//! for the old instance's mutex (which also blocks other launches during
//! the swap), copies every payload file while the old exe is still in
//! place, and only then swaps the exe with two adjacent renames - a crash
//! anywhere earlier leaves the old executable in place; the tiny no-exe
//! window between the renames is covered by update-recovery.cmd.
//!
//! Security properties of the pipeline (see src/manifest.rs):
//! - all network artifacts live in a private ACL'd staging dir under the
//!   machine temp dir, exclusively created and pinned for the update's
//!   lifetime - an unelevated same-user process cannot squat on, tamper
//!   with, or delete the staging tree (no predictable temp paths)
//! - downloads are byte-capped and the zip is extracted under hard
//!   entry/size/count limits
//! - an Ed25519-signed manifest (pinned public key) must cover the zip
//!   hash and every extracted payload file's hash+size before anything
//!   is applied; the helper re-verifies the tree before touching the
//!   install; every check fails closed

#![cfg(windows)]

use crate::core::{self, ReleaseInfo};
use crate::manifest;
use crate::sys;
use crate::update_install::{apply_tree, copy_retry, rename_retry};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const REPO: &str = "tbytez/simple-aion-stabilizer";
const APP_EXE: &str = manifest::APP_EXE;

/// Cap on the GitHub API response itself.
const MAX_API_BYTES: u64 = 4 * 1024 * 1024;

/// What the updater is doing, for the progress banner. Download progress
/// is the only part with a meaningful percentage.
pub enum Progress {
    Download(u8),
    Verify,
    Install,
}

fn agent(timeout_secs: u64) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .user_agent(concat!("aion-stabilizer/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// Latest GitHub release with a `*-win64.zip` asset AND its matching
/// `*-manifest.txt` asset, newer than the running version.
/// Ok(None) = up to date or no usable/verifiable assets (fail closed).
pub fn check_latest() -> Result<Option<ReleaseInfo>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = agent(15).get(&url).call().map_err(|e| e.to_string())?;
    let mut body = String::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_API_BYTES)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(core::release_from_json(&json, env!("CARGO_PKG_VERSION")))
}

/// One bounded download to `dest` (exclusive create), reporting
/// 0..=100 via `progress`. Aborts when the body exceeds `max_bytes`
/// even if the server lied about Content-Length.
fn download(
    url: &str,
    dest: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u8),
) -> Result<(), String> {
    let mut resp = agent(300).get(url).call().map_err(|e| e.to_string())?;
    let total = resp.body().content_length().unwrap_or(0);
    if total > max_bytes {
        return Err(format!("download too large ({total} bytes)"));
    }

    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| e.to_string())?;

    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut got = 0u64;
    let mut last_pct = 255u8;
    progress(0);
    let result = loop {
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) => break Err(e.to_string()),
        };
        if n == 0 {
            break Ok(());
        }
        got += n as u64;
        if got > max_bytes {
            break Err(format!("download exceeds limit ({max_bytes} bytes)"));
        }
        if let Err(e) = std::io::Write::write_all(&mut out, &buf[..n]) {
            break Err(e.to_string());
        }
        if let Some(pct) = (got * 100).checked_div(total) {
            let pct = pct.min(100) as u8;
            if pct != last_pct {
                last_pct = pct;
                progress(pct);
            }
        }
    };
    drop(out);
    match result {
        Ok(()) if total > 0 && got != total => {
            let _ = fs::remove_file(dest);
            Err(format!("download truncated ({got}/{total} bytes)"))
        }
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(dest);
            Err(e)
        }
    }
}

/// Fetch, verify and stage a release, then hand the actual file swap to
/// the staged new exe (`--finish-update`) and let the caller exit. On
/// success the caller should exit immediately; any failure before the
/// handoff leaves the current install untouched.
pub fn run_update(
    app_dir: &Path,
    info: &ReleaseInfo,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    // Private staging: random name, exclusive create, admin-only ACL,
    // pinned by an open handle until dropped. Prefer the machine temp
    // dir (outside every user's reach); the ACL + pin still protect the
    // tree when we have to fall back to the user temp dir.
    let stage = sys::SecureStage::new(&sys::machine_temp_dir())
        .or_else(|_| sys::SecureStage::new(&std::env::temp_dir()))?;

    // 1. signed manifest first - small, and no point downloading a zip
    //    we cannot authenticate anyway
    let manifest_path = stage.path().join("manifest.txt");
    download(
        &info.manifest_url,
        &manifest_path,
        manifest::MAX_MANIFEST_BYTES,
        &mut |_| {},
    )?;
    let text = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let m = manifest::verify_manifest_pinned(&text)?;
    if m.version != info.version {
        return Err(format!(
            "manifest version {} does not match release {}",
            m.version, info.version
        ));
    }
    let zip_base = info.zip_url.rsplit('/').next().unwrap_or_default();
    if zip_base != m.zip_name {
        return Err(format!("manifest is for {zip_base}, not {}", m.zip_name));
    }

    // 2. the zip payload, byte-capped and hash-checked against the
    //    signed manifest before it is even opened
    let zip_path = stage.path().join("payload.zip");
    download(
        &info.zip_url,
        &zip_path,
        manifest::MAX_DOWNLOAD_BYTES,
        &mut |p| progress(Progress::Download(p)),
    )?;
    progress(Progress::Verify);
    if manifest::sha256_file(&zip_path)? != m.zip_sha256 {
        return Err("downloaded zip does not match manifest hash".to_string());
    }

    // 3. extract under hard limits inside the private dir, then prove
    //    every extracted file is exactly what the manifest signed
    let xdir = stage.path().join("x");
    manifest::extract_zip_limited(&zip_path, &xdir, &manifest::ZIP_LIMITS)?;
    let root = manifest::find_payload_root(&xdir)?;
    manifest::verify_tree(&root, &m)?;

    // 4. stage the new exe next to the install and hand it the swap. The
    //    helper finishes the job after we exit - the user can close or
    //    kill this process without breaking the install. Staging dir
    //    ownership passes to the helper (it deletes it when done).
    progress(Progress::Install);
    let staged = app_dir.join(format!("{APP_EXE}.new"));
    let _ = fs::remove_file(&staged);
    copy_retry(&root.join(APP_EXE), &staged)?;
    match Command::new(&staged)
        .arg("--finish-update")
        .arg(stage.path())
        .spawn()
    {
        Ok(mut child) => {
            // Publish the live helper PID before relinquishing our stage
            // claim. Cleanup must also skip this stage during the brief
            // interval between the parent closing its claim and the helper
            // acquiring it. A failed marker write cancels the handoff.
            if let Err(e) = fs::write(stage.path().join("owner.pid"), child.id().to_string()) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot record update handoff: {e}"));
            }
            std::mem::forget(stage);
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&staged);
            Err(format!("cannot start updater: {e}"))
        }
    }
}

/// Headless `--finish-update <stage_dir>` entry point, run by the staged
/// new exe. Payload copying precedes the executable swap; failed promotion
/// attempts restore the old executable and report the failure explicitly.
pub fn finish_update(stage_dir: &Path) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return 1,
    };
    let Some(app_dir) = exe.parent() else {
        return 1;
    };
    let final_exe = app_dir.join(APP_EXE);
    let old_exe = final_exe.with_extension("exe.old");
    let mut log = format!(
        "Update helper v{}\nInstall: {}\n",
        env!("CARGO_PKG_VERSION"),
        app_dir.display()
    );
    let result = (|| -> Result<(), String> {
        // Claim the stage before waiting for the app. Startup cleanup skips
        // live claims, including during the gap before the helper gets the
        // instance mutex. This also works with older parent updaters.
        let _stage = sys::StageClaim::acquire(stage_dir, Duration::from_secs(120))?;
        let mutex = sys::InstanceMutex::acquire(Duration::from_secs(120))
            .ok_or_else(|| "timed out waiting for the app to exit".to_string())?;
        log.push_str("Previous instance exited; verifying payload.\n");
        let text = fs::read_to_string(stage_dir.join("manifest.txt")).map_err(|e| e.to_string())?;
        let m = manifest::verify_manifest_pinned(&text)?;
        crate::update_install::verify_helper_version(&m, env!("CARGO_PKG_VERSION"))?;
        let root = manifest::find_payload_root(&stage_dir.join("x"))?;
        manifest::verify_tree(&root, &m)?;
        log.push_str(&format!("Installing signed version {}.\n", m.version));
        apply_tree(&root, app_dir, &exe, &final_exe, &old_exe, &m)?;
        log.push_str("Installed executable verified; restarting.\n");
        // Keep the mutex through process creation. --updated waits for actual
        // ownership, then starts only after we release it, never mid-swap.
        Command::new(&final_exe)
            .arg("--updated")
            .current_dir(app_dir)
            .spawn()
            .map_err(|e| format!("cannot relaunch installed version: {e}"))?;
        drop(mutex);
        Ok(())
    })();
    if let Err(error) = result {
        log.push_str(&format!("FAILED: {error}\n"));
        let _ = fs::write(app_dir.join("update.log"), &log);
        // Do not silently reopen the old version and call it an update.
        // A dialog also surfaces failures when an older app invoked us.
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let message = sys::wide(&format!("The update did not complete: {error}\n\nDetails are saved in update.log beside the app. The existing installation will reopen if available. If it does not, run update-recovery.cmd."));
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                sys::wide("Aion Stabilizer update failed").as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        let _ = Command::new(&final_exe).current_dir(app_dir).spawn();
        let _ = fs::remove_dir_all(stage_dir);
        return 1;
    }
    log.push_str("SUCCESS\n");
    let _ = fs::write(app_dir.join("update.log"), log);
    let _ = fs::remove_dir_all(stage_dir);
    0
}

/// Headless `--finish-rollback` entry point, run by the just-installed
/// exe itself. Restores the previous version parked as
/// `AionStabilizer.exe.old`: waits for the app to exit, moves the
/// running (current) exe aside, puts the old binary back in place and
/// relaunches it. Same fail-closed shape as `finish_update` - a failure
/// leaves a runnable exe under the canonical name or reports the
/// problem, never a missing executable.
pub fn finish_rollback() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return 1,
    };
    let Some(app_dir) = exe.parent() else {
        return 1;
    };
    let final_exe = app_dir.join(APP_EXE);
    let old_exe = final_exe.with_extension("exe.old");
    let aside = app_dir.join(format!("{APP_EXE}.new"));
    let mut log = format!(
        "Rollback helper v{}\nInstall: {}\n",
        env!("CARGO_PKG_VERSION"),
        app_dir.display()
    );
    let result = (|| -> Result<(), String> {
        // The app's exit releases the instance mutex; hold it through the
        // relaunch so the restored version starts cleanly.
        let mutex = sys::InstanceMutex::acquire(Duration::from_secs(120))
            .ok_or_else(|| "timed out waiting for the app to exit".to_string())?;
        if !old_exe.is_file() {
            return Err("no previous version found to restore".to_string());
        }
        // This helper runs AS the installed exe; renaming its running
        // image aside is allowed on Windows and frees the canonical name.
        if final_exe.is_file() {
            let _ = fs::remove_file(&aside);
            rename_retry(&final_exe, &aside, 60)
                .map_err(|e| format!("cannot move current version aside: {e}"))?;
        }
        if let Err(e) = rename_retry(&old_exe, &final_exe, 60) {
            // Put the current version back - never leave no exe in place.
            if aside.is_file() {
                let _ = rename_retry(&aside, &final_exe, 60);
            }
            return Err(format!("cannot restore previous version: {e}"));
        }
        log.push_str("Previous version restored; restarting.\n");
        // A saved auto_update=1 would reinstall the same release the
        // user just rolled back from - turn it off for the old version.
        let ini = app_dir.join("config.ini");
        if let Ok(text) = fs::read_to_string(&ini) {
            let _ = fs::write(&ini, core::config_set(&text, "auto_update", "0"));
        }
        Command::new(&final_exe)
            .arg("--updated")
            .current_dir(app_dir)
            .spawn()
            .map_err(|e| format!("cannot relaunch previous version: {e}"))?;
        drop(mutex);
        Ok(())
    })();
    if let Err(error) = result {
        log.push_str(&format!("FAILED: {error}\n"));
        let _ = fs::write(app_dir.join("update.log"), &log);
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let message = sys::wide(&format!("The rollback did not complete: {error}\n\nDetails are saved in update.log beside the app. If the app does not open, run update-recovery.cmd."));
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                sys::wide("Aion Stabilizer rollback failed").as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        if final_exe.is_file() {
            let _ = Command::new(&final_exe).current_dir(app_dir).spawn();
        }
        return 1;
    }
    log.push_str("SUCCESS\n");
    let _ = fs::write(app_dir.join("update.log"), log);
    0
}

/// Delete the leftover files the updater may leave next to the exe.
/// Only removes our own known names, never user files. When
/// `preserve_old_exe` is set (the first `--updated` launch), the
/// previous exe kept as `*.exe.old` is left in place as the rollback
/// source; it is cleaned up on the next ordinary launch instead.
pub fn cleanup_old_files(preserve_old_exe: bool) {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        return;
    };
    let mut names = vec![format!("{APP_EXE}.new")];
    if !preserve_old_exe {
        names.push(format!("{APP_EXE}.old"));
        // the running exe may have been renamed before the update
        if let Some(stem) = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        {
            names.push(format!("{stem}.exe.old"));
        }
    }
    for n in names {
        let _ = fs::remove_file(dir.join(n));
    }
    // *.old files renamed aside during a locked-file update - only look
    // inside our own managed dirs (bin/, lists/), never user files
    for sub in ["bin", "lists"] {
        if let Ok(rd) = fs::read_dir(dir.join(sub)) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() && p.extension().is_some_and(|x| x == "old") {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
}

/// Remove abandoned `aion-stabilizer-update-*` staging dirs left behind
/// by a killed `--finish-update` helper. Runs at startup while we hold
/// the instance mutex, so no live update can be using them.
pub fn cleanup_stale_stages() {
    for parent in [sys::machine_temp_dir(), std::env::temp_dir()] {
        if let Ok(rd) = fs::read_dir(&parent) {
            for e in rd.flatten() {
                let p = e.path();
                let is_stage = p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("aion-stabilizer-update-"));
                if is_stage && !sys::stage_owner_running(&p) {
                    // Merely pinning the directory does not prevent recursive
                    // deletion of its children. Claim it exclusively first.
                    if let Ok(claim) = sys::StageClaim::acquire(&p, Duration::ZERO) {
                        // Keep the claim open throughout recursive deletion;
                        // its own file is removed after releasing the handle.
                        if let Ok(children) = fs::read_dir(&p) {
                            for child in children.flatten() {
                                if child.file_name() == "active.lock" {
                                    continue;
                                }
                                let path = child.path();
                                if path.is_dir() {
                                    let _ = fs::remove_dir_all(path);
                                } else {
                                    let _ = fs::remove_file(path);
                                }
                            }
                        }
                        drop(claim);
                        let _ = fs::remove_dir_all(&p);
                    }
                }
            }
        }
    }
}
