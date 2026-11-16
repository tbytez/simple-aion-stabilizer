//! Signed update manifest: the trust anchor for self-updates.
//!
//! A release ships two assets next to each other: the `*-win64.zip`
//! payload and an independent `*-manifest.txt` that lists the SHA-256 of
//! the zip plus the SHA-256 and size of every payload file inside it.
//! The manifest is Ed25519-signed; the verifying key is pinned below in
//! `UPDATE_PUBLIC_KEY`. Everything fails closed: bad signature, bad
//! format, unsafe paths, size/count overruns, missing files, extra
//! files, or hash mismatches all abort the update.
//!
//! Text format, LF line endings, one field line at a time:
//!
//! ```text
//! aion-stabilizer-manifest 1
//! version <semver>
//! zip <release zip file name>
//! zip-sha256 <64 hex>
//! files <count>
//! <sha256-hex> <size> <relative/path>
//! ... exactly <count> file lines ...
//! signature <128 hex ed25519 signature over every byte above>
//! ```
//!
//! The signature covers the raw bytes of all lines before the
//! `signature` line, including the last newline. `tools/sign-manifest`
//! produces this file; this module is shared with that tool via
//! `#[path]` so producer and verifier can never drift apart.

// The updater + signing tool are the consumers: on non-Windows builds
// of the app most of this is unused, and the tool uses only part of it.
#![allow(dead_code)]

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Main payload binary inside a release zip.
pub const APP_EXE: &str = "AionStabilizer.exe";

// ---------------- limits (finding: bounded downloads/extraction) ----------------

/// Whole release zip download cap. Current zip is ~4.6 MiB.
pub const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
/// Manifest file cap (a 2048-entry manifest is ~350 KiB of text).
pub const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
/// Single zip entry uncompressed cap.
pub const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
/// All zip entries uncompressed, total cap.
pub const MAX_EXPANDED_BYTES: u64 = 256 * 1024 * 1024;
/// Zip entry count cap. Current zip has 43.
pub const MAX_ZIP_ENTRIES: usize = 1024;
/// Manifest file-line cap. Current release has ~43 files.
pub const MAX_MANIFEST_ENTRIES: usize = 2048;
/// Single manifest path length cap.
pub const MAX_PATH_LEN: usize = 512;

/// Ed25519 public key (hex) that signs every update manifest.
///
/// Production verification key. The private key is stored outside git.
pub const UPDATE_PUBLIC_KEY: &str = "1a9cc08eb57007e0fa469fdbf8b900b6e22aebe3512cc1598b20ad56b3c00389";

const MAGIC: &str = "aion-stabilizer-manifest 1";

/// Extraction limits for a downloaded zip.
pub struct ZipLimits {
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
}

pub const ZIP_LIMITS: ZipLimits = ZipLimits {
    max_entries: MAX_ZIP_ENTRIES,
    max_entry_bytes: MAX_ENTRY_BYTES,
    max_total_bytes: MAX_EXPANDED_BYTES,
};

/// One payload file covered by the signature.
pub struct ManifestFile {
    /// `/`-separated path relative to the folder holding APP_EXE
    pub path: String,
    pub sha256: [u8; 32],
    pub size: u64,
}

/// A parsed and signature-verified update manifest.
pub struct Manifest {
    pub version: String,
    /// expected file name of the release zip asset
    pub zip_name: String,
    pub zip_sha256: [u8; 32],
    pub files: Vec<ManifestFile>,
}

// ---------------- hex helpers (keep deps small) ----------------

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in b.chunks_exact(2) {
        out.push(hex_val(pair[0])? * 16 + hex_val(pair[1])?);
    }
    Some(out)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn hex_32(s: &str) -> Result<[u8; 32], String> {
    hex_decode(s)
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| "bad sha256 hex".to_string())
}

// ---------------- path rules ----------------

