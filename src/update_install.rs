//! Filesystem portion of the update, shared with host-side regression tests.
use crate::manifest::{self, Manifest};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const APP_EXE: &str = manifest::APP_EXE;
const KEEP: &[&str] = &["config.ini", "profile.txt"];

/// Prove that the helper we are about to promote is the executable signed
/// in the manifest, not a leftover or a mislabeled release binary.
pub fn verify_helper_version(manifest: &Manifest, running_version: &str) -> Result<(), String> {
    if manifest.version != running_version {
        return Err(format!(
            "update helper version {running_version} does not match signed release {}",
            manifest.version
        ));
    }
    Ok(())
}

pub fn verify_executable(path: &Path, manifest: &Manifest) -> Result<(), String> {
    let expected = manifest
        .files
        .iter()
        .find(|f| f.path == APP_EXE)
        .ok_or("manifest has no application executable")?;
    if fs::metadata(path).map_err(|e| e.to_string())?.len() != expected.size
        || manifest::sha256_file(path)? != expected.sha256
    {
        return Err("application executable does not match the signed update".into());
    }
    Ok(())
}

pub fn apply_tree(
    root: &Path,
    app_dir: &Path,
    staged_exe: &Path,
    final_exe: &Path,
    old_exe: &Path,
    manifest: &Manifest,
) -> Result<(), String> {
    verify_executable(staged_exe, manifest)?;
    copy_tree(root, app_dir)?;
    swap_executable(staged_exe, final_exe, old_exe, manifest, rename_retry)
}

