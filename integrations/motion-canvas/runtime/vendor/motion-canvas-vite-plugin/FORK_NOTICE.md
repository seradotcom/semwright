# Semwright Motion Canvas Vite plugin fork

This is a maintained fork of @motion-canvas/vite-plugin 3.17.2, upstream
git e735a995aa738ca682feb4204a39ee7db7b5d279. The original MIT LICENSE and
all 50 published members are retained. The package has its own name and
version; it is not an upstream Motion Canvas release.

Changes: Vite 6.4.6 contract; ESM normalizePath import in the asynchronous
GLSL hook; explicit ordinary project-root settingsFile; bounded settings
read/write with link, identity and readback checks. Editor, project, scene,
asset, exporter, proxy, plugin-extension and meta/HMR protocols remain.

Original source maps are retained as provenance. The three changed JS files
do not advertise those unchanged maps as mappings for the modified output.
The original upstream roster and the final fork roster are recorded in the
adjacent, driver-authenticated motion-canvas-vite-plugin.provenance.json.

No lifecycle scripts run to assemble this vendored package. Its deterministic
tar archive has sorted regular members, mode 0644, uid/gid/mtime 0 and gzip
mtime 0. The SDK includes source, archive and license in its development
package and embeds the archive/provenance in generated projects.

This source adaptation is not render, Native, G2/G3 or legal acceptance.
