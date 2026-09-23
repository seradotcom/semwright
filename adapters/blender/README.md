# Blender adapter

The add-on receives allowlisted typed operations over a private user-owned Unix socket.
A main-thread timer drains a bounded queue; socket threads never call bpy. The Rust client
is a broker backend. No arbitrary `python.exec`, `eval`, expression, shell or remote TCP
interface exists.

Operations cover status/scene inspection, object listing/get/create/delete/transform,
collections, materials, render settings, render and scoped open/save. Consult
`../../schemas/commands.json` or `semwright commands search blender` for exact names.
Only allowlisted primitive kinds and constrained numeric/path inputs are accepted.
Duplicate/non-finite JSON, oversized frames and unknown command fields are rejected.

## Install after review

Choose a **private, canonical, trusted workspace**, not your whole home directory. Start
Blender from the same graphical login with:

```sh
export SEMWRIGHT_BLENDER_WORKSPACE=/home/YOUR_USER/projects/blender-fixture
blender
```

`XDG_RUNTIME_DIR` must already be provided by your login. Do not use `sudo`, a guessed
another-user runtime, or an untrusted shared workspace. Package the `semwright_blender/`
folder into an add-on ZIP for your Blender version, install through Blender preferences,
and explicitly enable it. Packaging uses the legacy add-on registration path; live
compatibility with your actual Blender version must be established before claiming support.

The default endpoint is `$XDG_RUNTIME_DIR/semwright-blender/bridge.sock`. A custom socket
is configured by top-level `blender_socket` in the broker's owner TOML. Allow
`blender.observe` separately from `blender.modify`. Deletion/open/save/render may require
additional capability or sensitive-action confirmation according to their descriptors.

```sh
semwright execute blender.status
semwright execute blender.scene.inspect
semwright execute blender.object.create --args-json '{"name":"FixtureCube","primitive":"cube"}'
```

## Path and privilege limitations

Paths are relative to the add-on's configured workspace, with traversal, symlink, hardlink
and suffix checks. `.blend` loading disables auto-run scripts, and saving is performed as
a copy. Nevertheless bpy opens paths, not broker-provided directory FDs; malicious concurrent
filesystem changes can race validation. Existing Blender has the interactive user's
privileges. This is **not** a sandbox for untrusted `.blend` files or plugins. Test only
trusted fixtures in a disposable account. Render duration may leave an uncertain timeout;
inspect the scene/output before retrying.

## Evidence and removal

The executed Python suite exercises framing, validation, socket queue handling and
operations against mocked bpy objects. **No Blender binary or GUI was run.** The Rust
bridge was not compiled. Disable the add-on in preferences before uninstalling its folder;
its unregister handler stops the host and removes its socket. Keep project files. Never
remove a live socket owned by an unrelated process to force startup.
