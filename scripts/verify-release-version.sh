#!/usr/bin/env bash
set -euo pipefail

tag="${1:-}"
version="$(node -p "require('./package.json').version")"
tauri_version="$(node -p "require('./src-tauri/tauri.conf.json').version")"
cargo_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' src-tauri/Cargo.toml | head -1)"

if [[ -n "${tag}" && "${tag}" != "v${version}" ]]; then
  echo "Tag ${tag} does not match package version v${version}." >&2
  exit 1
fi

if [[ "${version}" != "${tauri_version}" || "${version}" != "${cargo_version}" ]]; then
  echo "Version mismatch: package=${version}, tauri=${tauri_version}, cargo=${cargo_version}." >&2
  exit 1
fi

echo "Release versions agree at ${version}."
