#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Audio Router requires macOS 14.4 or later.' >&2
  exit 1
fi
export MACOSX_DEPLOYMENT_TARGET=14.4
cargo build --release --locked
APP="$PWD/dist/Audio Router.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/audio-router "$APP/Contents/MacOS/audio-router"
cp resources/Info.plist "$APP/Contents/Info.plist"
cp resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# Local signing requires no Apple developer account. For distribution, supply
# SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)".
codesign --force --sign "${SIGNING_IDENTITY:--}" --identifier local.audio-router "$APP"
codesign --verify --strict "$APP"
echo "Built: $APP"
if [[ "${1:-}" == --open ]]; then open "$APP"; fi
