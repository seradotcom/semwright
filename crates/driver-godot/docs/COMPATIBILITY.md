# Godot driver compatibility

## Certified target

The integration target for this closeout is **Godot 4.7.2-stable on Linux x86_64** with the
standard editor.

The real acceptance harness records the exact engine version and verifies the official
downloaded binary SHA-256 before use.

Editing uses an ordinary Godot `EditorPlugin` and official editor/scene/resource APIs. Under
Driver Host the authenticated bridge runs through a Host-managed loopback proxy into a private
Unix socket. Direct acceptance mode retains a loopback TCP listener.

Validation, runtime tests and exports use a separately digest-pinned Godot executable. On the
certified Linux Host that executable is verified and staged as a sealed secondary tool before
execution.

## Platform status

| Platform | Driver source | Real editor acceptance | Driver Host confinement |
| --- | --- | --- | --- |
| Linux x86_64 | supported | Godot 4.7.2 certified | bubblewrap + Landlock CI gate |
| Linux ARM64 | expected portable | not live-certified here | requires native Godot runner evidence |
| macOS | source-level portability | not certified | requires real Godot + platform-host acceptance |
| Windows | source-level portability | not certified | requires real Godot + Windows Driver Host acceptance |

No platform is inferred from a successful build on another OS.

## Version policy

The certified engine version is Godot 4.7.2. New Godot releases must pass the real editor
acceptance suite before the supported-version list is widened.

Unknown APIs may be described through bounded read-only introspection, but discovered methods are
never dynamically invoked.

## Distribution compatibility

Driver Package v2 can carry the Rust driver executable plus the reviewed
`addons/semwright/` companion tree. Package v1 remains readable for backward compatibility.

Companion installation is private to Semwright's installed-driver version directory. It does not
copy files into a project and does not enable the plugin. The owner must explicitly activate the
reviewed plugin in an approved Godot project.

## Known boundaries

- C#/.NET project workflows are not a certified surface.
- Arbitrary `@tool` projects, GDExtensions, custom importers and third-party EditorPlugins are
  executable software and are outside the trusted-project boundary.
- Executable export requires owner-installed export templates.
- Movie capture is optional and requires an explicitly configured display.
- Real-editor certification is platform-specific.

The default test fixture is disposable and contains no user project data.
