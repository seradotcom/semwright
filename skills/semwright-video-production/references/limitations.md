# Limitations

- Motion Canvas authoring does not replace video-domain or an NLE timeline.
- A managed render is not evidence of visual taste, comprehension, accessibility or brand quality.
- Bounds intersection alone is not a universal collision defect; transforms, masks, clips and intentional overlays matter.
- Font fallback changes metrics. If the runtime cannot prove the requested face/glyph coverage, verification remains UNKNOWN.
- Stateful generators make arbitrary partial rendering unsafe unless an equivalent checkpoint/replay strategy is proven. Prefer conservative replay.
- Raster output is not native object editability. Semantic text remains editable in the managed project, not inside an MP4.
- MLT delivery can change timestamps, padding or encoded duration. Verify the encoded result independently.
- Live application support depends on the pinned runtime and Driver Host environment. Portable compilation does not certify a live backend.
