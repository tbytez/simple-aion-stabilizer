//! Upgrade only our known stock profile; preserve custom strategies.
use crate::core;
use std::path::Path;

pub const DEFAULT: &str = include_str!("../assets/profile.txt");
const LEGACY: &str = include_str!("../assets/profile-legacy.txt");

fn is_legacy(text: &str) -> bool {
    core::parse_profile_args(text, "", "") == core::parse_profile_args(LEGACY, "", "")
}

/// Updaters preserve profile.txt. Migrate the old all-port strategy on first
/// use, with an exclusive backup before replacing it. Custom profiles survive.
pub fn load(app_dir: &Path) -> Result<String, String> {
    let path = app_dir.join("profile.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(&path, DEFAULT).map_err(|e| format!("Cannot write profile.txt: {e}"))?;
            return Ok(DEFAULT.into());
        }
        Err(e) => return Err(format!("Cannot read profile.txt: {e}")),
    };
    if !is_legacy(&text) {
        return Ok(text);
    }
    // If an earlier attempt already saved the same profile, retry safely.
    let backup = app_dir.join("profile.pre-domain-scope.txt");
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(mut file) => {
            file.write_all(text.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| format!("Cannot back up old profile: {e}"))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::read_to_string(&backup).ok().as_deref() != Some(&text) {
                return Err("Old profile backup already exists with different contents; profile.txt was left unchanged.".into());
            }
        }
        Err(e) => return Err(format!("Cannot back up old profile: {e}")),
    }
    // Commit the replacement in one rename, leaving the original intact
    // if writing fails or the process exits midway through the migration.
    let staged = app_dir.join(format!("profile.next-{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|e| format!("Cannot stage profile migration: {e}"))?;
    let result = file
        .write_all(DEFAULT.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = result.and_then(|_| std::fs::rename(&staged, &path)) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Cannot upgrade profile.txt: {e}"));
    }
    Ok(DEFAULT.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "aion-profile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn migrates_existing_install_and_backs_up() {
        let dir = dir();
        std::fs::write(dir.join("profile.txt"), LEGACY.replace('\n', "\r\n")).unwrap();
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        assert!(is_legacy(
            &std::fs::read_to_string(dir.join("profile.pre-domain-scope.txt")).unwrap()
        ));
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn preserves_custom_and_creates_missing_profile() {
        let dir = dir();
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        let custom = "--wf-tcp=443\n--filter-tcp=443\n--dpi-desync=split2";
        std::fs::write(dir.join("profile.txt"), custom).unwrap();
        assert_eq!(load(&dir).unwrap(), custom);
        assert!(!dir.join("profile.pre-domain-scope.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn default_never_intercepts_udp_or_unknown_protocols() {
        let args = core::parse_profile_args(DEFAULT, "bin/", "lists/");
        assert!(args.contains(&"--wf-tcp=80,443".into()));
        assert!(args.contains(&"--hostlist=lists/list-aion.txt".into()));
        assert!(!args.iter().any(|s| s.contains("udp")
            || s.contains("ipset")
            || s.contains("any-protocol")
            || s.contains("fake")
            || s.contains("seqovl")));
    }
    #[test]
    fn conflicting_backup_does_not_replace_profile() {
        let dir = dir();
        std::fs::write(dir.join("profile.txt"), LEGACY).unwrap();
        std::fs::write(dir.join("profile.pre-domain-scope.txt"), "custom backup").unwrap();
        assert!(load(&dir).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.txt")).unwrap(),
            LEGACY
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
