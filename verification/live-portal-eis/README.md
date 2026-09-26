# Portal/EIS live evidence

This directory separates evidence that is safe to retain from keyboard-targeting methods that were
invalidated during owner-observed testing.

- `gnome-connect-to-eis.json` retains the real GNOME portal consent/ConnectToEIS lifecycle,
  pointer delivery, coordinate and focus-drift evidence.
- `gnome-eis-cancellation.json` is explicitly marked `INVALIDATED` for keyboard target-delivery
  certification.
- `keyboard-targeting-methodology.json` records why owner-active and same-login nested GNOME
  methods are not acceptable keyboard safety boundaries.

## Isolation preflight before any future keyboard live run

Run `scripts/dev/eis-isolated-live-preflight.sh` **before** starting a RemoteDesktop portal
session. The script performs no input or portal mutation. It only checks that the current Wayland
session is inside one of these authority boundaries:

1. a virtual machine detected by `systemd-detect-virt --vm`; or
2. a local logind seat other than `seat0`.

The operator must explicitly acknowledge the disposable environment:

```sh
SEMWRIGHT_EIS_ISOLATION_ACK=I_AM_IN_A_DISPOSABLE_VM_OR_INDEPENDENT_SEAT \
SEMWRIGHT_EIS_EXPECT_DESKTOP=GNOME \
  scripts/dev/eis-isolated-live-preflight.sh
```

A successful result is `PASS_PREFLIGHT_ONLY` and always reports
`certification_complete=false`. The preflight deliberately rejects bare-metal `seat0`, which
also rejects a nested compositor that still shares the owner-active graphical login.

Only after this preflight passes may a future certification run request portal consent and use
keyboard input, and then only with disposable accounts/fixtures owned by the reviewer. Record exact
source/binary hashes, target-observed effects, cancellation timing, explicit portal stop, and
post-stop inactivity. R02 remains open until that isolated end-to-end evidence exists.