/// Windows device names that must never appear as file names.
const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "clock$", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn valid_segment(seg: &str) -> bool {
    if seg.is_empty() || seg == "." || seg == ".." {
        return false;
    }
    if seg.ends_with(['.', ' ']) {
        return false;
    }
    let stem = seg.split('.').next().unwrap_or_default().to_lowercase();
    !DEVICE_NAMES.contains(&stem.as_str())
}

/// True when `p` is a safe `/`-separated relative payload path.
/// Rejects traversal, absolute/drive paths, ADS (`:`), wildcards and
/// quoting chars, control chars, empty/dot segments, and Windows
/// device names.
pub fn valid_relpath(p: &str) -> bool {
    if p.is_empty() || p.len() > MAX_PATH_LEN || p.starts_with('/') {
        return false;
    }
    if p.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return false;
    }
    if p.chars().any(|c| matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return false;
    }
    p.split('/').all(valid_segment)
}

/// Join a validated `/`-path onto `root`, one component at a time so no
/// segment can ever escape the root.
fn join_relpath(root: &Path, rel: &str) -> PathBuf {
    let mut out = root.to_path_buf();
    for seg in rel.split('/') {
        out.push(seg);
    }
    out
}

// ---------------- sha256 ----------------

/// Streamed SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut f = fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

// ---------------- parse + verify ----------------

/// Split `text` into (signed body bytes, signature). The signature must
/// sit on the last non-empty line, prefixed `signature `.
fn split_signed(text: &str) -> Result<(&[u8], [u8; 64]), String> {
    let text = text.trim_end_matches('\n');
    let sig_start = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let last = &text[sig_start..];
    let sig_hex = last
        .strip_prefix("signature ")
        .ok_or_else(|| "manifest has no signature line".to_string())?;
    let sig_bytes = hex_decode(sig_hex)
        .and_then(|v| <[u8; 64]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| "manifest signature is not 64-byte hex".to_string())?;
    let body = text
        .get(..sig_start)
        .filter(|b| b.ends_with('\n'))
        .ok_or_else(|| "manifest body is malformed".to_string())?;
    Ok((body.as_bytes(), sig_bytes))
}

/// Verify signature and parse. `key_hex` is the pinned Ed25519 key.
/// Any failure aborts the update - fail closed.
pub fn verify_manifest(text: &str, key_hex: &str) -> Result<Manifest, String> {
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("manifest exceeds size limit".to_string());
    }
    // signatures are computed over LF text; normalize so a CRLF mirror
    // still maps onto the exact signed bytes
    let text = text.replace("\r\n", "\n");
    let (body, sig) = split_signed(&text)?;

    let key = hex_decode(key_hex)
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| "update signing key is not provisioned".to_string())?;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&key)
        .map_err(|_| "update signing key is invalid".to_string())?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig);
    vk.verify_strict(body, &signature)
        .map_err(|_| "manifest signature verification failed".to_string())?;

    parse_body(std::str::from_utf8(body).map_err(|_| "manifest is not UTF-8".to_string())?)
}

/// Verify with the release-pinned public key.
pub fn verify_manifest_pinned(text: &str) -> Result<Manifest, String> {
    verify_manifest(text, UPDATE_PUBLIC_KEY)
}

