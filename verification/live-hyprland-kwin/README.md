# Hardware-backed Hyprland live certification

This evidence closes the previously missing live Hyprland native-socket case without claiming that a nested compositor is equivalent to every physical Hyprland login.

## Certified baseline

- Semwright source: `8d76053ff8a63760b553aeebae9bb1733c28c1e2`.
- Executables: x86_64 binaries from GitHub Actions run `36199720444`, artifact `supply-chain-x86_64-8d76053ff8a63760b553aeebae9bb1733c28c1e2`.
- Parent compositor: KWin 6.7.5 in `--virtual` mode.
- Child compositor: Hyprland 0.56.2 / Aquamarine 0.15.1.
- Rendering: real host AMD `renderD129` through `amdgpu`.
- Fixture: native Wayland `foot` client.
- Container: no network and `no-new-privileges`.

The container recipe is pinned in `scripts/dev/hyprland-kwin-live-cert.Containerfile`. The Hyprland COPR repo uses RPM signature verification; the executed image reports Hyprland signed by key ID `A5B5F0CF64407CDC`, matching the observed COPR key fingerprint `CE4F9876716F2756FE6A576AA5B5F0CF64407CDC`. The actual live sequence is `scripts/dev/hyprland-kwin-live-cert.sh`.

## Executed assertions

The production Semwright broker and CLI discovered the `hyprland` backend as `SUPPORTED`, listed a native Wayland fixture, focused it, moved it to `111,77`, resized it to `500x320`, rejected its old reference with `StaleReference` after exit, and observed an empty client list after cleanup.

A second headless output was then created and configured at `800,0`, `640x480`, scale `1.25`, while the primary output remained scale `1.0`. This exercises mixed-scale compositor state; it is not evidence for physical mixed-DPI displays.

## Reproduction shape

Build the pinned certification image, obtain the exact Semwright binaries to certify, then run the container with only the selected render node and evidence directory exposed. The inner script requires `SEMWRIGHT_HYPR_BASELINE_SHA` and fails closed if its binaries, compositor tools, fixture or expected state are absent.

The executed run used the equivalent of:

```bash
docker build \
  -f scripts/dev/hyprland-kwin-live-cert.Containerfile \
  -t semwright-hyprland-cert:local .

docker run --rm \
  --network none \
  --security-opt no-new-privileges \
  --user 1000:1000 \
  --group-add 110 \
  --device /dev/dri/renderD129:/dev/dri/renderD129 \
  -e SEMWRIGHT_HYPR_BASELINE_SHA=<exact-source-sha> \
  -v "$PWD/scripts/dev/hyprland-kwin-live-cert.sh:/cert.sh:ro" \
  -v "$EVIDENCE_DIR:/evidence" \
  -v "$BIN_DIR/semwright:/test/semwright:ro" \
  -v "$BIN_DIR/semwrightd:/test/semwrightd:ro" \
  semwright-hyprland-cert:local /cert.sh
```

Group ID `110` was the host render group for the executed machine; reproductions must use the group that owns their selected render node rather than assuming that numeric ID.

## Physical-login preflight for the remaining R06 work

Before attempting the still-open physical Hyprland slice, run
`scripts/dev/hyprland-physical-preflight.sh` from the login that would produce the evidence. The
script is deliberately non-mutating: it checks the active logind session, Wayland/Hyprland identity,
the Hyprland compositor environment and monitor topology, then exits before any Semwright window or
input action.

It requires the explicit acknowledgment
`SEMWRIGHT_HYPR_PHYSICAL_ACK=I_AM_ON_A_DISPOSABLE_PHYSICAL_HYPRLAND_LOGIN`. It fails closed if the
compositor process inherited `WAYLAND_DISPLAY` (a nested-compositor signal), if the desktop/session
is not an active local Wayland Hyprland login, or if no physical connector-like output is visible.
Set `SEMWRIGHT_HYPR_REQUIRE_MIXED_SCALE=1` when the run is specifically intended to produce physical
mixed-scale evidence; that mode also requires at least two physical outputs with distinct scales.

A successful result is `PASS_PREFLIGHT_ONLY` with `certification_complete=false`. It is permission to
start the disposable physical-login procedure, **not** R06 closure evidence. The subsequent run must
still record the exact baseline/binary hashes and execute the remaining restart/reconnect,
focus-drift/cancellation, physical display and cleanup assertions called out in `RELEASE_BLOCKERS.md`.

## Evidence boundary

This certifies hardware-backed nested Hyprland native IPC, lifecycle/stale-ref handling, cleanup and a synthetic mixed-scale multi-output configuration. It does **not** certify a physical TTY/login Hyprland session, physical mixed-DPI monitors, compositor restart recovery, Hyprland portal/EIS consent, focus-drift negative cases, or in-flight cancellation.
