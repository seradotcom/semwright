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

record landlock-host-fs "$sandbox" \
  --limit-nofile 256 --limit-nproc 16 --limit-cpu 120 \
  --limit-as 2147483648 --limit-fsize 536870912 \
  --read-root "$libs" --exec-root "$helper" \
  -- "$helper" probe "$libs"

base_mounts=(
  --die-with-parent
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
  base_mounts+=(--ro-bind /lib64 /lib64)
fi
if [ -e /etc/ld.so.cache ]; then
  base_mounts+=(--ro-bind /etc/ld.so.cache /etc/ld.so.cache)
fi
base_mounts+=(
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

run_minimal() {
  local name="$1"
  shift
  record "$name" bwrap "$@" "${base_mounts[@]}" -- /plugin/tools/faust-interpreter probe /workspace/libs
}

run_minimal bwrap-mount-only-no-session
run_minimal bwrap-new-session-only --new-session
run_minimal bwrap-unshare-user --unshare-user
run_minimal bwrap-unshare-ipc --unshare-ipc
run_minimal bwrap-unshare-pid --unshare-pid
run_minimal bwrap-unshare-net --unshare-net
run_minimal bwrap-unshare-uts --unshare-uts
run_minimal bwrap-unshare-cgroup --unshare-cgroup-try
run_minimal bwrap-unshare-all --unshare-all
run_minimal bwrap-unshare-all-share-net --unshare-all --share-net

# libfaust resolves argv[0] with popen("which faust"); POSIX popen invokes /bin/sh.
# The product profile exposes /usr/bin but not /bin, so test that dependency alone.
if [ -L /bin ] && [ "$(readlink /bin)" = "usr/bin" ]; then
  run_minimal bwrap-with-bin --symlink usr/bin /bin
elif [ -d /bin ]; then
  run_minimal bwrap-with-bin --ro-bind /bin /bin
fi

if [ -e /sys ]; then
  run_minimal bwrap-with-sys --ro-bind /sys /sys
fi
if [ -e /run ]; then
  run_minimal bwrap-with-run --ro-bind /run /run
fi
if [ -e /sys ] && [ -e /run ]; then
  run_minimal bwrap-with-sys-run --ro-bind /sys /sys --ro-bind /run /run
fi

record bwrap-root-ro-no-session bwrap \
  --die-with-parent --clearenv --cap-drop ALL \
  --ro-bind / / \
  --tmpfs /tmp --tmpfs /dev/shm \
  --setenv HOME /tmp --setenv TMPDIR /tmp \
  --setenv XDG_CACHE_HOME /tmp/cache --setenv XDG_CONFIG_HOME /tmp/config \
  --setenv XDG_DATA_HOME /tmp/data --setenv FAUST_LIB_PATH "$libs" \
  --setenv LANG C.UTF-8 --setenv LC_ALL C.UTF-8 --setenv PATH /usr/bin:/bin \
  --chdir /tmp -- "$helper" probe "$libs"

record bwrap-root-ro-new-session bwrap \
  --die-with-parent --new-session --clearenv --cap-drop ALL \
  --ro-bind / / \
  --tmpfs /tmp --tmpfs /dev/shm \
  --setenv HOME /tmp --setenv TMPDIR /tmp \
  --setenv XDG_CACHE_HOME /tmp/cache --setenv XDG_CONFIG_HOME /tmp/config \
  --setenv XDG_DATA_HOME /tmp/data --setenv FAUST_LIB_PATH "$libs" \
  --setenv LANG C.UTF-8 --setenv LC_ALL C.UTF-8 --setenv PATH /usr/bin:/bin \
  --chdir /tmp -- "$helper" probe "$libs"

if command -v strace >/dev/null 2>&1; then
  trace_dir="$out/strace"
  mkdir -p "$trace_dir"
  trace_mounts=("${base_mounts[@]}")
  trace_mounts+=(--bind "$trace_dir" /trace)
  record strace-bwrap bwrap "${trace_mounts[@]}" -- \
    /usr/bin/strace -ff -qq -s 160 -o /trace/faust.strace \
    /plugin/tools/faust-interpreter probe /workspace/libs
  for trace in "$trace_dir"/faust.strace*; do
    [ -f "$trace" ] || continue
    tail -n 160 "$trace" >"$trace.tail"
    rm -f "$trace"
  done
else
  printf '%s\n' 127 >"$out/strace-bwrap.exit"
  printf '%s\n' "strace unavailable" >"$out/strace-bwrap.stderr"
fi

product_alias=()
if [ -L /bin ] && [ "$(readlink /bin)" = "usr/bin" ]; then
  product_alias=(--symlink usr/bin /bin)
fi
record bwrap-landlock bwrap --unshare-all "${product_alias[@]}" "${base_mounts[@]}" \
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

exit 0
