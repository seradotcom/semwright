# Motion

Figma Motion is **Beta** as of the 2026-09-23 official documentation check. The pack pins plugin typings 1.138.0 and reports Motion as supported-with-limitation.

Implemented plugin primitives: list animation styles, inspect animationStyles/manualKeyframeTracks/animations/timelines, apply/remove animation style, apply/remove manual keyframe track, set timeline duration, physical spring normalization. Rust validates finite bounded timeline durations and sorted bounded keyframes.

Animated export is represented in the catalog, but real MP4/GIF/WebM acceptance requires an authorized disposable Figma session and supported animated top-level node.
