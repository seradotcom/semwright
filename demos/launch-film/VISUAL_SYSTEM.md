# Launch film visual system

The film derives its palette and typography from the read-only Semwright site reference. It uses the light editorial system: paper `#f7f5ee`, surface `#fdfcf8`, ink `#202d42`, muted `#5b6471`, brand blue `#234ea2`, line `#dadbd4`, blue-soft `#e4ebf8`, peach `#f2ddc8`, mint `#dce9dc`, lavender `#e9e2f2`, and navy `#152b54`.

Typography is **Instrument Sans Variable** for interface/editorial text and **IBM Plex Mono** only for bounded technical annotations. Both are pinned through Fontsource in the managed Motion Canvas runtime. No proprietary font is required.

## Motion language

- Primary enter: 450–650 ms, `ease_out_cubic`.
- Structural draw: 500–850 ms line progression.
- Stagger: typically 90–350 ms depending on hierarchy.
- Emphasis: restrained `ease_out_back` scale from roughly 0.82–0.88 to 1.0.
- Scene transitions: 350 ms fade or directional slide.
- Camera tricks are deliberately omitted in v1; hierarchy and transforms carry the motion.
- Critical text remains on screen for seconds, not frames.

## Layout rules

The frame uses a generous 1920×1080 canvas, approximately 120 px outer safe area, 20–24 px rounded panels and 2–4 px structural lines. Large headlines remain under roughly 1,600 px width. The central information hierarchy is readable without audio.

The style deliberately avoids gradients, decorative glow, fake chrome, particle fields, lens flare and pseudo-HUD overlays. Movement exists to explain topology and sequencing.
