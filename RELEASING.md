# Releasing

How to cut a release of Simple Aion Stabilizer. The in-app updater only
installs a release when it can verify the signed update manifest, so a
release is not publishable until the manifest exists.

## One-time: signing key

The updater verifies a manifest signed with Ed25519; the public key is
pinned in `src/manifest.rs` (`UPDATE_PUBLIC_KEY`) and the private key
never enters git (`release-keys/` and `*.key` are gitignored).

```sh
cargo run -p sign-manifest -- generate release-keys
```

- writes `release-keys/update-private.key` (0600) and
  `release-keys/update-public.key`
- prints the public key hex - paste it into `UPDATE_PUBLIC_KEY` in
  `src/manifest.rs`
- **keep the private key secret**: anyone holding it can push updates
  the app will install. Store a backup offline. `generate` refuses to
  overwrite an existing key; delete it first to rotate (rotating means
  every install must first update to a build with the new public key).

`AION_UPDATE_KEY=<path>` overrides the private-key location for
`package.sh`.

## Steps

1. Bump `version` in `Cargo.toml` (semver; the updater compares it
   against the tag).
2. Build + assemble: `./package.sh <path-to-extracted-zapret-bundle>`
   - cross-compiles `AionStabilizer.exe`
   - builds `dist/SimpleAionStabilizer-v<ver>-win64.zip`
   - checks every profile file dependency and the pinned AXV-reference backend
     hashes through `tools/validate-payload.sh`; this rejects incomplete bundles
     and requires deliberate review when updating the backend
   - builds with Cargo.lock enforced and an explicit target directory, preventing
     CARGO_TARGET_DIR from making packaging pick an older executable
   - signs `dist/SimpleAionStabilizer-v<ver>-manifest.txt` (skipped
     with a loud warning when no signing key exists)
3. Verify the artifacts before publishing:

   ```sh
   unzip -q dist/SimpleAionStabilizer-v<ver>-win64.zip -d /tmp/x
   cargo run -p sign-manifest -- \
       verify dist/SimpleAionStabilizer-v<ver>-manifest.txt \
              release-keys/update-public.key
   cargo run -p sign-manifest -- \
       check dist/SimpleAionStabilizer-v<ver>-manifest.txt \
             /tmp/x/SimpleAionStabilizer \
             --key release-keys/update-public.key
   ```

   `verify` proves the signature; `check` proves the zip contents are
   exactly the signed files.
4. Create the GitHub release: tag `v<ver>` and title exactly `v<ver>`
   (no suffixes or extra text in the title - details go in the notes
   body) and upload BOTH assets:
   - `SimpleAionStabilizer-v<ver>-win64.zip`
   - `SimpleAionStabilizer-v<ver>-manifest.txt`

   Both are required: the updater only offers a release when the zip
   and its matching manifest asset are present (fail closed).

## Release gates

- The signing key must match the public key pinned in `src/manifest.rs`.
- Missing `release-keys/update-private.key` (or `AION_UPDATE_KEY`) at
  package time -> no manifest is produced; do not publish the zip
  alone, the app will never offer it.
- Private key must never be committed (`git status` should never list
  `*.key`).

## v0.1.8 validation status

See docs/PRE-RELEASE-AUDIT.md for the artifact audit and remaining
native checks. Cross-compilation and host tests do not establish successful
mixed-DPI dragging, live Aion login, or a native Windows update/relaunch.
Retain these unverified runtime checks in the release notes.
