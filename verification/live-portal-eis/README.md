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

Only after this preflight passes may a certification run request portal consent and use keyboard
input, and then only with disposable accounts/fixtures owned by the reviewer. Record exact
source/binary hashes, target-observed effects, cancellation timing, explicit portal stop, and
post-stop inactivity.

## Isolated GNOME VM keyboard certification — 2026-09-26

`gnome-vm-keyboard-2026-09-26.json` records the first keyboard-targeting run accepted by this
methodology. The authority boundary was an independent KVM guest running Ubuntu 24.04.5, GNOME
Shell 46 and a real Wayland login session. VNC and SSH were bound to host loopback; the guest could
not enumerate or target owner-host windows.

With the candidate sustained pacing of four keycode-backed characters per 9 ms batch, a single
`input.type` call delivered **4096/4096** uppercase `A` characters to the disposable GTK target in
9.422 s and remained stable. A second run cancelled 4096 uppercase characters after about 20 ms;
GTK settled at four characters for about 1.46 s, a follow-up lowercase `z` succeeded, and Shift was
not left pressed. Explicit `portal.stop` then left EIS inactive with no input route.

Earlier owner-active and same-login nested-shell keyboard runs remain invalidated and are not
rehabilitated by this evidence. R02 remains open for portal-granted keyboard evidence on additional
supported Wayland desktops.