fn parse_body(body: &str) -> Result<Manifest, String> {
    let mut lines = body.lines();

    if lines.next() != Some(MAGIC) {
        return Err("manifest magic/version mismatch".to_string());
    }
    let mut next = |what: &str| -> Result<String, String> {
        lines
            .next()
            .map(|l| l.to_string())
            .ok_or_else(|| format!("manifest missing {what} line"))
    };
    let field = |line: String, key: &str| -> Result<String, String> {
        line.strip_prefix(&format!("{key} "))
            .map(|v| v.to_string())
            .ok_or_else(|| format!("manifest line out of order, expected {key}"))
    };

    let version = field(next("version")?, "version")?;
    if version.is_empty() || version.len() > 64 {
        return Err("bad manifest version".to_string());
    }
    let zip_name = field(next("zip")?, "zip")?;
    if !valid_relpath(&zip_name) || zip_name.contains('/') || !zip_name.ends_with("-win64.zip") {
        return Err("bad manifest zip name".to_string());
    }
    let zip_sha256 = hex_32(&field(next("zip-sha256")?, "zip-sha256")?)?;
    let count: usize = field(next("files")?, "files")?
        .parse()
        .map_err(|_| "bad manifest file count".to_string())?;
    if count == 0 || count > MAX_MANIFEST_ENTRIES {
        return Err("manifest file count out of range".to_string());
    }

    let mut files = Vec::with_capacity(count);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..count {
        let line = next("file")?;
        let mut it = line.splitn(3, ' ');
        let hash = it.next().unwrap_or_default();
        let size: u64 = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| "bad manifest file size".to_string())?;
        let path = it
            .next()
            .ok_or_else(|| "bad manifest file line".to_string())?
            .to_string();
        if size > MAX_ENTRY_BYTES {
            return Err(format!("manifest entry too large: {path}"));
        }
        if !valid_relpath(&path) {
            return Err(format!("unsafe path in manifest: {path}"));
        }
        if !seen.insert(path.clone()) {
            return Err(format!("duplicate path in manifest: {path}"));
        }
        files.push(ManifestFile {
            path,
            sha256: hex_32(hash)?,
            size,
        });
    }
    if lines.next().is_some() {
        return Err("trailing data before signature".to_string());
    }
    Ok(Manifest {
        version,
        zip_name,
        zip_sha256,
        files,
    })
}

// ---------------- build (shared with tools/sign-manifest) ----------------

/// Canonical manifest body (the bytes that get signed). File order is
/// path-sorted so output is deterministic.
pub fn body_text(version: &str, zip_name: &str, zip_sha256: &[u8; 32], files: &[ManifestFile]) -> String {
    let mut files: Vec<&ManifestFile> = files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut s = String::new();
    s.push_str(MAGIC);
    s.push('\n');
    s.push_str(&format!("version {version}\n"));
    s.push_str(&format!("zip {zip_name}\n"));
    s.push_str(&format!("zip-sha256 {}\n", hex_encode(zip_sha256)));
    s.push_str(&format!("files {}\n", files.len()));
    for f in files {
        s.push_str(&format!("{} {} {}\n", hex_encode(&f.sha256), f.size, f.path));
    }
    s
}

/// Sign the canonical body and return the complete manifest text.
pub fn signed_text(
    version: &str,
    zip_name: &str,
    zip_sha256: &[u8; 32],
    files: &[ManifestFile],
    key: &ed25519_dalek::SigningKey,
) -> String {
    use ed25519_dalek::Signer;
    let body = body_text(version, zip_name, zip_sha256, files);
    let sig = key.sign(body.as_bytes());
    format!("{body}signature {}\n", hex_encode(&sig.to_bytes()))
}

/// Collect a manifest file list from a payload directory (recursive).
/// Paths are `/`-separated relative to `root`; symlinks are refused.
pub fn files_in_tree(root: &Path) -> Result<Vec<ManifestFile>, String> {
    let mut out = Vec::new();
    collect_files(root, root, &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<ManifestFile>) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", p.display()))?;
        if ft.is_symlink() {
            return Err(format!("refusing symlink in payload: {}", p.display()));
        }
        if ft.is_dir() {
            collect_files(root, &p, out)?;
            continue;
        }
        if !ft.is_file() {
            continue; // sockets/fifos cannot ship in a zip anyway
        }
        let rel = p
            .strip_prefix(root)
            .map_err(|_| "path outside payload root".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if !valid_relpath(&rel) {
            return Err(format!("unsafe payload file name: {rel}"));
        }
        out.push(ManifestFile {
            path: rel,
            sha256: sha256_file(&p)?,
            size: p.metadata().map_err(|e| e.to_string())?.len(),
        });
    }
    Ok(())
}

// ---------------- zip extraction with hard limits ----------------

