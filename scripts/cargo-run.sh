#!/bin/bash
set -euo pipefail

binary="$1"
shift

# Cargo also uses its runner for tests and examples. Run those normally.
if [[ "$(basename "$binary")" != audio-router ]]; then
  exec "$binary" "$@"
fi

project_root="$(cd "$(dirname "$0")/.." && pwd)"
binary_dir="$(cd "$(dirname "$binary")" && pwd)"
app="$binary_dir/Audio Router.app"
bundled_binary="$app/Contents/MacOS/audio-router"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# Keep the signed bundle intact when the executable and metadata are unchanged.
if ! cmp -s "$binary" "$bundled_binary" || \
   ! cmp -s "$project_root/resources/Info.plist" "$app/Contents/Info.plist" || \
   ! cmp -s "$project_root/resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"; then
  cp "$binary" "$bundled_binary"
  cp "$project_root/resources/Info.plist" "$app/Contents/Info.plist"
  cp "$project_root/resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
  codesign --force --sign "${SIGNING_IDENTITY:--}" --identifier local.audio-router "$app"
fi

# Running the executable inside its bundle preserves its macOS app identity,
# while keeping Cargo's normal terminal output and argument forwarding.
exec "$bundled_binary" "$@"
