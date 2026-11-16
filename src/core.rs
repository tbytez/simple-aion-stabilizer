//! Pure logic with no Windows dependencies - unit-testable on any OS.

/// Parse profile.txt content into a winws argument vector.
///
/// `#` starts a comment line, tokens are whitespace-separated, and
/// `{BIN}` / `{LISTS}` are replaced with the given directories.
#[allow(dead_code)] // used on Windows + in tests
pub fn parse_profile_args(text: &str, bin_dir: &str, lists_dir: &str) -> Vec<String> {
    let mut args = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        for tok in line.split_whitespace() {
            args.push(tok.replace("{BIN}", bin_dir).replace("{LISTS}", lists_dir));
        }
    }
    args
}

/// Read a `key=1` / `key=0` boolean from config.ini text. Missing → `default`.
pub fn config_get_bool(text: &str, key: &str, default: bool) -> bool {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(v) = rest.strip_prefix('=') {
                return v.trim() == "1";
            }
        }
    }
    default
}

/// Read a comma-separated list value, e.g. `watch_process=a.exe,b.exe`.
/// Returns default when key missing or list empty after cleaning.
pub fn config_get_list(text: &str, key: &str, default: &[&str]) -> Vec<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(v) = rest.strip_prefix('=') {
                let items: Vec<String> = v
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty() && s.ends_with(".exe"))
                    .collect();
                if !items.is_empty() {
                    return items;
                }
            }
        }
    }
    default.iter().map(|s| s.to_string()).collect()
}

/// Set (or add) a `key=value` line, preserving all other lines and comments.
pub fn config_set(text: &str, key: &str, value: &str) -> String {
    let mut out = String::new();
    let mut found = false;
    for line in text.lines() {
        let t = line.trim();
        let is_key = t
            .strip_prefix(key)
            .map(|r| r.starts_with('='))
            .unwrap_or(false);
        if is_key && !found {
            out.push_str(&format!("{key}={value}\n"));
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

/// Process names the watcher looks for by default (overridable in config.ini).
pub const DEFAULT_WATCH_PROCESSES: &[&str] = &["aion2.exe", "aion.bin", "aion.exe", "aion64.bin"];

// ---------------- update check helpers ----------------

/// Parse "v1.2.3" / "1.2.3" / "1.2.3-beta" into (1,2,3). Missing parts
/// default to 0. Returns None when the leading token is not numeric.
#[allow(dead_code)] // used on Windows + in tests
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split(['-', '+']).next().unwrap_or("");
    let mut parts = s.splitn(4, '.');
    let mut out = [0u32; 3];
    for slot in &mut out {
        match parts.next() {
            Some(p) => *slot = p.trim().parse().ok()?,
            None => break,
        }
    }
    Some((out[0], out[1], out[2]))
}

/// True when release `tag` names a strictly newer version than `current`.
#[allow(dead_code)] // used on Windows + in tests
pub fn version_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(t), Some(c)) => t > c,
        _ => false,
    }
}

/// Extract (version, download-url) from a GitHub `releases/latest` JSON
/// body when it carries a `*-win64.zip` asset and is newer than `current`.
#[allow(dead_code)] // used on Windows + in tests
pub fn release_from_json(json: &serde_json::Value, current: &str) -> Option<(String, String)> {
    let tag = json.get("tag_name")?.as_str()?;
    if !version_newer(tag, current) {
        return None;
    }
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    let assets = json.get("assets")?.as_array()?;
    for a in assets {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let url = a
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if name.ends_with("-win64.zip") && url.starts_with("https://") {
            return Some((version, url.to_string()));
        }
    }
    None
}

