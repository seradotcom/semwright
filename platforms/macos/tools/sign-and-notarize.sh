#!/bin/bash
set -euo pipefail
# Explicit owner invocation only. Never run from an untrusted pull request workflow.
[[ $# -ge 2 && $# -le 3 ]] || { echo 'usage: sign-and-notarize.sh NEW_APP DEVELOPER_ID_IDENTITY [KEYCHAIN_PROFILE]' >&2; exit 2; }
[[ $(uname -s) == Darwin ]] || exit 77
app=$1; identity=$2; profile=${3:-}
[[ "$app" = /* && "$app" = *.app && -d "$app/Contents/MacOS" ]] || exit 2
[[ "$identity" == 'Developer ID Application:'* ]] || { echo 'An explicit Developer ID Application identity is required' >&2; exit 2; }
root=$(cd "$(dirname "$0")/.." && pwd)
for name in semwrightd semwright semwright-mcp semwright-sandbox; do
  codesign --force --options runtime --timestamp --sign "$identity" "$app/Contents/MacOS/$name"
done
codesign --force --options runtime --timestamp --sign "$identity" "$app/Contents/Frameworks/libSemwrightNative.dylib"
codesign --force --options runtime --timestamp --entitlements "$root/Entitlements.plist" --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
[[ -n "$profile" ]] || { echo 'SIGNED, NOT NOTARIZED: no Keychain profile was provided'; exit 0; }
work=$(mktemp -d "${TMPDIR:-/tmp}/semwright-notary.XXXXXX")
trap 'rm -rf "$work"' EXIT
ditto -c -k --keepParent "$app" "$work/upload.zip"
xcrun notarytool submit "$work/upload.zip" --keychain-profile "$profile" --wait --output-format json > "$work/result.json"
id=$(python3 - "$work/result.json" <<'PY2'
import json,sys
v=json.load(open(sys.argv[1]));assert v.get('status')=='Accepted', 'Notarization was not accepted';print(v['id'])
PY2
)
xcrun notarytool log "$id" --keychain-profile "$profile" "$work/notary-log.json"
# Retain a metadata-only notarization log beside the bundle for owner review.
cp "$work/notary-log.json" "${app%.app}.notary-log.json"
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose=2 "$app"
echo 'Ticket stapled; review the notary log and perform installed-app live acceptance before distribution.'
