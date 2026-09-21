"""Semwright Blender add-on. Enable explicitly after setting SEMWRIGHT_BLENDER_WORKSPACE."""
bl_info = {
    "name": "Semwright semantic bridge", "author": "Semwright contributors",
    "version": (0, 9, 0), "blender": (4, 2, 0),
    "location": "Preferences > Add-ons", "category": "System",
    "description": "Local typed API; no arbitrary Python command",
}

_server = None


def register():
    import os
    from pathlib import Path
    import bpy
    from .commands import Commands
    from .host import Server, private_directory
    global _server
    if _server is not None:
        return
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    workspace = os.environ.get("SEMWRIGHT_BLENDER_WORKSPACE")
    if not runtime or not workspace:
        raise RuntimeError("Set XDG_RUNTIME_DIR and SEMWRIGHT_BLENDER_WORKSPACE before launching Blender")
    private_directory(runtime)
    directory = private_directory(Path(runtime) / "semwright-blender")
    server = Server(directory / "bridge.sock", Commands(bpy, workspace))
    server.start()
    try:
        bpy.app.timers.register(server.drain, first_interval=0.02, persistent=True)
    except BaseException:
        server.stop()
        raise
    _server = server


def unregister():
    import bpy
    global _server
    if _server is not None:
        if bpy.app.timers.is_registered(_server.drain):
            bpy.app.timers.unregister(_server.drain)
        _server.stop()
        _server = None
