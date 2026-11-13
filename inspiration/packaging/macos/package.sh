#!/usr/bin/env bash
# Package the independent Rust/Iced studio. No server, Node, Python or Swift runtime is bundled.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "$script_dir/../.." && pwd)"
arch="$(uname -m)"
case "$arch" in arm64|x86_64) ;; *) printf 'Unsupported host architecture: %s\n' "$arch" >&2; exit 2 ;; esac
binary=""
output="$workspace/dist/macos-$arch"
version="0.1.1"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) binary="${2:?--binary needs a file}"; shift 2 ;;
    --output) output="${2:?--output needs a new directory}"; shift 2 ;;
    *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
  esac
done
if [[ -z "$binary" ]]; then
  cargo build --manifest-path "$workspace/Cargo.toml" --release --locked --offline -p ie-desktop
  target_dir="${CARGO_TARGET_DIR:-$workspace/target}"
  [[ "$target_dir" == /* ]] || target_dir="$workspace/$target_dir"
  binary="$target_dir/release/ie-desktop"
fi
[[ -x "$binary" ]] || { printf 'Missing executable: %s\n' "$binary" >&2; exit 1; }
lipo -verify_arch "$arch" "$binary"
# No recursive deletion/overwriting: a previous package or user directory is never replaced.
[[ ! -e "$output" && ! -L "$output" ]] || { printf 'Output exists; choose a new --output: %s\n' "$output" >&2; exit 1; }
mkdir -p "$(dirname "$output")"
mkdir "$output"
output="$(cd "$output" && pwd)"
app="$output/炼丹炉.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/ie-desktop"
chmod 755 "$app/Contents/MacOS/ie-desktop"
# AppKit is only a build-time vector/text rasterizer for the ICNS asset.
swift "$script_dir/make-icon.swift" "$output/AppIcon.iconset"
iconutil -c icns "$output/AppIcon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
<key>CFBundleDisplayName</key><string>炼丹炉</string>
<key>CFBundleName</key><string>炼丹炉</string>
<key>CFBundleIdentifier</key><string>local.inspiration.studio</string>
<key>CFBundleExecutable</key><string>ie-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>CFBundleIconFile</key><string>AppIcon.icns</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict></plist>
EOF
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$output/炼丹炉-$version-macos-$arch.zip"
printf 'Created %s\nLocal ad-hoc signature only; no Developer ID signing/notarization or Windows verification is implied.\n' "$app"