/// Extract `zip_path` into `dest` under `limits`. Entries that cannot be
/// enclosed or are unix symlinks are skipped; anything that would
/// exceed a limit aborts the whole extraction.
pub fn extract_zip_limited(
    zip_path: &Path,
    dest: &Path,
    limits: &ZipLimits,
) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    if archive.len() > limits.max_entries {
        return Err(format!(
            "zip has {} entries (limit {})",
            archive.len(),
            limits.max_entries
        ));
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;

    let mut total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue; // skips absolute paths and .. traversal
        };
        // unix symlink entries are never part of our releases - drop
        // them so a crafted zip cannot plant a reparse point
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            continue;
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if entry.size() > limits.max_entry_bytes {
            return Err(format!("zip entry too large: {}", rel.display()));
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut w = fs::File::create(&out).map_err(|e| e.to_string())?;
        // count the actually decoded bytes - a zip can understate its
        // declared size, so the cap is enforced on real output
        let mut capped = entry.take(limits.max_entry_bytes + 1);
        let wrote = std::io::copy(&mut capped, &mut w).map_err(|e| e.to_string())?;
        if wrote > limits.max_entry_bytes {
            return Err(format!("zip entry too large: {}", rel.display()));
        }
        total += wrote;
        if total > limits.max_total_bytes {
            return Err("zip exceeds total expanded size limit".to_string());
        }
    }
    Ok(())
}

/// Locate the payload root inside an extracted zip: the directory that
/// contains APP_EXE, top level or single subdirectory deep.
pub fn find_payload_root(extracted: &Path) -> Result<PathBuf, String> {
    if extracted.join(APP_EXE).is_file() {
        return Ok(extracted.to_path_buf());
    }
    if let Ok(rd) = fs::read_dir(extracted) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && p.join(APP_EXE).is_file() {
                return Ok(p);
            }
        }
    }
    Err(format!("{APP_EXE} not found in the downloaded package"))
}

/// Verify every manifest file exists on disk under `root` with matching
/// size+hash, and that the tree holds no files the manifest does not
/// cover. Fail closed on any mismatch.
pub fn verify_tree(root: &Path, m: &Manifest) -> Result<(), String> {
    let want: BTreeMap<&str, &ManifestFile> =
        m.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut found = 0usize;
    check_tree(root, root, &want, &mut found)?;
    if found != m.files.len() {
        return Err(format!(
            "package is missing {} manifest file(s)",
            m.files.len() - found
        ));
    }
    Ok(())
}

fn check_tree(
    root: &Path,
    dir: &Path,
    want: &BTreeMap<&str, &ManifestFile>,
    found: &mut usize,
) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", p.display()))?;
        if ft.is_symlink() {
            return Err(format!("symlink in update payload: {}", p.display()));
        }
        if ft.is_dir() {
            check_tree(root, &p, want, found)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .map_err(|_| "path outside payload root".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let Some(mf) = want.get(rel.as_str()) else {
            return Err(format!("file not covered by manifest: {rel}"));
        };
        *found += 1;
        if p.metadata().map_err(|e| e.to_string())?.len() != mf.size {
            return Err(format!("size mismatch for {rel}"));
        }
        if sha256_file(&p)? != mf.sha256 {
            return Err(format!("hash mismatch for {rel}"));
        }
    }
    Ok(())
}