/// True when any watch name appears as an image name in `tasklist /FO CSV`
/// output. Locale-independent: only matches the quoted image-name column.
/// Kept for tests and as a fallback reference; the runtime uses Toolhelp.
#[allow(dead_code)]
pub fn process_in_tasklist(csv: &str, watch_names: &[String]) -> bool {
    for line in csv.lines() {
        let line = line.trim();
        if !line.starts_with('"') {
            continue;
        }
        // first CSV field = image name
        if let Some(end) = line[1..].find('"') {
            let image = line[1..1 + end].to_lowercase();
            if watch_names.iter().any(|n| *n == image) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_parses_tokens_and_placeholders() {
        let text = "# comment\n--wf-tcp=80,443\n\n--filter-tcp=443 --hostlist={LISTS}a.txt --dpi-desync-fake={BIN}x.bin --new\n# another\n--filter-udp=443\n";
        let args = parse_profile_args(text, "C:\\bin\\", "C:\\lists\\");
        assert_eq!(args[0], "--wf-tcp=80,443");
        assert_eq!(args[1], "--filter-tcp=443");
        assert_eq!(args[2], "--hostlist=C:\\lists\\a.txt");
        assert_eq!(args[3], "--dpi-desync-fake=C:\\bin\\x.bin");
        assert_eq!(args[4], "--new");
        assert_eq!(args[5], "--filter-udp=443");
        assert_eq!(args.len(), 6);
    }

    #[test]
    fn profile_handles_crlf_empty_and_unicode() {
        assert_eq!(parse_profile_args("", "b", "l"), Vec::<String>::new());
        assert_eq!(parse_profile_args("# only comments\n# x", "b", "l").len(), 0);
        let args = parse_profile_args("--hostlist={LISTS}файл.txt\r\n--x", "b", "l");
        assert_eq!(args[0], "--hostlist=lфайл.txt");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn profile_stress_many_lines() {
        let mut text = String::new();
        for i in 0..10_000 {
            text.push_str(&format!("--arg{i}={{BIN}}f.bin --arg{i}b={{LISTS}}l.txt --new\n# c\n"));
        }
        let args = parse_profile_args(&text, "B", "L");
        assert_eq!(args.len(), 30_000);
        assert!(args.iter().all(|a| !a.contains('{')));
    }

    #[test]
    fn config_bool_parse() {
        let t = "autostart_engine=1\nwatch_game=0\n";
        assert!(config_get_bool(t, "autostart_engine", false));
        assert!(!config_get_bool(t, "watch_game", true));
        assert!(config_get_bool(t, "missing", true));
        assert!(!config_get_bool("", "x", false));
        // key prefix must not collide: watch_game != watch
        assert!(!config_get_bool("watch_game=1", "watch", false));
    }

    #[test]
    fn config_list_parse() {
        let t = "watch_process=AION2.EXE, foo.exe ,,\n";
        assert_eq!(
            config_get_list(t, "watch_process", &["d.exe"]),
            vec!["aion2.exe".to_string(), "foo.exe".to_string()]
        );
        // falls back on empty/garbage
        assert_eq!(config_get_list("watch_process=,,\n", "watch_process", &["d.exe"]), vec!["d.exe"]);
        // missing key -> default
        assert_eq!(
            config_get_list("x=1\n", "watch_process", DEFAULT_WATCH_PROCESSES).len(),
            DEFAULT_WATCH_PROCESSES.len()
        );
    }

    #[test]
    fn config_set_preserves_other_lines() {
        let t = "# comment\na=1\nb=2\n";
        let t2 = config_set(t, "b", "0");
        assert!(t2.contains("# comment"));
        assert!(t2.contains("a=1"));
        assert!(t2.contains("b=0"));
        assert!(!t2.contains("b=2"));
        // adds when missing
        let t3 = config_set(&t2, "new", "x");
        assert!(t3.contains("new=x"));
        // replaces only first occurrence
        let t4 = config_set("k=1\nk=2\n", "k", "9");
        assert_eq!(t4.matches("k=9").count(), 1);
    }

    #[test]
    fn version_compare() {
        assert!(version_newer("v1.0.1", "1.0.0"));
        assert!(version_newer("1.1.0", "1.0.9"));
        assert!(version_newer("v2.0.0", "1.9.9"));
        assert!(version_newer("1.0.1-beta", "1.0.0"));
        assert!(!version_newer("v1.0.0", "1.0.0"));
        assert!(!version_newer("v0.9.9", "1.0.0"));
        assert!(!version_newer("garbage", "1.0.0"));
        assert!(!version_newer("v1.0.0", "garbage"));
        // two-part tags
        assert!(version_newer("v1.1", "1.0.9"));
        assert!(!version_newer("v1.0", "1.0.0"));
    }

    #[test]
    fn release_json_parsing() {
        let json = serde_json::json!({
            "tag_name": "v1.2.0",
            "assets": [
                {"name": "source.tar.gz", "browser_download_url": "https://x/src"},
                {"name": "SimpleAionStabilizer-v1.2.0-win64.zip",
                 "browser_download_url": "https://x/app.zip"}
            ]
        });
        let (v, url) = release_from_json(&json, "1.0.0").unwrap();
        assert_eq!(v, "1.2.0");
        assert_eq!(url, "https://x/app.zip");

        // same version -> no update
        assert!(release_from_json(&json, "1.2.0").is_none());
        // no matching asset
        let nozip = serde_json::json!({"tag_name": "v9.9.9", "assets": [{"name": "src.zip", "browser_download_url": "https://x/a"}]});
        assert!(release_from_json(&nozip, "1.0.0").is_none());
        // assets missing entirely
        let noassets = serde_json::json!({"tag_name": "v9.9.9"});
        assert!(release_from_json(&noassets, "1.0.0").is_none());
    }

    #[test]
    fn tasklist_matching() {
        let csv = "\"Image Name\",\"PID\",\"Session Name\",\"Session#\",\"Mem Usage\"\r\n\"aion2.exe\",\"1234\",\"Console\",\"1\",\"500,000 K\"\r\n\"svchost.exe\",\"88\",\"Console\",\"0\",\"5,000 K\"\r\n";
        let names = vec!["aion2.exe".to_string()];
        assert!(process_in_tasklist(csv, &names));
        assert!(!process_in_tasklist(csv, &["aion.bin".to_string()]));
        // no match message (locale may vary - we don't rely on it)
        let none = "INFO: No tasks are running which match the specified criteria.";
        assert!(!process_in_tasklist(none, &names));
        // partial names do not match (aion2_extra.exe != aion2.exe)
        assert!(!process_in_tasklist("\"aion2_extra.exe\",\"1\"", &names));
        // stress: big list, last entry matches
        let big = format!("{}\"aion64.bin\",\"9\"", "\"x.exe\",\"1\"\r\n".repeat(50_000));
        assert!(process_in_tasklist(&big, &["aion64.bin".to_string()]));
    }
}
