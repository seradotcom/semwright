# Godot authoring research baseline

Role D, 2026-09-28. Product base `b736d41b61c4a4146c9e75c16796e251b025e69f`; A contract `26602e4b25929be869d69ef28fef4dd9713180d7` merged normally, without the media implementation. A C1 audio/AV contract inspected; no new AV type is needed by this compiler.

The existing native lane pins Godot **4.7.2-stable**, Linux x86_64, binary SHA-256 `8d106cbe6144c2dc7e881d61d2429c1a8a76e6b22ef48bd5e48dcf934953f71e`. Runtime acceptance remains pending until this branch actually runs it. Rust toolchain for CI is 1.98.1.

| Source | Observation and decision |
|---|---|
| https://docs.godotengine.org/en/4.7/classes/class_packedscene.html | Parent hierarchy is not ownership. Authored scenes use native nodes; save/reopen must observe owner and packed membership. |
| https://docs.godotengine.org/en/4.7/classes/class_resourcesaver.html | Flags alter external resource/path behavior. Preserve #171 scene-only mode exactly; do not claim it supports inherited/instanced scenes. |
| https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html | Cache bypass alone is not a new-process persistence proof. Use separate engine processes for acceptance. |
| https://docs.godotengine.org/en/4.7/classes/class_animation.html | Typed native value tracks, keys and libraries; no method tracks in this IR. D implements separate bounded native track/key paging whose cursors bind the stable managed observation rather than process-local identity. |
| https://docs.godotengine.org/en/4.7/tutorials/animation/animation_tree.html | AnimationTree must reference an AnimationPlayer; D now realizes typed state-machine and BlendSpace1D roots over authored clips and keeps control in generated backend code rather than AnimationPlayer playback. |
| https://docs.godotengine.org/en/4.7/classes/class_animationnodestatemachineplayback.html | State machines are controlled through parameters/playback with start/travel. D generates only explicit states/transitions and uses travel; no caller-supplied condition/expression is accepted. |
| https://docs.godotengine.org/en/4.7/tutorials/migrating/upgrading_to_godot_4.7.html | Godot 4.7 replaces BlendSpace sync bool with SyncMode. D models the four modes explicitly and requires cyclic_length only for constant cyclic sync. |
| https://docs.godotengine.org/en/4.7/classes/class_resource.html | resource_local_to_scene is the native isolation primitive. D models shared versus local-to-scene material bindings explicitly; shared bindings reject per-instance overrides and local copies have separate managed resources. |
| https://docs.godotengine.org/en/4.7/classes/class_node.html | Node.reparent(new_parent, keep_global_transform) preserves global transform when supported; D exposes only typed managed-node reparenting, rejects self/descendant/cross-tree targets, and permits keep_global only where spatial semantics are explicit. |
| https://docs.godotengine.org/en/4.7/classes/class_node3d.html | Node3D position/rotation/scale are parent-space transforms and rotation is radians. D's typed rotation/scale actions use those native properties rather than arbitrary property names. |
| https://docs.godotengine.org/en/4.7/classes/class_@globalscope.html | Pinned key codes: left 4194319, up 4194320, right 4194321, down 4194322. Add native enum parity checks. |
| https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html | Require parsed native output and expected tests; exit zero alone is insufficient. |
| https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows | New branch diagnostics use push, not an assumed default-branch dispatch registration. |

No engine, build, tests, dependencies or targets were run/installed locally. Local operations are Git/gh, source editing, installed rustfmt formatting and lightweight syntax checks. Initial own worktree 30 MiB; free disk 6.4 GiB. Native-integrations failure on the base is AT-SPI job 109181209084/run 36497759637, not Godot evidence.
