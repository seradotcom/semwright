#!/bin/bash
set -euo pipefail
[[ $# == 1 && $(uname -s) == Darwin ]] || { echo 'usage: build-fixture.sh NEW_OUTPUT.app (on macOS)' >&2; exit 2; }
[[ "$1" == *.app && ! -e "$1" ]] || exit 2
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$1/Contents/MacOS"
cp "$root/fixtures/Info.plist" "$1/Contents/Info.plist"
xcrun swiftc -swift-version 5 -parse-as-library -target "$(uname -m)-apple-macosx14.0" -framework AppKit "$root/fixtures/DesktopFixture.swift" -o "$1/Contents/MacOS/SemwrightFixture"
echo 'Fixture built only. Open it manually in a disposable GUI session for live AX/input tests.'
