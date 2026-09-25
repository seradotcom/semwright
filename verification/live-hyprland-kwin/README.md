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

The container recipe is pinned in `scripts/dev/hyprland-kwin-live-cert.Containerfile`. The actual live sequence is `scripts/dev/hyprland-kwin-live-cert.sh`.

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

## Evidence boundary

This certifies hardware-backed nested Hyprland native IPC, lifecycle/stale-ref handling, cleanup and a synthetic mixed-scale multi-output configuration. It does **not** certify a physical TTY/login Hyprland session, physical mixed-DPI monitors, compositor restart recovery, Hyprland portal/EIS consent, focus-drift negative cases, or in-flight cancellation.
