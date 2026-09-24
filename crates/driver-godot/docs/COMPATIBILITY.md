# Godot driver compatibility

## Certified target

The integration target is **Godot 4.7.2-stable** with the standard Linux x86_64 editor.
The real acceptance harness records the exact engine version and verifies the downloaded
binary SHA-256 before use.

The driver communicates with a normal `EditorPlugin` through an authenticated loopback
WebSocket. Editing uses official editor/scene/resource APIs. Validation, runtime tests and
exports use a separately digest-pinned Godot executable.

## Platform status

| Platform | Driver source | Real editor acceptance | Driver Host confinement |
| --- | --- | --- | --- |
| Linux x86_64 | supported | Godot 4.7.2 tested | bubblewrap + Landlock CI gate |
| Linux ARM64 | expected portable | not live-tested here | requires native runner evidence |
| macOS | source-level portability only | not certified | arbitrary driver launch remains host-dependent |
| Windows | source-level portability only | not certified | requires Windows Driver Host acceptance |

No platform is inferred from a successful build on another OS.
## Version policy

The manifest declares Godot 4.7.2. New Godot releases must pass the real editor acceptance
before the supported version list is widened. Unknown APIs are not discovered and invoked
dynamically.

## Known boundaries

- C#/.NET project workflows are not a certified surface.
- Arbitrary `@tool` projects, GDExtensions, custom importers and third-party EditorPlugins are
  executable software and are outside the trusted-project boundary.
- Executable export requires owner-installed export templates.
- Movie capture is optional and requires an explicitly configured display.
- The companion EditorPlugin must currently be installed into the approved project separately
  from the driver executable package.

The default test fixture is disposable and contains no user project data.
