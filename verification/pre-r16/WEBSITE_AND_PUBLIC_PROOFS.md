# Website and public proof observation

The website checkout was read-only and clean at `a1ee04fc3f5e6ed3d486e679d8c40077e24752b0`.
It deliberately identifies product revision `9ecf35fd9c3d6fbcbc1f8b72b8d4734c70037ffa`, reviewed
2026-09-27, and explicitly calls it a development snapshot rather than a release candidate.
Its historical pin is documented, not silently interpreted as the current main SHA.
No website rebuild, deployment, design change or source edit was performed.

## Figma

The public proof records Figma Desktop 126.5.6 at product SHA
`3cd86958f70f7a8231d31e492b5505acff19dae0`, with an executable digest, exported artifact digest,
14 persisted roots and three prototype flows. Its route is the first-party driver, authenticated
loopback bridge and official Plugin API. It expressly does not claim every operation traversed
CLI/daemon/broker/policy, WCAG certification, or all FigJam/Slides/Buzz/Motion surfaces.
This is historical real-application evidence, not merely a fake-host fixture; this audit did not
reproduce it or promote it to current-SHA certification. The root evidence matrix is corrected
accordingly rather than silently discarding the narrower real proof.

## Godot

Parcel Lantern records product SHA `9ecf35fd9c3d6fbcbc1f8b72b8d4734c70037ffa` after PR149,
Godot 4.7.2 and the CLI -> daemon -> broker/policy -> sandboxed driver -> Host-mediated loopback ->
EditorPlugin route. It records ten operations, consent boundaries, preserved scene bytes and clean
restart. Movie capture through the broker and universal Godot/version support are expressly not
claimed. This record is not a new execution by the audit.

## Disposition

Historical website pin: deliberate. Update after an actual reviewed release/candidate decision,
not merely because main moved. The site does not supply an independent R16 report. Root docs now
acknowledge the limited real Figma/Godot records. Source file hashes and complete small proof records
are preserved in `inventory/site-observation.json`; image/video assets were not copied or edited.