/// Write `bytes` to `path` (used by the signing tool).
pub fn write_text(path: &Path, text: &str) -> Result<(), String> {
    let mut f = fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[7u8; 32])
    }

    fn pub_hex(sk: &ed25519_dalek::SigningKey) -> String {
        hex_encode(&sk.verifying_key().to_bytes())
    }

    fn sample_files() -> Vec<ManifestFile> {
        vec![
            ManifestFile {
                path: "bin/winws.exe".into(),
                sha256: [1u8; 32],
                size: 10,
            },
            ManifestFile {
                path: "AionStabilizer.exe".into(),
                sha256: [2u8; 32],
                size: 20,
            },
        ]
    }

    fn sign_sample(key: &ed25519_dalek::SigningKey) -> String {
        signed_text("1.2.0", "App-v1.2.0-win64.zip", &[9u8; 32], &sample_files(), key)
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "aion-manifest-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode("DEADBEEF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
    }

    #[test]
    fn relpath_rules() {
        assert!(valid_relpath("AionStabilizer.exe"));
        assert!(valid_relpath("bin/winws.exe"));
        assert!(valid_relpath("lists/list-aion.txt"));
        assert!(valid_relpath("a/b/c/d.bin"));
        // traversal / absolute / drive
        assert!(!valid_relpath("../x"));
        assert!(!valid_relpath("a/../b"));
        assert!(!valid_relpath("a/./b"));
        assert!(!valid_relpath("/abs"));
        assert!(!valid_relpath("C:/x"));
        assert!(!valid_relpath("a:b"));
        // separators and wildcards
        assert!(!valid_relpath("a\\b"));
        assert!(!valid_relpath("a//b"));
        assert!(!valid_relpath("a*b"));
        assert!(!valid_relpath("a?b"));
        assert!(!valid_relpath("a\"b"));
        assert!(!valid_relpath("a<b"));
        assert!(!valid_relpath("a|b"));
        // dot/space endings, device names
        assert!(!valid_relpath("a/b."));
        assert!(!valid_relpath("a/b "));
        assert!(!valid_relpath("con.txt"));
        assert!(!valid_relpath("a/NUL"));
        assert!(!valid_relpath("lpt3.dat"));
        // control chars, empty, too long
        assert!(!valid_relpath("a\tb"));
        assert!(!valid_relpath(""));
        assert!(!valid_relpath(&"x".repeat(MAX_PATH_LEN + 1)));
    }

    #[test]
    fn signed_manifest_roundtrip() {
        let key = test_key();
        let text = sign_sample(&key);
        let m = verify_manifest(&text, &pub_hex(&key)).unwrap();
        assert_eq!(m.version, "1.2.0");
        assert_eq!(m.zip_name, "App-v1.2.0-win64.zip");
        assert_eq!(m.zip_sha256, [9u8; 32]);
        assert_eq!(m.files.len(), 2);
        // body sorts paths
        assert_eq!(m.files[0].path, "AionStabilizer.exe");
        assert_eq!(m.files[1].path, "bin/winws.exe");
    }

    #[test]
    fn verification_fails_closed() {
        let key = test_key();
        let text = sign_sample(&key);
        // wrong key
        let other = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        assert!(verify_manifest(&text, &pub_hex(&other)).is_err());
        // tampered body
        let tampered = text.replace("version 1.2.0", "version 9.9.9");
        assert!(verify_manifest(&tampered, &pub_hex(&key)).is_err());
        // tampered signature
        let tampered_sig = format!("{}0", &text[..text.len() - 2]);
        assert!(verify_manifest(&tampered_sig, &pub_hex(&key)).is_err());
        // no signature line
        let body = body_text("1.2.0", "App-v1.2.0-win64.zip", &[9u8; 32], &sample_files());
        assert!(verify_manifest(&body, &pub_hex(&key)).is_err());
        // placeholder key never verifies
        assert!(verify_manifest(&text, UPDATE_PUBLIC_KEY).is_err());
    }

    #[test]
    fn parse_rejects_bad_content() {
        let key = test_key();
        let sign = |files: &[ManifestFile]| {
            signed_text("1.2.0", "App-v1.2.0-win64.zip", &[9u8; 32], files, &key)
        };
        // unsafe and duplicate paths cannot even be produced by the
        // signer-side walker, so craft the text by hand and resign
        let mut bad = body_text("1.2.0", "App-v1.2.0-win64.zip", &[9u8; 32], &[]);
        bad = bad.replace("files 0\n", "files 1\n");
        bad.push_str(&format!("{} 1 ../evil\n", hex_encode(&[0u8; 32])));
        let sig = {
            use ed25519_dalek::Signer;
            key.sign(bad.as_bytes())
        };
        let mtext = format!("{bad}signature {}\n", hex_encode(&sig.to_bytes()));
        assert!(verify_manifest(&mtext, &pub_hex(&key)).is_err());

        // duplicate path
        let mut bad = body_text("1.2.0", "App-v1.2.0-win64.zip", &[9u8; 32], &[]);
        bad = bad.replace("files 0\n", "files 2\n");
        bad.push_str(&format!("{0} 1 a.txt\n{0} 2 a.txt\n", hex_encode(&[0u8; 32])));
        let sig = {
            use ed25519_dalek::Signer;
            key.sign(bad.as_bytes())
        };
        let mtext = format!("{bad}signature {}\n", hex_encode(&sig.to_bytes()));
        assert!(verify_manifest(&mtext, &pub_hex(&key)).is_err());

        // wrong magic
        let m = sign(&sample_files()).replace(MAGIC, "aion-stabilizer-manifest 2");
        assert!(verify_manifest(&m, &pub_hex(&key)).is_err());
    }

    #[test]
    fn extract_enforces_limits() {
        let dir = tmpdir("zip");
        let zpath = dir.join("t.zip");
        // build a small stored zip: a.txt (5B) + dir/b.txt (6B)
        {
            let f = fs::File::create(&zpath).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opt = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("a.txt", opt).unwrap();
            w.write_all(b"12345").unwrap();
            w.start_file("d/b.txt", opt).unwrap();
            w.write_all(b"123456").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.join("x");
        // happy path
        extract_zip_limited(&zpath, &dest, &ZIP_LIMITS).unwrap();
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "12345");
        assert_eq!(fs::read_to_string(dest.join("d/b.txt")).unwrap(), "123456");
        // entry count cap
        let l = ZipLimits {
            max_entries: 1,
            ..ZIP_LIMITS
        };
        assert!(extract_zip_limited(&zpath, &dir.join("x2"), &l).is_err());
        // total expanded cap (5+6 > 10)
        let l = ZipLimits {
            max_total_bytes: 10,
            ..ZIP_LIMITS
        };
        assert!(extract_zip_limited(&zpath, &dir.join("x3"), &l).is_err());
        // per-entry cap (6 > 5)
        let l = ZipLimits {
            max_entry_bytes: 5,
            ..ZIP_LIMITS
        };
        assert!(extract_zip_limited(&zpath, &dir.join("x4"), &l).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_tree_matches_disk() {
        let key = test_key();
        let dir = tmpdir("tree");
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(dir.join("AionStabilizer.exe"), b"exe-bytes").unwrap();
        fs::write(dir.join("bin/winws.exe"), b"winws-bytes").unwrap();
        let files = files_in_tree(&dir).unwrap();
        let m = verify_manifest(
            &signed_text("1.2.0", "z-v1.2.0-win64.zip", &[1u8; 32], &files, &key),
            &pub_hex(&key),
        )
        .unwrap();
        verify_tree(&dir, &m).unwrap();

        // extra untracked file -> fail closed
        fs::write(dir.join("evil.exe"), b"x").unwrap();
        assert!(verify_tree(&dir, &m).is_err());
        fs::remove_file(dir.join("evil.exe")).unwrap();

        // tampered content -> fail
        fs::write(dir.join("bin/winws.exe"), b"tampered!").unwrap();
        assert!(verify_tree(&dir, &m).is_err());
        fs::write(dir.join("bin/winws.exe"), b"winws-bytes").unwrap();

        // missing file -> fail
        fs::remove_file(dir.join("bin/winws.exe")).unwrap();
        assert!(verify_tree(&dir, &m).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_root_layouts() {
        let dir = tmpdir("root");
        // no exe -> error
        assert!(find_payload_root(&dir).is_err());
        // exe in a single subdir
        fs::create_dir_all(dir.join("App")).unwrap();
        fs::write(dir.join("App").join(APP_EXE), b"x").unwrap();
        assert_eq!(find_payload_root(&dir).unwrap(), dir.join("App"));
        // exe at top level wins
        fs::write(dir.join(APP_EXE), b"x").unwrap();
        assert_eq!(find_payload_root(&dir).unwrap(), dir);
        let _ = fs::remove_dir_all(&dir);
    }
}
