#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  echo "ci-driver-bwrap-profile is only for disposable GitHub Actions runners" >&2
  exit 2
fi

evidence="${1:-}"
if [[ -z "$evidence" ]]; then
  echo "usage: ci-driver-bwrap-profile.sh EVIDENCE_FILE" >&2
  exit 2
fi
mkdir -p "$(dirname "$evidence")"

sudo apt-get update
sudo apt-get install -y bubblewrap apparmor-profiles apparmor-utils

current_userns=$(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || true)
if [[ -n "$current_userns" && "$current_userns" != "1" ]]; then
  sudo sysctl -w kernel.unprivileged_userns_clone=1
fi

PROFILE=/usr/share/apparmor/extra-profiles/bwrap-userns-restrict
test -f "$PROFILE"
patched="$RUNNER_TEMP/bwrap-userns-restrict-semwright"
cp "$PROFILE" "$patched"

python3 - "$patched" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
old = "flags=(attach_disconnected)"
count = text.count(old)
if count != 2:
    raise SystemExit(f"unexpected bwrap AppArmor profile shape: {count} flag matches")
text = text.replace(old, "flags=(attach_disconnected,mediate_deleted)")

parent = "allow px /** -> bwrap//&unpriv_bwrap,"
nested = "allow pix /** -> &unpriv_bwrap,"
parent_rules = "\n".join([
    "  allow px /plugin/sandbox -> bwrap//&unpriv_bwrap,",
    "  allow ix /plugin/bin,",
    "  allow ix /workspace/**,",
    "  allow ix /usr/**,",
    "  allow ix /tmp/**,",
])
child_rules = "\n".join([
    "  allow ix /plugin/bin,",
    "  allow ix /workspace/**,",
    "  allow ix /usr/**,",
    "  allow ix /tmp/**,",
])
if text.count(parent) != 1 or text.count(nested) != 1:
    raise SystemExit("unexpected bwrap AppArmor exec-rule shape")
text = text.replace("  " + parent, parent_rules)
text = text.replace("  " + nested, child_rules)
path.write_text(text)
PY

sudo install -m 0644 "$patched" /etc/apparmor.d/bwrap-userns-restrict
sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict

# Do not weaken the host policy. The scoped profile must make the chain work
# while Ubuntu's unprivileged-userns restriction remains enabled.
apparmor=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null || echo 1)
test "$apparmor" != "0"

rm -rf "$RUNNER_TEMP/semwright-driver-aa-probe"
mkdir -p "$RUNNER_TEMP/semwright-driver-aa-probe/runtime"
SETPRIV=$(command -v setpriv)
test -x "$SETPRIV"
cp "$SETPRIV" "$RUNNER_TEMP/semwright-driver-aa-probe/sandbox"
cp /bin/sh "$RUNNER_TEMP/semwright-driver-aa-probe/driver"
cp /usr/bin/true "$RUNNER_TEMP/semwright-driver-aa-probe/runtime/tool"
chmod 0755 "$RUNNER_TEMP/semwright-driver-aa-probe/sandbox" \
  "$RUNNER_TEMP/semwright-driver-aa-probe/driver" \
  "$RUNNER_TEMP/semwright-driver-aa-probe/runtime/tool"

probe=(bwrap --unshare-all --clearenv --proc /proc --dev /dev --tmpfs /tmp
  --dir /home --dir /workspace --dir /plugin --dir /etc)
for root in /usr /lib /lib64; do
  [[ ! -e "$root" ]] || probe+=(--ro-bind "$root" "$root")
done
[[ ! -f /etc/ld.so.cache ]] || probe+=(--ro-bind /etc/ld.so.cache /etc/ld.so.cache)
probe+=(--ro-bind "$RUNNER_TEMP/semwright-driver-aa-probe/sandbox" /plugin/sandbox)
probe+=(--ro-bind "$RUNNER_TEMP/semwright-driver-aa-probe/driver" /plugin/bin)
probe+=(--ro-bind "$RUNNER_TEMP/semwright-driver-aa-probe/runtime" /workspace/runtime)
probe+=(--setenv PATH /usr/bin:/bin --setenv LANG C.UTF-8)
"${probe[@]}" -- /plugin/sandbox --no-new-privs /plugin/bin -c '/workspace/runtime/tool && /usr/bin/true'

{
  echo "sandbox_setup=scoped-apparmor-driver-chain"
  echo "kernel.unprivileged_userns_clone=$(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || true)"
  echo "kernel.apparmor_restrict_unprivileged_userns=$apparmor"
} | tee "$evidence"