fn swap_executable(
    staged: &Path,
    installed: &Path,
    old: &Path,
    manifest: &Manifest,
    mut rename: impl FnMut(&Path, &Path, u32) -> Result<(), String>,
) -> Result<(), String> {
    verify_executable(staged, manifest)?;
    if old.exists() {
        remove_retry(old, 120)
            .map_err(|e| format!("cannot remove previous executable backup: {e}"))?;
    }
    let had_old = installed.is_file();
    if had_old {
        rename(installed, old, 120).map_err(|e| format!("cannot move old executable: {e}"))?;
    }
    let result =
        rename(staged, installed, 120).and_then(|()| verify_executable(installed, manifest));
    if let Err(error) = result {
        // If promotion or verification failed, explicitly restore the old
        // executable. Report rollback failure too; never call this success.
        if had_old {
            if installed.exists() {
                // The new executable may be this running helper. Windows
                // permits renaming its image, but not deleting it in place.
                rename(installed, staged, 120)
                    .map_err(|e| format!("{error}; cannot move failed executable aside: {e}"))?;
            }
            rename(old, installed, 120).map_err(|e| format!("{error}; rollback failed: {e}"))?;
        }
        return Err(format!("update executable swap failed: {error}"));
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    copy_tree_level(src, dst, dst, true)
}

/// `top` marks the app root, where user-editable KEEP files are skipped.
fn copy_tree_level(src: &Path, dst: &Path, install_root: &Path, top: bool) -> Result<(), String> {
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
            copy_tree_level(&s, &d, install_root, false)?;
            continue;
        }
        let fname = name.to_string_lossy();
        // at the app root skip user-editable files and the already-staged exe
        if top && (KEEP.contains(&fname.as_ref()) || fname == APP_EXE) {
            continue;
        }
        // Aion destinations are editable connection configuration. Install
        // the default on first update, but retain an existing edited list.
        if d.is_file()
            && d.strip_prefix(install_root).ok() == Some(Path::new("lists/ipset-aion.txt"))
        {
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

fn remove_retry(path: &Path, tries: u32) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..tries {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(last)
}

/// Rename with retries - the just-relinquished exe can stay locked
/// briefly by a virus scanner.
pub fn rename_retry(s: &Path, d: &Path, tries: u32) -> Result<(), String> {
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
pub fn copy_retry(s: &Path, d: &Path) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        dir: PathBuf,
        root: PathBuf,
        install: PathBuf,
        manifest: Manifest,
    }
    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "aion-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let root = dir.join("payload");
            let install = dir.join("installed");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&install).unwrap();
            fs::write(root.join(APP_EXE), b"new version binary").unwrap();
            fs::write(root.join("profile.txt"), b"stock profile").unwrap();
            fs::write(root.join("config.ini"), b"stock config").unwrap();
            fs::write(root.join("README.md"), b"new readme").unwrap();
            fs::write(install.join(APP_EXE), b"old version binary").unwrap();
            fs::write(install.join("profile.txt"), b"custom profile").unwrap();
            fs::write(install.join("config.ini"), b"custom config").unwrap();
            fs::copy(root.join(APP_EXE), install.join(format!("{APP_EXE}.new"))).unwrap();
            let manifest = Manifest {
                version: "9.0.0".into(),
                zip_name: "test.zip".into(),
                zip_sha256: [0; 32],
                files: manifest::files_in_tree(&root).unwrap(),
            };
            Self {
                dir,
                root,
                install,
                manifest,
            }
        }
        fn paths(&self) -> (PathBuf, PathBuf, PathBuf) {
            (
                self.install.join(format!("{APP_EXE}.new")),
                self.install.join(APP_EXE),
                self.install.join(format!("{APP_EXE}.old")),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn one_handoff_installs_exact_new_binary_and_preserves_settings() {
        let f = Fixture::new();
        let (staged, installed, old) = f.paths();
        apply_tree(&f.root, &f.install, &staged, &installed, &old, &f.manifest).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"new version binary");
        assert_eq!(fs::read(&old).unwrap(), b"old version binary");
        assert_eq!(
            fs::read(f.install.join("profile.txt")).unwrap(),
            b"custom profile"
        );
        assert_eq!(
            fs::read(f.install.join("config.ini")).unwrap(),
            b"custom config"
        );
        assert_eq!(
            fs::read(f.install.join("README.md")).unwrap(),
            b"new readme"
        );
        assert!(!staged.exists());
    }
    #[test]
    fn helper_must_be_the_signed_release_version() {
        let f = Fixture::new();
        assert!(verify_helper_version(&f.manifest, "9.0.0").is_ok());
        assert!(verify_helper_version(&f.manifest, "8.0.0").is_err());
    }
    #[test]
    fn update_preserves_existing_aion_targets_but_installs_missing_list() {
        for existing in [false, true] {
            let mut f = Fixture::new();
            let (staged, installed, old) = f.paths();
            fs::create_dir(f.root.join("lists")).unwrap();
            fs::write(f.root.join("lists/ipset-aion.txt"), b"193.202.112.0/24\n").unwrap();
            f.manifest.files = manifest::files_in_tree(&f.root).unwrap();
            if existing {
                fs::create_dir(f.install.join("lists")).unwrap();
                fs::write(f.install.join("lists/ipset-aion.txt"), b"203.0.113.7/32\n").unwrap();
            }
            apply_tree(&f.root, &f.install, &staged, &installed, &old, &f.manifest).unwrap();
            assert_eq!(
                fs::read(f.install.join("lists/ipset-aion.txt")).unwrap(),
                if existing {
                    b"203.0.113.7/32\n".as_slice()
                } else {
                    b"193.202.112.0/24\n".as_slice()
                }
            );
        }
    }
    #[test]
    fn one_install_then_first_start_migrates_real_stock_profiles() {
        for previous in [
            include_str!("../assets/profile-legacy.txt"),
            include_str!("../assets/profile-web.txt"),
        ] {
            let mut f = Fixture::new();
            let (staged, installed, old) = f.paths();
            fs::write(f.install.join("profile.txt"), previous).unwrap();
            fs::write(f.root.join("profile.txt"), crate::profile::DEFAULT).unwrap();
            fs::create_dir(f.root.join("lists")).unwrap();
            fs::write(
                f.root.join("lists/ipset-aion.txt"),
                crate::profile::DEFAULT_TARGETS,
            )
            .unwrap();
            f.manifest.files = manifest::files_in_tree(&f.root).unwrap();
            apply_tree(&f.root, &f.install, &staged, &installed, &old, &f.manifest).unwrap();
            // The updater preserves user settings; the new app performs the
            // stock migration on its very first start, without another update.
            assert_eq!(
                fs::read_to_string(f.install.join("profile.txt")).unwrap(),
                previous
            );
            let args = crate::profile::arguments(&f.install).unwrap();
            assert!(args.iter().any(|a| a == "--filter-tcp=1024-65535"));
            assert_eq!(
                fs::read_to_string(f.install.join("profile.txt")).unwrap(),
                crate::profile::DEFAULT
            );
            assert_eq!(crate::profile::arguments(&f.install).unwrap(), args);
            assert_eq!(
                fs::read_to_string(f.install.join("config.ini")).unwrap(),
                "custom config"
            );
            verify_executable(&installed, &f.manifest).unwrap();
        }
    }
    #[test]
    fn stale_helper_is_rejected_before_any_payload_changes() {
        let f = Fixture::new();
        let (staged, installed, old) = f.paths();
        fs::write(&staged, b"old version binary").unwrap();
        assert!(apply_tree(&f.root, &f.install, &staged, &installed, &old, &f.manifest).is_err());
        assert_eq!(fs::read(installed).unwrap(), b"old version binary");
        assert!(!old.exists());
        assert!(!f.install.join("README.md").exists());
    }
    #[test]
    fn failed_promotion_restores_old_executable_and_reports_failure() {
        let f = Fixture::new();
        let (staged, installed, old) = f.paths();
        let result = swap_executable(&staged, &installed, &old, &f.manifest, |src, dst, _| {
            if src == staged {
                return Err("simulated antivirus lock".into());
            }
            fs::rename(src, dst).map_err(|e| e.to_string())
        });
        assert!(result.unwrap_err().contains("simulated antivirus lock"));
        assert_eq!(fs::read(installed).unwrap(), b"old version binary");
        assert!(!old.exists());
    }
    #[test]
    fn corruption_after_rename_is_detected_and_rolled_back() {
        let f = Fixture::new();
        let (staged, installed, old) = f.paths();
        let result = swap_executable(&staged, &installed, &old, &f.manifest, |src, dst, _| {
            fs::rename(src, dst).map_err(|e| e.to_string())?;
            if src == staged {
                fs::write(dst, b"corrupt").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(installed).unwrap(), b"old version binary");
    }
    #[test]
    fn payload_copy_failure_keeps_old_executable_in_place() {
        let f = Fixture::new();
        let (staged, installed, old) = f.paths();
        fs::create_dir(f.root.join("bin")).unwrap();
        fs::write(f.root.join("bin/driver"), b"driver").unwrap();
        fs::write(f.install.join("bin"), b"obstructing file").unwrap();
        assert!(apply_tree(&f.root, &f.install, &staged, &installed, &old, &f.manifest).is_err());
        assert_eq!(fs::read(installed).unwrap(), b"old version binary");
        assert!(!old.exists());
    }
}
