//! Upgrade only our known stock profile; preserve custom strategies.
use crate::core;
use std::path::Path;

pub const DEFAULT: &str = include_str!("../assets/profile.txt");
const LEGACY: &str = include_str!("../assets/profile-legacy.txt");
const WEB: &str = include_str!("../assets/profile-web.txt");
pub const DEFAULT_TARGETS: &str = include_str!("../assets/lists/ipset-aion.txt");

fn is_legacy(text: &str) -> bool {
    core::parse_profile_args(text, "", "") == core::parse_profile_args(LEGACY, "", "")
}

/// Migrate known shipped profiles on first use. Custom strategies survive.
/// Use separate backups for the broad and web-only generations.
pub fn load(app_dir: &Path) -> Result<String, String> {
    let path = app_dir.join("profile.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_profile_atomic(&path, DEFAULT)?;
            return Ok(DEFAULT.into());
        }
        Err(e) => return Err(format!("Cannot read profile.txt: {e}")),
    };
    let backup_name = if is_legacy(&text) {
        "profile.pre-domain-scope.txt"
    } else if core::parse_profile_args(&text, "", "") == core::parse_profile_args(WEB, "", "") {
        "profile.pre-aion-tcp.txt"
    } else {
        return Ok(text);
    };
    backup_profile(app_dir, backup_name, &text)?;
    write_profile_atomic(&path, DEFAULT)?;
    Ok(DEFAULT.into())
}

/// Retain every earlier backup. A prior migration or manual backup must
/// not prevent upgrading a different known stock profile later.
fn backup_profile(app_dir: &Path, name: &str, text: &str) -> Result<(), String> {
    use std::io::Write;
    for suffix in 0..64 {
        let backup = app_dir.join(if suffix == 0 {
            name.to_owned()
        } else {
            format!("{name}.{suffix}")
        });
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(mut file) => {
                return file
                    .write_all(text.as_bytes())
                    .and_then(|_| file.sync_all())
                    .map_err(|e| format!("Cannot back up old profile: {e}"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read_to_string(&backup).ok().as_deref() == Some(text) {
                    return Ok(());
                }
            }
            Err(e) => return Err(format!("Cannot back up old profile: {e}")),
        }
    }
    Err("Too many existing profile backups; profile.txt was left unchanged.".into())
}

/// Publish a complete profile in one rename. Exclusive numbered staging
/// names tolerate leftovers from an interrupted run/PID reuse.
fn write_profile_atomic(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or("Profile path has no parent")?;
    for attempt in 0..64 {
        let staged = parent.join(format!("profile.next-{}-{attempt}.tmp", std::process::id()));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot stage profile: {e}")),
        };
        let result = file
            .write_all(text.as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(e) = result.and_then(|_| std::fs::rename(&staged, path)) {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("Cannot write complete profile: {e}"));
        }
        return Ok(());
    }
    Err("Cannot find an unused profile staging filename; profile.txt was left unchanged.".into())
}

/// Construct the exact argument array used by the Windows engine. Keep
/// loading/migration and target validation in this shared, host-tested path.
pub fn arguments(app_dir: &Path) -> Result<Vec<String>, String> {
    let text = load(app_dir)?;
    let tokens = core::parse_profile_args(&text, "", "");
    if tokens.iter().any(|a| a == "--ipset=ipset-aion.txt") {
        ensure_targets(app_dir)?;
    }
    let bin = format!("{}\\", app_dir.join("bin").display());
    let lists = format!("{}\\", app_dir.join("lists").display());
    Ok(core::parse_profile_args(&text, &bin, &lists))
}

