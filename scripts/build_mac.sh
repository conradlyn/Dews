#!/usr/bin/env bash
# Build the macOS client (.app + .dmg) for Dews.
#
# Tauri's bundled dmg script (bundle_dmg.sh) relies on `grep -E` with a device
# path as the pattern. On environments where `grep` is a toybox shim (e.g.
# sandboxed agent shells) this fails with "bad regex: brackets not balanced".
# This script builds the .app via Tauri, then creates the .dmg with hdiutil
# directly, which is stable across macOS versions.
set -euo pipefail

# Ensure cargo is discoverable even when the caller's PATH lacks rustup's bin.
if ! command -v cargo >/dev/null 2>&1 && [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="$ROOT/src-tauri/target/release/bundle"
APP="$BUNDLE/macos/dews.app"
DMG="$BUNDLE/dmg/dews_1.0.1_aarch64.dmg"

cd "$ROOT"
npx tauri build --bundles app

if [[ ! -d "$APP" ]]; then
  echo "error: .app bundle not found at $APP" >&2
  exit 1
fi

mkdir -p "$(dirname "$DMG")"
hdiutil create -volname "dews" -srcfolder "$APP" -ov -format UDZO "$DMG"

echo "Built:"
echo "  $APP"
echo "  $DMG"
