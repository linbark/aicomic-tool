#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
target="$(rustc -vV | sed -n 's/^host: //p')"
profile=release
skip_build=0
binary=""
output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) target="$2"; shift 2 ;;
    --profile) profile="$2"; shift 2 ;;
    --binary) binary="$2"; skip_build=1; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --skip-build) skip_build=1; shift ;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done
case "$target" in
  aarch64-apple-darwin) arch=arm64 ;;
  x86_64-apple-darwin) arch=x64 ;;
  *) printf 'Unsupported macOS target: %s\n' "$target" >&2; exit 2 ;;
esac
case "$profile" in release|debug) ;; *) printf 'Profile must be release or debug\n' >&2; exit 2 ;; esac
version="$(awk '/^\[workspace.package\]/{found=1;next} found && /^version = /{gsub(/"/, "", $3);print $3;exit}' "$repo_dir/Cargo.toml")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'Invalid release version\n' >&2; exit 2; }
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
cd "$repo_dir"
if [[ "$skip_build" == 0 ]]; then
  if [[ "$profile" == release ]]; then
    cargo build --locked -p aicomic-desktop --release --target "$target"
  else
    cargo build --locked -p aicomic-desktop --target "$target"
  fi
fi
binary="${binary:-$repo_dir/target/$target/$profile/aicomic-tool}"
[[ -x "$binary" ]] || { printf 'Missing executable: %s\n' "$binary" >&2; exit 1; }
expected_arch="$arch"
[[ "$arch" == x64 ]] && expected_arch=x86_64
lipo -verify_arch "$expected_arch" "$binary"
output="${output:-$repo_dir/dist/macos-$arch}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
staging="$(mktemp -d "${TMPDIR:-/tmp}/xingqing-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
app="$staging/星晴.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/aicomic-tool"
chmod 755 "$app/Contents/MacOS/aicomic-tool"
cp "$repo_dir/packaging/icons/icon.icns" "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
  <key>CFBundleDisplayName</key><string>星晴</string>
  <key>CFBundleName</key><string>星晴</string>
  <key>CFBundleIdentifier</key><string>com.ljc.aicomictool</string>
  <key>CFBundleExecutable</key><string>aicomic-tool</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict></plist>
EOF
plutil -lint "$app/Contents/Info.plist"
# Local ad-hoc signature only. No Developer ID certificate or notarization is implied.
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
ditto "$app" "$output/星晴.app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$output/Xingqing-$version-macos-$arch.app.zip"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "星晴" -srcfolder "$staging" -format UDZO -fs HFS+ -ov "$output/Xingqing-$version-macos-$arch.dmg"
hdiutil verify "$output/Xingqing-$version-macos-$arch.dmg"
printf 'Created %s\nSigning: local ad-hoc only; no Developer ID signing or notarization.\n' "$output"