/// An empty ipset can be treated as no include filter by the backend. Fail
/// before starting rather than accidentally broadening this targeted profile.
/// Only create the known default when missing; preserve edited target lists.
pub fn ensure_targets(app_dir: &Path) -> Result<(), String> {
    let path = app_dir.join("lists/ipset-aion.txt");
    if !path.exists() {
        std::fs::create_dir_all(app_dir.join("lists")).map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Cannot create Aion targets: {e}"))?;
        file.write_all(DEFAULT_TARGETS.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    validate_targets(&text)
}

fn validate_targets(text: &str) -> Result<(), String> {
    let mut count = 0;
    for line in text.strip_prefix('\u{feff}').unwrap_or(text).lines() {
        let value = line.split('#').next().unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }
        let (address, prefix) = value.split_once('/').unwrap_or((value, "32"));
        let ip: std::net::Ipv4Addr = address
            .parse()
            .map_err(|_| format!("Invalid IPv4 target: {value}"))?;
        let prefix: u32 = prefix
            .parse()
            .map_err(|_| format!("Invalid target prefix: {value}"))?;
        if prefix == 0 || prefix > 32 {
            return Err(format!(
                "Aion target must be a specific IPv4 address or subnet: {value}"
            ));
        }
        let mask = u32::MAX << (32 - prefix);
        if u32::from(ip) & !mask != 0 {
            return Err(format!("Aion target is not a subnet boundary: {value}"));
        }
        count += 1;
    }
    if count == 0 {
        return Err(
            "Aion target list is empty; add destination addresses to lists/ipset-aion.txt.".into(),
        );
    }
    Ok(())
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
    fn engine_argument_path_migrates_then_validates_targets_before_launch() {
        let dir = dir();
        std::fs::write(dir.join("profile.txt"), WEB).unwrap();
        let args = arguments(&dir).unwrap();
        assert!(args
            .iter()
            .any(|a| a == &format!("--ipset={}\\ipset-aion.txt", dir.join("lists").display())));
        assert!(dir.join("profile.pre-aion-tcp.txt").is_file());
        assert!(dir.join("lists/ipset-aion.txt").is_file());
        std::fs::write(dir.join("lists/ipset-aion.txt"), "# empty").unwrap();
        assert!(arguments(&dir).is_err());
        std::fs::write(
            dir.join("profile.txt"),
            "--wf-tcp=443 --filter-tcp=443 --dpi-desync=multisplit",
        )
        .unwrap();
        assert!(
            arguments(&dir).is_ok(),
            "Custom unrelated profile must not depend on this target list"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn reference_default_keeps_game_tcp_scoped_and_unknown_udp_disabled() {
        let args = core::parse_profile_args(DEFAULT, "bin/", "lists/");
        assert!(args
            .iter()
            .any(|a| a.starts_with("--wf-tcp=") && a.ends_with("1024-65535")));
        assert!(args.contains(&"--wf-udp=443,19294-19344,50000-50100".into()));
        assert!(!args.iter().any(|a| a.contains("ipset-all")
            || a.contains("0.0.0.0/0")
            || a.contains("fake-unknown-udp")
            || a == "--filter-udp=12"));
        let groups: Vec<_> = args[2..].split(|a| a == "--new").collect();
        assert_eq!(groups.len(), 8);
        for group in &groups {
            // Unknown game traffic may only be manipulated on scoped TCP.
            if group.contains(&"--dpi-desync-any-protocol=1".into()) {
                assert!(group.contains(&"--filter-tcp=1024-65535".into()));
                assert!(group.contains(&"--ipset=lists/ipset-aion.txt".into()));
                assert!(group.contains(&"--dpi-desync-cutoff=n3".into()));
                assert!(group.contains(&"--dpi-desync-split-seqovl=568".into()));
                assert!(group.contains(
                    &"--dpi-desync-split-seqovl-pattern=bin/tls_clienthello_4pda_to.bin".into()
                ));
            }
            // Every rule must have an IP, hostname or recognized-protocol scope.
            assert!(group.iter().any(|a| a.starts_with("--ipset=")
                || a.starts_with("--hostlist=")
                || a.starts_with("--hostlist-domains=")
                || a == "--filter-l7=discord,stun"));
        }
        assert_eq!(
            groups
                .iter()
                .filter(|g| g.contains(&"--ipset=lists/ipset-aion.txt".into()))
                .count(),
            3
        );
        assert!(groups
            .iter()
            .any(|g| g.contains(&"--filter-tcp=80,443".into())
                && g.contains(&"--hostlist=lists/list-general.txt".into())));
        validate_targets(DEFAULT_TARGETS).unwrap();
    }
    #[test]
    fn web_only_profile_upgrades_without_overwriting_earlier_backup() {
        let dir = dir();
        std::fs::write(dir.join("profile.txt"), WEB).unwrap();
        std::fs::write(dir.join("profile.pre-domain-scope.txt"), LEGACY).unwrap();
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.pre-aion-tcp.txt")).unwrap(),
            WEB
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.pre-domain-scope.txt")).unwrap(),
            LEGACY
        );
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn empty_or_global_targets_fail_and_valid_edits_survive() {
        let dir = dir();
        ensure_targets(&dir).unwrap();
        for value in [
            "# empty",
            "0.0.0.0/0",
            "193.202.112.1/24",
            "hostname.example",
        ] {
            std::fs::write(dir.join("lists/ipset-aion.txt"), value).unwrap();
            assert!(ensure_targets(&dir).is_err());
        }
        let custom = "203.0.113.7/32\n";
        std::fs::write(dir.join("lists/ipset-aion.txt"), custom).unwrap();
        ensure_targets(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("lists/ipset-aion.txt")).unwrap(),
            custom
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn conflicting_backup_is_preserved_and_migration_uses_another_name() {
        let dir = dir();
        std::fs::write(dir.join("profile.txt"), LEGACY).unwrap();
        std::fs::write(dir.join("profile.pre-domain-scope.txt"), "custom backup").unwrap();
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.pre-domain-scope.txt")).unwrap(),
            "custom backup"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.pre-domain-scope.txt.1")).unwrap(),
            LEGACY
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bom_stock_profile_and_stale_temporary_file_do_not_block_migration() {
        let dir = dir();
        let input = format!("\u{feff}{}", WEB.replace('\n', "\r\n"));
        std::fs::write(dir.join("profile.txt"), &input).unwrap();
        let stale = dir.join(format!("profile.next-{}-0.tmp", std::process::id()));
        std::fs::write(&stale, "incomplete old write").unwrap();
        assert_eq!(load(&dir).unwrap(), DEFAULT);
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.pre-aion-tcp.txt")).unwrap(),
            input
        );
        assert_eq!(
            std::fs::read_to_string(stale).unwrap(),
            "incomplete old write"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("profile.txt")).unwrap(),
            DEFAULT
        );
        assert!(validate_targets("\u{feff}# target list\r\n193.202.112.0/24\r\n").is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_profile_failure_keeps_existing_profile_and_removes_new_temp() {
        let dir = dir();
        let protected = dir.join("profile.txt");
        std::fs::create_dir(&protected).unwrap();
        std::fs::write(protected.join("keep"), "existing data").unwrap();
        assert!(write_profile_atomic(&protected, DEFAULT).is_err());
        assert_eq!(
            std::fs::read_to_string(protected.join("keep")).unwrap(),
            "existing data"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
