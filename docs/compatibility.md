# Compatibility and verification levels

The columns distinguish delivered source from execution evidence. The accepted Linux
development line has real Rust CI, and the platformized tree has a separate Linux regression
and Darwin verification matrix. Runtime doctor statuses are reachability/capability observations,
not a replacement for this evidence table.

| Environment or route | Implementation boundary | Verified here | Remaining |
|---|---|---|---|
| GNOME Wayland | AT-SPI + optional GJS bridge + portal Notify/Screenshot | Shared JS contract, syntax only | Compile Rust; real GNOME version/consent/window tests |
| Plasma Wayland | AT-SPI + KWin script/mailbox + portal | Shared JS contract, syntax only | Compile Rust; real KWin asynchronous lifecycle tests |
| Sway / i3-style IPC | Typed native socket commands/tree | Rust test sources only | Live Sway IPC, identity, workspace/focus tests |
| Hyprland | Native JSON socket / dispatch | Rust test sources only | Live version-specific IPC and restart tests |
| Native X11 | EWMH + explicit XTEST fallback | No Rust/Xvfb route executed | Bound synchronous I/O; lifecycle identity; real WM/XTEST tests |
| AT-SPI | Dedicated accessibility bus; bounded tree and semantic actions | Rust normalization/selector test sources | Private bus + GTK/Qt app tests, live event invalidation |
| Portal | Native Notify input + interactive Screenshot | Rust state/URI test sources | Consent, cancellation, session revocation on actual desktop |
| macOS host | AXUIElement + CoreGraphics + ScreenCaptureKit + NSPasteboard behind platform contracts | Linux regression + Darwin cross-target checks; native ARM64/Intel CI is a separate gate | Live TCC/AX/input/capture, Retina/multi-display, service/signing and driver isolation acceptance |
| Blender | Python typed host; Rust Unix client | Python fake-bpy/host tests | Live bpy, Blender background and GUI, Rust client |
| Chromium | Broker-launched private profile + CDP | Separate live Python CDP contract probe | Rust adapter and broker path, quotas/crash cleanup |
| Plugins | Bubblewrap + Landlock isolated ELF process | No sandbox execution | Negative tests on real kernels/user namespaces |
| x86_64 | Workspace and packaging definitions | Python/JS/C ran in x86_64 sandbox | Rust clean build and release install |
| aarch64 | Native CI runner/package definition | Not run | Resolve dependencies, compile, run, package |
| Nix | Guarded package expression | Text source only | Lockfile, evaluation, build; no flake lock |

Bridge manifests list candidate GNOME API versions 46–49, not a tested support guarantee.
Do not broaden that list for newer versions without testing. COSMIC and Windows have no
backend here. The macOS backend foundation is under verification and is not yet a support claim.
No low-level uinput/root helper is implemented. Chromium downloads
are disabled by default. Browser navigation defaults to `about:blank` until origins are
explicitly granted. This source is not a universal “works on Wayland” implementation.
