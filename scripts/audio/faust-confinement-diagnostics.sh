#!/usr/bin/env bash
set -uo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <helper> <sandbox> <libraries> <output-dir>" >&2
  exit 2
fi

helper="$1"
sandbox="$2"
libs="$3"
out="$4"
mkdir -p "$out"

record() {
  local name="$1"
  shift
  set +e
  "$@" >"$out/$name.stdout" 2>"$out/$name.stderr"
  local code="$?"
  set -e
  printf '%s\n' "$code" >"$out/$name.exit"
}

clean_env=(
  env -i
  HOME=/tmp
  TMPDIR=/tmp
  XDG_CACHE_HOME=/tmp/cache
  XDG_CONFIG_HOME=/tmp/config
  XDG_DATA_HOME=/tmp/data
  FAUST_LIB_PATH="$libs"
  LANG=C.UTF-8
  LC_ALL=C.UTF-8
  PATH=/usr/bin:/bin
)

record direct-full-env "$helper" probe "$libs"
record direct-clean-env "${clean_env[@]}" "$helper" probe "$libs"

if command -v setpriv >/dev/null 2>&1; then
  record no-new-privs-clean-env "${clean_env[@]}" setpriv --no-new-privs "$helper" probe "$libs"
else
  printf '%s\n' 127 >"$out/no-new-privs-clean-env.exit"
  printf '%s\n' "setpriv unavailable" >"$out/no-new-privs-clean-env.stderr"
fi

# Landlock-only differential: no mount/user/pid/network namespaces.
record landlock-host-fs "$sandbox" \
  --limit-nofile 256 --limit-nproc 16 --limit-cpu 120 \
  --limit-as 2147483648 --limit-fsize 536870912 \
  --read-root "$libs" --exec-root "$helper" \
  -- "$helper" probe "$libs"

bwrap_mounts=(
  --die-with-parent
  --new-session
  --clearenv
  --cap-drop ALL
  --proc /proc
  --dev /dev
  --perms 1777
  --tmpfs /dev/shm
  --tmpfs /tmp
  --dir /home
  --dir /workspace
  --dir /plugin
  --dir /etc
  --ro-bind /usr /usr
  --ro-bind /lib /lib
)
if [ -e /lib64 ]; then
  bwrap_mounts+=(--ro-bind /lib64 /lib64)
fi
if [ -e /etc/ld.so.cache ]; then
  bwrap_mounts+=(--ro-bind /etc/ld.so.cache /etc/ld.so.cache)
fi
bwrap_mounts+=(
  --ro-bind "$helper" /plugin/tools/faust-interpreter
  --ro-bind "$libs" /workspace/libs
  --setenv HOME /tmp
  --setenv TMPDIR /tmp
  --setenv XDG_CACHE_HOME /tmp/cache
  --setenv XDG_CONFIG_HOME /tmp/config
  --setenv XDG_DATA_HOME /tmp/data
  --setenv FAUST_LIB_PATH /workspace/libs
  --setenv LANG C.UTF-8
  --setenv LC_ALL C.UTF-8
  --setenv PATH /usr/bin:/bin
  --chdir /tmp
)

run_bwrap() {
  local name="$1"
  shift
  record "$name" bwrap "$@" "${bwrap_mounts[@]}" -- /plugin/tools/faust-interpreter probe /workspace/libs
}

# Bubblewrap necessarily creates a mount namespace. Everything below isolates
# additional namespace flags relative to that baseline.
run_bwrap bwrap-mount-only
run_bwrap bwrap-unshare-user --unshare-user
run_bwrap bwrap-unshare-ipc --unshare-ipc
run_bwrap bwrap-unshare-pid --unshare-pid
run_bwrap bwrap-unshare-net --unshare-net
run_bwrap bwrap-unshare-uts --unshare-uts
run_bwrap bwrap-unshare-cgroup --unshare-cgroup-try
run_bwrap bwrap-unshare-all --unshare-all
run_bwrap bwrap-unshare-all-share-net --unshare-all --share-net

# Product-like chain: Bubblewrap namespaces + Landlock helper.
record bwrap-landlock bwrap --unshare-all "${bwrap_mounts[@]}" \
  --ro-bind "$sandbox" /plugin/sandbox \
  -- /plugin/sandbox \
  --limit-nofile 256 --limit-nproc 16 --limit-cpu 120 \
  --limit-as 2147483648 --limit-fsize 536870912 \
  --read-root /workspace/libs --exec-root /plugin/tools/faust-interpreter \
  -- /plugin/tools/faust-interpreter probe /workspace/libs

{
  for f in "$out"/*.exit; do
    printf '%s=' "$(basename "$f" .exit)"
    cat "$f"
  done
} | sort | tee "$out/summary.txt"

# Diagnostics are evidence only; the real Host conformance test decides PASS.
exit 0
