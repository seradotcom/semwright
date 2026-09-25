#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  echo "ci-bwrap-preflight is only for disposable GitHub Actions runners" >&2
  exit 2
fi

if ! command -v bwrap >/dev/null; then
  sudo apt-get update
  sudo apt-get install -y bubblewrap
fi

current_userns=$(sysctl -n kernel.unprivileged_userns_clone 2>/dev/null || true)
if [[ -n "$current_userns" && "$current_userns" != "1" ]]; then
  sudo sysctl -w kernel.unprivileged_userns_clone=1
fi

current_apparmor=$(sysctl -n kernel.apparmor_restrict_unprivileged_userns 2>/dev/null || true)
if [[ "$current_apparmor" == "1" ]]; then
  sudo apt-get update
  sudo apt-get install -y apparmor-profiles apparmor-utils
  profile=/usr/share/apparmor/extra-profiles/bwrap-userns-restrict
  if [[ -f "$profile" ]] &&
    sudo install -m 0644 "$profile" /etc/apparmor.d/bwrap-userns-restrict &&
    sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict; then
    :
  else
    # The runner is disposable; production Semwright never changes this sysctl.
    sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
  fi
fi

bwrap_args=(
  --die-with-parent --new-session --unshare-all --clearenv --cap-drop ALL
  --proc /proc --dev /dev --tmpfs /tmp --dir /home --dir /workspace --dir /plugin
)
for runtime in /usr /lib /lib64; do
  [[ ! -e "$runtime" ]] || bwrap_args+=(--ro-bind "$runtime" "$runtime")
done
bwrap_args+=(--dir /etc)
[[ ! -e /etc/ld.so.cache ]] ||
  bwrap_args+=(--ro-bind /etc/ld.so.cache /etc/ld.so.cache)
bwrap "${bwrap_args[@]}" -- /usr/bin/true
