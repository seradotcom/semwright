#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" == "true" && "${RUNNER_ENVIRONMENT:-}" == "github-hosted" ]]; then
  ci_provider="github-hosted-certification"
  certification_eligible=true
elif [[ "${CIRCLECI:-}" == "true" ]]; then
  ci_provider="circleci-private-iteration"
  certification_eligible=false
else
  echo "ci-driver-bwrap-profile requires a real disposable GitHub-hosted or CircleCI runner" >&2
  exit 2
fi
ci_temp="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/semwright-ci"
mkdir -p "$ci_temp"

evidence="${1:-}"
if [[ -z "$evidence" ]]; then
  echo "usage: ci-driver-bwrap-profile.sh EVIDENCE_FILE" >&2
  exit 2
fi
mkdir -p "$(dirname "$evidence")"

if ! command -v bwrap >/dev/null 2>&1   || ! command -v apparmor_parser >/dev/null 2>&1   || [[ ! -f /usr/share/apparmor/extra-profiles/bwrap-userns-restrict ]]; then
  sudo apt-get update
  sudo apt-get install -y bubblewrap apparmor-profiles apparmor-utils
fi

current_userns=$(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || true)
if [[ -n "$current_userns" && "$current_userns" != "1" ]]; then
  sudo sysctl -w kernel.unprivileged_userns_clone=1
fi

PROFILE=/usr/share/apparmor/extra-profiles/bwrap-userns-restrict
test -f "$PROFILE"
patched="$ci_temp/bwrap-userns-restrict-semwright"
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
    "  allow ix /plugin/tools/**,",
    "  allow ix /workspace/**,",
    "  allow ix /usr/**,",
    "  allow ix /tmp/**,",
])
child_rules = "\n".join([
    "  allow ix /plugin/bin,",
    "  allow ix /plugin/tools/**,",
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

probe_root="$ci_temp/semwright-driver-aa-probe"
rm -rf "$probe_root"
mkdir -p "$probe_root/runtime"
SETPRIV=$(command -v setpriv)
test -x "$SETPRIV"
cp "$SETPRIV" "$probe_root/sandbox"
cp /bin/sh "$probe_root/driver"
cp /usr/bin/true "$probe_root/runtime/tool"
chmod 0755 "$probe_root/sandbox"   "$probe_root/driver"   "$probe_root/runtime/tool"

probe=(bwrap --unshare-all --clearenv --proc /proc --dev /dev --tmpfs /tmp
  --dir /home --dir /workspace --dir /plugin --dir /plugin/tools --dir /etc)
for root in /usr /lib /lib64; do
  [[ ! -e "$root" ]] || probe+=(--ro-bind "$root" "$root")
done
[[ ! -f /etc/ld.so.cache ]] || probe+=(--ro-bind /etc/ld.so.cache /etc/ld.so.cache)
probe+=(--ro-bind "$probe_root/sandbox" /plugin/sandbox)
probe+=(--ro-bind "$probe_root/driver" /plugin/bin)
# Exercise the same sealed-tool destination used by Driver Host. If AppArmor
# cannot execute /plugin/tools/* this preflight must fail before conformance.
probe+=(--ro-bind "$probe_root/runtime/tool" /plugin/tools/tool)
probe+=(--setenv PATH /usr/bin:/bin --setenv LANG C.UTF-8)
"${probe[@]}" -- /plugin/sandbox --no-new-privs /plugin/bin -c '/plugin/tools/tool && /usr/bin/true'

{
  echo "sandbox_setup=scoped-apparmor-driver-chain"
  echo "ci_provider=$ci_provider"
  echo "certification_eligible=$certification_eligible"
  echo "kernel.unprivileged_userns_clone=$(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || true)"
  echo "kernel.apparmor_restrict_unprivileged_userns=$apparmor"
} | tee "$evidence"
