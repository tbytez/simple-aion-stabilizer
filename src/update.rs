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
//! anywhere earlier leaves the old install fully intact; the tiny no-exe
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
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const REPO: &str = "tbytez/simple-aion-stabilizer";
const APP_EXE: &str = manifest::APP_EXE;

/// Files the user may have edited - never overwritten by an update.
const KEEP: &[&str] = &["config.ini", "profile.txt"];

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
        if total > 0 {
            let pct = (got * 100 / total).min(100) as u8;
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
    download(&info.zip_url, &zip_path, manifest::MAX_DOWNLOAD_BYTES, &mut |p| {
        progress(Progress::Download(p))
    })?;
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
        Ok(_) => {
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
/// new exe. Exit code for the process; on any failure the install is
/// untouched (the running exe is renamed only after every payload file
/// is already in place).
pub fn finish_update(stage_dir: &Path) -> i32 {
    let result = (|| -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let app_dir = exe
            .parent()
            .ok_or_else(|| "cannot locate install dir".to_string())?
            .to_path_buf();
        let final_exe = app_dir.join(APP_EXE);
        let old_exe = final_exe.with_extension("exe.old");

        // Owning the instance mutex proves the old app exited, keeps any
        // new launch from starting mid-swap, and stops a racing launch's
        // stale-stage cleanup from deleting our payload.
        let mutex = sys::wait_instance_mutex(Duration::from_secs(120))
            .ok_or_else(|| "timed out waiting for the app to exit".to_string())?;

        // Re-verify the signed tree before touching the install: the
        // helper must never apply bytes it did not authenticate itself.
        let text = fs::read_to_string(stage_dir.join("manifest.txt"))
            .map_err(|e| e.to_string())?;
        let m = manifest::verify_manifest_pinned(&text)?;
        let root = manifest::find_payload_root(&stage_dir.join("x"))?;
        let verified = manifest::verify_tree(&root, &m);

        let swap = verified.and_then(|()| apply_tree(&root, &app_dir, &exe, &final_exe, &old_exe));
        sys::release_mutex(mutex);
        swap?;

        Command::new(&final_exe)
            .arg("--updated")
            .spawn()
            .map_err(|e| format!("cannot relaunch: {e}"))?;
        Ok(())
    })();

    let _ = fs::remove_dir_all(stage_dir);
    if result.is_err() {
        // the install is untouched - bring the old app back so the
        // user is not left with a silently vanished window
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = Command::new(dir.join(APP_EXE)).spawn();
            }
        }
        return 1;
    }
    0
}

/// Copy the verified payload into `app_dir`, then swap the exe. Copying
/// first means a failure leaves the old exe untouched; the only no-exe
/// window is the pair of adjacent renames at the end.
fn apply_tree(
    root: &Path,
    app_dir: &Path,
    staged_exe: &Path,
    final_exe: &Path,
    old_exe: &Path,
) -> Result<(), String> {
    copy_tree(root, app_dir)?;

    let _ = fs::remove_file(old_exe);
    if final_exe.is_file() {
        rename_retry(final_exe, old_exe, 8)
            .map_err(|e| format!("cannot move old exe: {e}"))?;
    }
    if let Err(e) = rename_retry(staged_exe, final_exe, 4) {
        // try to leave a working install: the old exe is still intact
        let _ = fs::rename(old_exe, final_exe);
        return Err(format!("cannot place new exe: {e}"));
    }
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
                if is_stage {
                    let _ = fs::remove_dir_all(&p);
                }
            }
        }
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
        let ft = e.file_type().map_err(|e| e.to_string())?;
        // reparse points/links must never be applied to the install
        if ft.is_symlink() {
            return Err(format!("refusing to copy link: {}", s.display()));
        }
        if ft.is_dir() {
            fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            copy_tree_level(&s, &d, false)?;
            continue;
        }
        let fname = name.to_string_lossy();
        // at the app root skip user-editable files and the already-staged exe
        if top && (KEEP.contains(&fname.as_ref()) || fname == APP_EXE) {
            continue;
        }
        // skip untouched files - avoids touching the WinDivert driver
        // image, which stays locked by the kernel for the whole boot
        if d.is_file() && same_file(&s, &d) {
            continue;
        }
        copy_retry(&s, &d)?;
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `foo.sys` -> `foo.sys.old` (rename-aside target for locked files)
fn aside_path(d: &Path) -> PathBuf {
    let mut p = d.as_os_str().to_os_string();
    p.push(".old");
    p.into()
}

/// Rename with retries - the just-relinquished exe can stay locked
/// briefly by a virus scanner.
fn rename_retry(s: &Path, d: &Path, tries: u32) -> Result<(), String> {
    let mut last_err = String::new();
    for _ in 0..tries {
        match fs::rename(s, d) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e.to_string();
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(last_err)
}

/// Copy with retries - engine files can stay locked briefly after it
/// stops. A file that stays locked (loaded WinDivert driver, leftover
/// winws/cygwin process) cannot be opened for writing but CAN be renamed
/// aside, same trick used for the running exe.
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
    if d.is_file() {
        let aside = aside_path(d);
        let _ = fs::remove_file(&aside);
        if fs::rename(d, &aside).is_ok() {
            if let Err(e) = fs::copy(s, d) {
                let _ = fs::rename(&aside, d);
                last_err = e.to_string();
            } else {
                return Ok(());
            }
        }
    }
    Err(format!("cannot write {}: {last_err}", d.display()))
}
