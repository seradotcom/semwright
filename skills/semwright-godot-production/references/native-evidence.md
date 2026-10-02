# Native evidence

Keep these evidence layers separate:

1. **Managed source readback**: bytes/hashes and derivation manifest under the granted output root.
2. **Godot parse/readback**: SceneTree/Node/Resource/Animation structure observed through the pinned Godot runtime or native addon.
3. **Persistence**: save, close, new process, load, and inspect. Reloading from the same ResourceLoader cache is insufficient.
4. **Runtime/playtest**: controlled inputs, elapsed physics ticks, state/events/counters, transform ranges, and bounded artifacts.
5. **Export**: package built through the registered export route, executed without the editor or Semwright authoring bridge.

Do not convert missing native evidence into PASS. Source-level `IN_SYNC` can coexist with UNKNOWN native parse/persistence/export status.

For animation collections, use `driver.godot.composition.native.tracks.page` for track metadata and `driver.godot.composition.native.keys.page` for one exact track's keyframes. Track cursors bind the managed source fingerprint plus normalized native projection; key cursors additionally bind player/library/animation/track index. Neither cursor trusts a process-local PID/nonce, so a fresh inspect process can continue only while the observed content is unchanged. Follow both collections through their final item; a stale cursor is a reacquire signal. For physics, assert tolerances and semantic events/state rather than claiming bit-identical cross-platform simulation.

Export evidence must check that broker/GitHub credentials, private authoring roots, editor bridge listeners, and debug hooks are absent or disabled in the normal distributable.
