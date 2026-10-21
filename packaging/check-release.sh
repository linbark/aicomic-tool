#!/usr/bin/env bash
# Static release guard. Runs on macOS using Cargo and system command-line tools.
set -euo pipefail
repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
offline=()
self_test=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --offline) offline=(--offline); shift ;;
    --self-test) self_test=1; shift ;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done

check_sources() {
  local root="$1"
  local file
  local legacy_paths='react-frontend|src-tauri|requirements\.txt|(^|[^[:alnum:]_])backend[/\\]'
  local legacy_commands='(^|[[:space:];])((python[0-9.]*|node|npm|npx|yarn|pnpm|uvicorn)([[:space:]]|$))'
  local inputs=(
    "$root/packaging/macos/package.sh"
    "$root/packaging/windows/package.ps1"
    "$root/packaging/windows/installer.nsi"
    "$root/.github/workflows/rust-desktop.yml"
    "$root/crates/aicomic-desktop/build.rs"
    "$root/Cargo.toml"
  )
  for file in "$root"/crates/*/Cargo.toml; do inputs+=("$file"); done
  for file in "${inputs[@]}"; do
    [[ -s "$file" ]] || { printf 'Missing release input: %s\n' "$file" >&2; return 1; }
    if grep -Eni "$legacy_paths|$legacy_commands" "$file"; then
      printf 'Legacy runtime or source path in release input: %s\n' "$file" >&2
      return 1
    fi
  done
  local legacy_process='Command[[:space:]]*::[[:space:]]*new[[:space:]]*\([[:space:]]*"(python[0-9.]*|node|npm|npx|yarn|pnpm|uvicorn)"'
  while IFS= read -r file; do
    if grep -Eni "$legacy_paths|$legacy_process" "$file"; then
      printf 'Legacy source include or process launch in Rust code: %s\n' "$file" >&2
      return 1
    fi
  done < <(find "$root/crates" -type f -name '*.rs' -path '*/src/*' ! -path '*/target/*')
  if grep -En '^name = "(tauri([^"]*)?|pyo3|cpython|neon|nodejs-sys|wry|webview2-com)"' "$root/Cargo.lock"; then
    printf 'A legacy application runtime is present in Cargo.lock\n' >&2
    return 1
  fi
  grep -Fq 'target-feature=+crt-static' "$root/packaging/windows/package.ps1" || {
    printf 'Windows release packaging must request a static CRT\n' >&2; return 1;
  }
  grep -Fq 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS' "$root/.github/workflows/rust-desktop.yml" || {
    printf 'Windows CI must use the same static CRT release flags\n' >&2; return 1;
  }
  bash -n "$root/packaging/macos/package.sh"
}

check_sources "$repo_dir"
[[ "$(od -An -tx1 -N4 "$repo_dir/packaging/icons/icon.ico" | tr -d ' \n')" == 00000100 ]] || { printf 'Invalid Windows icon\n' >&2; exit 1; }
[[ "$(dd if="$repo_dir/packaging/icons/icon.icns" bs=4 count=1 2>/dev/null)" == icns ]] || { printf 'Invalid macOS icon\n' >&2; exit 1; }
[[ "$(od -An -tx1 -N8 "$repo_dir/packaging/icons/icon.png" | tr -d ' \n')" == 89504e470d0a1a0a ]] || { printf 'Invalid PNG icon\n' >&2; exit 1; }
if command -v xmllint >/dev/null 2>&1; then
  xmllint --nonet --noout "$repo_dir/packaging/windows/app.manifest"
fi

staging="$(mktemp -d "${TMPDIR:-/tmp}/xingqing-release-check.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
# Reconstruct only the Rust delivery inputs. No old application trees are copied.
tar -C "$repo_dir" --exclude='*/target' --exclude='*/Cargo.lock' -cf - \
  Cargo.toml Cargo.lock crates packaging .github/workflows/rust-desktop.yml | tar -C "$staging" -xf -
[[ ! -e "$staging/backend" && ! -e "$staging/react-frontend" && ! -e "$staging/requirements.txt" ]]
cargo metadata --locked "${offline[@]}" --no-deps --format-version 1 \
  --manifest-path "$staging/Cargo.toml" > "$staging/workspace-metadata.json"
printf 'Release inputs: no legacy source or runtime references; Rust-only staged workspace resolves.\n'

if [[ "$self_test" == 1 ]]; then
  # Only inject text into the throwaway copy. Never execute the injected command.
  printf '\nnpm run build\n' >> "$staging/packaging/macos/package.sh"
  if check_sources "$staging" > "$staging/rejection.log" 2>&1; then
    printf 'Release guard failed to reject a legacy build command\n' >&2; exit 1
  fi
  grep -Fq 'Legacy runtime or source path' "$staging/rejection.log"
  printf 'Guard self-test: an injected legacy build command was rejected.\n'
fi
printf 'Static release check passed. Windows execution and installer validation still require the native CI job.\n'
