# Atmos Album Builder

Atmos Album Builder is a local-first macOS application for turning folders of
Dolby Atmos M4A or MKA tracks into chaptered album MKVs or verified individual MKVs. It stream-copies the audio,
adds a 1920×1080 still-image or looping animation video track, writes editable album metadata, and
verifies that the compressed audio packet hash is unchanged before committing
the output.

## Workflow

1. Add a library root. Every visible folder directly containing `.m4a` or `.mka` files
   becomes an album.
2. Review album metadata and choose embedded artwork, a custom image, or a
   looping MP4/MOV animation.
3. Review each track's codec, reorder tracks, remove unwanted tracks from the
   job, and edit their chapter titles.
4. Choose one chaptered album MKV or one numbered MKV per retained track, then
   review the generated filename(s) and destination.
5. Approve albums individually or use **Approve all valid**.
6. Export one album or run the sequential batch export.

Source files are never modified. Each MKV is produced under a temporary name,
verified, and then moved atomically to its destination. Existing destinations
require explicit replacement.

## Development

Requirements:

- Apple Silicon Mac
- Node.js 20+
- Rust
- FFmpeg and FFprobe on PATH for development

Install dependencies with npm install, then launch with npm run tauri dev.

Run checks with npm run build, cargo test --manifest-path
src-tauri/Cargo.toml, and cargo clippy --manifest-path src-tauri/Cargo.toml
--all-targets -- -D warnings.

Build the macOS app and DMG with npm run tauri build.

The packaged application includes arm64 FFmpeg and FFprobe sidecars plus their
license notices. Public distribution still requires an Apple Developer signing
identity and notarization.

## Updates and releases

Installed builds check the project's GitHub Releases feed once per day and can
also check on demand from the refresh button in the app. Downloaded updates are
verified with the public updater key embedded in the application before they
are installed.

To publish a release, update the matching version in `package.json`,
`src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, then push a tag such as
`v0.5.0`. The release workflow builds the Apple Silicon app and DMG, signs the
updater bundle with the `TAURI_SIGNING_PRIVATE_KEY` repository secret, generates
`latest.json`, and publishes the GitHub release only after all assets upload.

The updater signing private key must remain outside source control. Keep a
secure backup: future updates must be signed with the same key already trusted
by installed copies of the app.
