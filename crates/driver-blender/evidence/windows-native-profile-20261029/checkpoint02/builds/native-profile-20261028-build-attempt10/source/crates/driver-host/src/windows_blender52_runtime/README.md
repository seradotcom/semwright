# Compiled first-party Blender runtime

These seven files are byte copies of the canonical first-party sources named in manifest.json.
The Windows Host compiles them into its executable. It never loads code from the workspace.
The Host-local bootstrap and empty initializer remain in the complete private-runtime inventory.

Repository builds compare every canonical resource and fail on missing files or drift on every host platform.
Standalone Cargo packages use the mandatory included copies.
The build script reads only; it never synchronizes, downloads or launches anything.

After reviewing a canonical change, explicitly run:
python crates/driver-host/tools/sync_windows_blender_runtime.py
The --check option verifies copies and manifest hashes without writing.
Python sources use LF bytes so checkout conversion does not change the embedded inventory.

The manifest provides source traceability, not an external signature.
Workspace compilation and package completeness are separate from registry installation qualification.
