#!/bin/bash
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { echo 'SKIP: native smoke requires macOS and Xcode' >&2; exit 77; }
repo=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/semwright-native-smoke.XXXXXX")
trap 'rm -rf "$work"' EXIT
arch=$(uname -m)
native="$repo/crates/platform-macos/native"
xcrun clang -arch "$arch" -mmacosx-version-min=14.0 -c "$native/SecureInput.c" -o "$work/secure.o"
xcrun swiftc -swift-version 5 -warnings-as-errors -parse-as-library -target "$arch-apple-macosx14.0" -emit-library -module-name SemwrightNative \
  -I "$native/include" "$native"/*.swift "$work/secure.o" -framework AppKit -framework ApplicationServices -framework CoreGraphics \
  -framework ScreenCaptureKit -framework ImageIO -framework UniformTypeIdentifiers -framework Security \
  -framework ServiceManagement -framework Carbon -Xlinker -install_name -Xlinker @rpath/libSemwrightNative.dylib \
  -o "$work/libSemwrightNative.dylib"
xcrun swiftc -swift-version 5 -warnings-as-errors -parse-as-library "$repo/platforms/macos/tests/NativeSmoke.swift" \
  -I "$native/include" -L "$work" -lSemwrightNative -Xlinker -rpath -Xlinker "$work" -o "$work/native-smoke"
"$work/native-smoke"
