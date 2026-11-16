//! sign-manifest: release signing tool for Simple Aion Stabilizer.
//!
//! Shares the exact manifest format code with the app via #[path], so
//! what this tool produces is precisely what the updater verifies.
//!
//!   sign-manifest generate [dir]
//!       Create <dir>/update-private.key and <dir>/update-public.key
//!       (default dir: release-keys). Prints the public key hex to pin
//!       in src/manifest.rs UPDATE_PUBLIC_KEY. NEVER commit the private
//!       key - release-keys/ is gitignored.
//!
//!   sign-manifest sign <payload-dir> --key <private.key> --zip <zip>
//!       [--out <manifest.txt>]
//!       Hash every file under <payload-dir> (the folder that becomes
//!       the release zip contents), plus the zip file itself, sign the
//!       manifest, and write it (stdout when --out is omitted).
//!
//!   sign-manifest verify <manifest.txt> <key-or-pubkey-file>
//!       Verify a manifest against a hex public key or a file holding
//!       one. Exit code 0 = valid.
//!
//!   sign-manifest check <manifest.txt> <dir>
//!       Verify every file under <dir> matches the manifest (and no
//!       extra files exist). Release gate: run this against an
//!       extracted copy of the release zip before publishing.

#[path = "../../../src/manifest.rs"]
mod manifest;

use ed25519_dalek::SigningKey;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("generate") => generate(args.get(1).map(|s| s.as_str()).unwrap_or("release-keys")),
        Some("sign") => sign(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("check") => check(&args[1..]),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  sign-manifest generate [dir]\n  sign-manifest sign <payload-dir> --key <private.key> --zip <zip> [--out <manifest.txt>]\n  sign-manifest verify <manifest.txt> <pubkey-hex|pubkey-file>\n  sign-manifest check <manifest.txt> <dir> --key <pubkey-file-or-hex>"
        .to_string()
}

fn opt_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let flag = format!("--{name}");
    args.iter()
        .position(|a| a == &flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn load_signing_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read key {}: {e}", path.display()))?;
    let raw = manifest::hex_decode(text.trim())
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| format!("key {} is not 64-char hex", path.display()))?;
    Ok(SigningKey::from_bytes(&raw))
}

fn load_pubkey(path_or_hex: &str) -> Result<String, String> {
    let p = Path::new(path_or_hex);
    let hex = if p.is_file() {
        std::fs::read_to_string(p).map_err(|e| e.to_string())?
    } else {
        path_or_hex.to_string()
    };
    // sanity: must decode to 32 bytes
    manifest::hex_decode(hex.trim())
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| "not a 32-byte hex public key".to_string())?;
    Ok(hex.trim().to_string())
}

fn generate(dir: &str) -> Result<(), String> {
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let priv_path = dir.join("update-private.key");
    let pub_path = dir.join("update-public.key");
    if priv_path.exists() {
        return Err(format!(
            "{} already exists - refusing to overwrite (delete it first to rotate)",
            priv_path.display()
        ));
    }

    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    let sk = SigningKey::from_bytes(&seed);
    let pk = sk.verifying_key();

    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut private = options.open(&priv_path).map_err(|e| e.to_string())?;
    writeln!(private, "{}", manifest::hex_encode(&sk.to_bytes())).map_err(|e| e.to_string())?;
    manifest::write_text(&pub_path, &format!("{}\n", manifest::hex_encode(&pk.to_bytes())))?;

    println!("wrote {}", priv_path.display());
    println!("wrote {}", pub_path.display());
    println!();
    println!("public key - pin this in src/manifest.rs UPDATE_PUBLIC_KEY:");
    println!("{}", manifest::hex_encode(&pk.to_bytes()));
    println!();
    println!("KEEP THE PRIVATE KEY SECRET. Never commit it (release-keys/ is gitignored).");
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let dir = args
        .first()
        .ok_or_else(usage)?
        .clone();
    let key_path = opt_value(args, "key").ok_or_else(usage)?;
    let zip_path = opt_value(args, "zip").ok_or_else(usage)?;
    let out_path = opt_value(args, "out");

    let payload = PathBuf::from(&dir);
    if !payload.is_dir() {
        return Err(format!("payload dir {} not found", payload.display()));
    }
    let zip = PathBuf::from(zip_path);
    if !zip.is_file() {
        return Err(format!("zip {} not found", zip.display()));
    }
    let zip_name = zip
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "bad zip file name".to_string())?
        .to_string();
    if !zip_name.ends_with("-win64.zip") {
        return Err(format!(
            "zip name must end with -win64.zip (got {zip_name})"
        ));
    }
    let version = zip_name
        .strip_prefix("SimpleAionStabilizer-v")
        .and_then(|s| s.strip_suffix("-win64.zip"))
        .ok_or_else(|| {
            format!("cannot derive version from zip name {zip_name} (expected SimpleAionStabilizer-v<ver>-win64.zip)")
        })?
        .to_string();

    let sk = load_signing_key(Path::new(key_path))?;
    let files = manifest::files_in_tree(&payload)?;
    if !files.iter().any(|f| f.path == manifest::APP_EXE) {
        return Err(format!(
            "payload dir has no {} - refusing to sign",
            manifest::APP_EXE
        ));
    }
    let zip_hash = manifest::sha256_file(&zip)?;
    let text = manifest::signed_text(&version, &zip_name, &zip_hash, &files, &sk);

    match out_path {
        Some(p) => {
            manifest::write_text(Path::new(p), &text)?;
            println!("wrote {p} ({} files covered)", files.len());
        }
        None => print!("{text}"),
    }
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let (mfile, key) = match args {
        [m, k, ..] => (m, k),
        _ => return Err(usage()),
    };
    let text = std::fs::read_to_string(mfile).map_err(|e| e.to_string())?;
    let key_hex = load_pubkey(key)?;
    let m = manifest::verify_manifest(&text, &key_hex)?;
    println!(
        "OK: version {} zip {} ({} files)",
        m.version,
        m.zip_name,
        m.files.len()
    );
    Ok(())
}

/// `check <manifest.txt> <dir> --key <pubkey>`: verify the signature,
/// then prove the tree under <dir> is exactly what was signed.
fn check(args: &[String]) -> Result<(), String> {
    let (mfile, dir) = match args {
        [m, d, ..] => (m, d),
        _ => return Err(usage()),
    };
    let text = std::fs::read_to_string(mfile).map_err(|e| e.to_string())?;
    let Some(key) = opt_value(args, "key").or_else(|| opt_value(args, "pubkey")) else {
        return Err("check needs --key <pubkey-file-or-hex>".to_string());
    };
    let key_hex = load_pubkey(key)?;
    let m = manifest::verify_manifest(&text, &key_hex)?;
    manifest::verify_tree(Path::new(dir), &m)?;
    println!("OK: {} files match manifest", m.files.len());
    Ok(())
}
