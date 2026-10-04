# Audio compatibility matrix

| Surface | Linux x86_64 GitHub runner | Windows | macOS | Notes |
|---|---|---|---|---|
| audio-domain / audio-authoring | required portable Rust gate | portable compile required at final candidate | portable compile required at final candidate | no native DAW dependency |
| Faust source translation | supported portable Rust | portable translation only | portable translation only | source generation is backend-neutral |
| Faust native interpreter/render | supported, pinned 2.37.3/2.70.3 baselines; synth/SFX, hash-pinned sample and polyphonic MIDI lanes | blocked | blocked | current sealed runtime implementation is Linux-only; both declared Faust versions run native CI |
| native acoustic analysis | supported, libebur128 1.2.6 | blocked | blocked | current tool/mount runtime is Linux-only |
| Ardour deep managed sessions | supported baseline Ardour 8.4.0 | blocked | blocked | current native acceptance and sealed tools are Linux-only |
| Ardour live OSC | driver semantics implemented | not certified | not certified | packaged Linux manifest uses explicit Driver Host loopback port 3819; no general network |
| physical audio/MIDI hardware | not certified | not certified | not certified | no native CI depends on user hardware |

Portable compilation does not imply native runtime support. Unsupported native platforms fail closed instead of launching unsandboxed fallbacks.

The declared production/native target for this mission is Linux with pinned runtimes. Final Composition+Audio integration should compile the portable crates on the repository's normal Linux/Windows/macOS matrix and run native audio acceptance only where its isolation/runtime prerequisites are supported.
