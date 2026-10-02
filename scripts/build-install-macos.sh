#!/usr/bin/env bash
set -euo pipefail

readonly signing_identity="Apple Development: i@jlzov.com (SFRBKW6P78)"
readonly signing_cert_sha1="158D75C090FD771527624707372E63CB8952B69A"
readonly development_team="NKW2BHRJH2"
readonly developer_dir="/Applications/Xcode.app/Contents/Developer"
readonly built_app="src-tauri/target/release/bundle/macos/Handy.app"
readonly installed_app="/Applications/Handy.app"
readonly built_executable="$built_app/Contents/MacOS/handy"
readonly installed_executable="$installed_app/Contents/MacOS/handy"
readonly bundled_onnxruntime="$built_app/Contents/Frameworks/libonnxruntime.1.dylib"

# The Homebrew onnxruntime links protobuf, abseil, onnx and re2 through
# /opt/homebrew/opt/<formula>, which moves to the new version on every
# `brew upgrade` and left the installed app unable to launch. Resolve a
# dependency whose opt link has moved to the Cellar copy it was built against.
resolve_homebrew_dylib() {
  local dep="$1" formula name candidate
  if [[ -e "$dep" ]]; then
    printf '%s\n' "$dep"
    return 0
  fi
  formula="${dep#/opt/homebrew/opt/}"
  formula="${formula%%/*}"
  name="$(basename "$dep")"
  for candidate in /opt/homebrew/Cellar/"$formula"/*/lib/"$name"; do
    if [[ -e "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

# Copy every Homebrew dylib the bundled onnxruntime needs, transitively, into
# the Frameworks directory and point each reference at its sibling copy.
bundle_homebrew_dependencies() {
  local frameworks_dir="$1" changed=1 dylib deps dep name source
  while ((changed)); do
    changed=0
    for dylib in "$frameworks_dir"/*.dylib; do
      deps="$(otool -L "$dylib" | awk 'NR>1 {print $1}')"
      while IFS= read -r dep; do
        [[ "$dep" == /opt/homebrew/* ]] || continue
        name="$(basename "$dep")"
        if [[ ! -f "$frameworks_dir/$name" ]]; then
          if ! source="$(resolve_homebrew_dylib "$dep")"; then
            echo "Cannot find a copy of $dep to bundle." >&2
            return 1
          fi
          cp -L "$source" "$frameworks_dir/$name"
          chmod u+w "$frameworks_dir/$name"
          install_name_tool -id "@loader_path/$name" "$frameworks_dir/$name"
          changed=1
        fi
        install_name_tool -change "$dep" "@loader_path/$name" "$dylib"
      done <<<"$deps"
    done
  done
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer is only for macOS." >&2
  exit 1
fi

available_identities="$(security find-identity -v -p codesigning)"
if ! grep -Fq "$signing_cert_sha1" <<<"$available_identities"; then
  echo "Required signing certificate is unavailable: $signing_identity" >&2
  exit 1
fi

compiler_runtime="$(DEVELOPER_DIR="$developer_dir" xcrun clang \
  --print-file-name=libclang_rt.osx.a)"
if [[ ! -f "$compiler_runtime" ]]; then
  echo "Xcode compiler runtime not found: $compiler_runtime" >&2
  exit 1
fi

ORT_LIB_LOCATION="$(brew --prefix onnxruntime)/lib" \
  ORT_PREFER_DYNAMIC_LINK=1 \
  MACOSX_DEPLOYMENT_TARGET=26.0 \
  DEVELOPER_DIR="$developer_dir" \
  RUSTFLAGS="-C link-arg=$compiler_runtime" \
  bun run tauri build --config src-tauri/tauri.local.conf.json --bundles app

if [[ ! -d "$built_app" ]]; then
  echo "Built application not found: $built_app" >&2
  exit 1
fi

onnxruntime_link_path="$(brew --prefix onnxruntime)/lib/libonnxruntime.1.dylib"
mkdir -p "$(dirname "$bundled_onnxruntime")"
ditto "$onnxruntime_link_path" "$bundled_onnxruntime"
install_name_tool -id "@rpath/libonnxruntime.1.dylib" "$bundled_onnxruntime"
bundle_homebrew_dependencies "$(dirname "$bundled_onnxruntime")"

linked_libraries="$(otool -L "$built_executable")"
if grep -Fq "$onnxruntime_link_path" <<<"$linked_libraries"; then
  install_name_tool -change "$onnxruntime_link_path" \
    "@rpath/libonnxruntime.1.dylib" "$built_executable"
fi

load_commands="$(otool -l "$built_executable")"
if ! grep -Fq "@executable_path/../Frameworks" <<<"$load_commands"; then
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$built_executable"
fi

for bundled_dylib in "$(dirname "$bundled_onnxruntime")"/*.dylib; do
  codesign --force --options runtime --timestamp=none \
    --sign "$signing_identity" "$bundled_dylib"
done
codesign --force --options runtime --timestamp=none \
  --entitlements src-tauri/Entitlements.local.plist \
  --sign "$signing_identity" "$built_app"

codesign --verify --deep --strict "$built_app"
if otool -L "$built_executable" "$(dirname "$bundled_onnxruntime")"/*.dylib |
  grep -F "/opt/homebrew"; then
  echo "The app still links Homebrew libraries and would break on upgrade." >&2
  exit 1
fi
signature_info="$(codesign -dvv "$built_app" 2>&1)"
grep -Fq "Authority=$signing_identity" <<<"$signature_info"
grep -Fq "TeamIdentifier=$development_team" <<<"$signature_info"

# The process name is the lowercase executable (`handy`), not the bundle name
# (`Handy`). Stop only the installed app and wait for it to exit; otherwise
# LaunchServices keeps the old executable alive and `open` never starts the
# freshly copied build.
pkill -TERM -f -x "$installed_executable" >/dev/null 2>&1 || true
for _ in {1..50}; do
  if ! pgrep -f -x "$installed_executable" >/dev/null; then
    break
  fi
  sleep 0.1
done
if pgrep -f -x "$installed_executable" >/dev/null; then
  echo "Installed Handy process did not exit: $installed_executable" >&2
  exit 1
fi

rm -rf "$installed_app"
ditto "$built_app" "$installed_app"
codesign --verify --deep --strict "$installed_app"
open "$installed_app"

echo "Build complete, signature verified, and app launched: $installed_app"
