//! Self-update against GitHub releases: check, download, verify, swap,
//! relaunch.
//!
//! A running exe cannot be overwritten on Windows, but it CAN be renamed.
//! Apply order is chosen so a crash at any point leaves a usable install:
//! stage the new exe as `*.new` first, rename the running exe aside to
//! `*.old`, rename `*.new` into place, then copy the rest. The relaunched
//! process deletes the leftovers on startup.
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
//!   is applied; every check fails closed

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

/// Fetch, verify and apply a release end to end, then relaunch.
/// On success the caller should exit; any failure leaves the current
/// install untouched.
pub fn run_update(
    app_dir: &Path,
    info: &ReleaseInfo,
    progress: &mut dyn FnMut(u8),
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
    download(&info.zip_url, &zip_path, manifest::MAX_DOWNLOAD_BYTES, progress)?;
    if manifest::sha256_file(&zip_path)? != m.zip_sha256 {
        return Err("downloaded zip does not match manifest hash".to_string());
    }

    // 3. extract under hard limits inside the private dir, then prove
    //    every extracted file is exactly what the manifest signed
    let xdir = stage.path().join("x");
    manifest::extract_zip_limited(&zip_path, &xdir, &manifest::ZIP_LIMITS)?;
    let root = manifest::find_payload_root(&xdir)?;
    manifest::verify_tree(&root, &m)?;

    // 4. swap + relaunch (staging cleans itself up on drop)
    apply(app_dir, &root)?;
    relaunch(app_dir)
}

/// Swap `new_dir` contents into `app_dir`. Stages the new exe as `*.new`,
/// renames the running exe to `*.old`, renames `*.new` into place, then
/// copies everything else except KEEP files. On failure the running exe
/// is restored.
fn apply(app_dir: &Path, new_dir: &Path) -> Result<(), String> {
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
fn relaunch(app_dir: &Path) -> Result<(), String> {
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
