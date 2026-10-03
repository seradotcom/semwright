#!/usr/bin/env bash
# Receipt-owned installation only. Does not change PATH, policy, services or OS consent.
set -euo pipefail
umask 077
fail() { printf '%s\n' "Semwright: $*" >&2; exit 2; }
[[ $(id -u) != 0 ]] || fail 'Run as the installing user, not root.'
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
HOME_ROOT=$(cd "$HOME" && pwd -P)
case $(uname -s) in
  Linux) DEFAULT_PREFIX="$HOME_ROOT/.local/share/semwright" ;;
  Darwin) DEFAULT_PREFIX="$HOME_ROOT/Library/Application Support/Semwright" ;;
  *) fail 'This helper supports Linux and macOS only.' ;;
esac
MODE=${1:-}; shift || true
PREFIX=$DEFAULT_PREFIX
if [[ "$MODE" == uninstall && -f "$HERE/.semwright-install-receipt" ]]; then PREFIX=$HERE; fi
if [[ $# -gt 0 ]]; then
  [[ $# -eq 2 && "$1" == --prefix ]] || fail 'Usage: install.sh or uninstall.sh [--prefix /absolute/user/path]'
  PREFIX=${2%/}
fi
[[ "$MODE" == install || "$MODE" == uninstall ]] || fail 'Unknown operation.'
[[ "$PREFIX" == "$HOME_ROOT/"* && "$PREFIX" != "$HOME_ROOT/" ]] || fail 'Prefix must be a child of your HOME.'
case "$PREFIX/" in *'/../'*|*'/./'*|*'//'*) fail 'Noncanonical prefix.' ;; esac

reject_links() {
  local p=$1
  while [[ "$p" != / && "$p" != . ]]; do
    [[ ! -L "$p" ]] || fail "Symlink refused: $p"
    p=$(dirname "$p")
  done
}
user_directory_chain() {
  local p=$PREFIX owner mode
  while :; do
    [[ ! -L "$p" ]] || fail "Symlink refused: $p"
    if [[ -e "$p" ]]; then
      [[ -d "$p" ]] || fail "Not a directory: $p"
      if [[ $(uname -s) == Darwin ]]; then
        owner=$(stat -f '%u' "$p"); mode=$(stat -f '%Lp' "$p")
      else
        owner=$(stat -c '%u' "$p"); mode=$(stat -c '%a' "$p")
      fi
      [[ "$owner" == "$(id -u)" ]] || fail "Not user owned: $p"
      (( (8#$mode & 0022) == 0 )) || fail "Writable by another user: $p"
    fi
    [[ "$p" != "$HOME_ROOT" ]] || break
    p=$(dirname "$p")
  done
}
hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f1
  else
    fail 'SHA-256 utility required (sha256sum or shasum).'
  fi
}
# Restrict receipt paths to portable relative filenames, never absolute paths or traversal.
load_receipt() {
  local manifest=$1 line digest relative seen='|'
  reject_links "$manifest"
  [[ -f "$manifest" ]] || fail 'Missing regular checksum/receipt file.'
  [[ $(wc -c < "$manifest") -le 131072 ]] || fail 'Oversized checksum/receipt file.'
  FILES=(); HASHES=()
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ "$line" == *'  '* ]] || fail 'Malformed checksum/receipt entry.'
    digest=${line%%  *}; relative=${line#*  }
    [[ "$digest" =~ ^[0-9a-f]{64}$ && "$relative" =~ ^[A-Za-z0-9_.\/-]+$ ]] || fail 'Invalid checksum/receipt entry.'
    case "/$relative/" in *'/../'*|*'/./'*|*'//'*) fail 'Unsafe receipt path.' ;; esac
    [[ "$relative" != /* && "$relative" != .semwright-install-receipt ]] || fail 'Unsafe receipt path.'
    case "$seen" in *"|$relative|"*) fail 'Duplicate receipt path.' ;; esac
    seen="$seen$relative|"
    FILES+=("$relative"); HASHES+=("$digest")
    [[ ${#FILES[@]} -le 1024 ]] || fail 'Too many receipt entries.'
  done < "$manifest"
  [[ ${#FILES[@]} -gt 0 ]] || fail 'Empty checksum/receipt file.'
}
verify_files() {
  local base=$1 i path
  for ((i=0; i<${#FILES[@]}; i++)); do
    path="$base/${FILES[$i]}"
    reject_links "$path"
    [[ -f "$path" && ! -L "$path" ]] || fail "Missing or nonregular file: $path"
    [[ $(hash_file "$path") == "${HASHES[$i]}" ]] || fail "File changed; refusing operation: $path"
  done
}
user_directory_chain
if [[ "$MODE" == install ]]; then
  [[ ! -e "$PREFIX" && ! -L "$PREFIX" ]] || fail 'Refusing to replace an existing install directory; review/uninstall first.'
  load_receipt "$HERE/SHA256SUMS"
  verify_files "$HERE"
  for binary in semwright semwrightd semwright-mcp semwright-inspect semwright-sandbox; do
    [[ -f "$HERE/bin/$binary" ]] || fail "Incomplete bundle: $binary"
    found=false
    for relative in "${FILES[@]}"; do [[ "$relative" != "bin/$binary" ]] || found=true; done
    [[ "$found" == true ]] || fail "Unchecked binary: $binary"
  done
  mkdir -p "$(dirname "$PREFIX")"
  user_directory_chain
  mkdir -m 700 "$PREFIX"
  # Failure leaves a private partial install for inspection; never recursively erase user files.
  for relative in "${FILES[@]}"; do
    mkdir -p "$(dirname "$PREFIX/$relative")"
    cp "$HERE/$relative" "$PREFIX/$relative"
    case "$relative" in bin/*|Frameworks/*|*.sh) chmod 700 "$PREFIX/$relative" ;; *) chmod 600 "$PREFIX/$relative" ;; esac
  done
  cp "$HERE/SHA256SUMS" "$PREFIX/SHA256SUMS"
  { cat "$HERE/SHA256SUMS"; printf '%s  SHA256SUMS\n' "$(hash_file "$HERE/SHA256SUMS")"; } > "$PREFIX/.semwright-install-receipt"
  chmod 600 "$PREFIX/SHA256SUMS" "$PREFIX/.semwright-install-receipt"
  load_receipt "$PREFIX/.semwright-install-receipt"
  verify_files "$PREFIX"
  printf 'Installed in %s\nNo PATH, service, configuration or OS permission was changed.\n' "$PREFIX"
  printf 'Next: "%s/bin/semwright" setup\n' "$PREFIX"
  printf 'Then start the broker and run: "%s/bin/semwright" --json doctor\n' "$PREFIX"
  printf 'Remove: "%s/uninstall.sh"\n' "$PREFIX"
else
  load_receipt "$PREFIX/.semwright-install-receipt"
  verify_files "$PREFIX"
  # Validate ALL files before deleting ANY; unknown files/configuration are never enumerated for deletion.
  for ((i=0; i<${#FILES[@]}; i++)); do
    path="$PREFIX/${FILES[$i]}"
    reject_links "$path"
    [[ $(hash_file "$path") == "${HASHES[$i]}" ]] || fail "File changed during removal: $path"
    rm "$path"
  done
  rm "$PREFIX/.semwright-install-receipt"
  for relative in "${FILES[@]}"; do
    parent=$(dirname "$PREFIX/$relative")
    while [[ "$parent" != "$PREFIX" ]]; do
      rmdir "$parent" 2>/dev/null || true
      parent=$(dirname "$parent")
    done
  done
  rmdir "$PREFIX" 2>/dev/null || printf 'Retained unowned files in %s\n' "$PREFIX"
  printf 'Removed only receipt-owned unchanged files. External user data and settings were retained.\n'
fi
