# Maintained Motion Canvas Vite plugin

The runtime pins Vite 6.4.6 and esbuild 0.25.12. Motion Canvas core, 2d and UI remain 3.17.2. The published Motion Canvas plugin 3.17.2 declares Vite 4 or 5 peers, so the SDK carries a private, separately named fork: `@semwright/motion-canvas-vite-plugin@3.17.2-semwright.1`. It does not impersonate an upstream release.

## Provenance and complete package

The base is the official [3.17.2 npm package](https://registry.npmjs.org/@motion-canvas/vite-plugin/3.17.2), upstream commit `e735a995aa738ca682feb4204a39ee7db7b5d279`. The original archive SHA256 is `3bcf37d161dc60213ea00fd64023d7c0e9fde6f85e1d00b2cf54b91c517e49ae`. Its 50 regular members, including the original MIT LICENSE, are represented in `integrations/motion-canvas/runtime/vendor/motion-canvas-vite-plugin.provenance.json`; each retains its original identity alongside the final fork identity. The fork adds one notice. Unchanged members remain byte-exact.

The source subtree, deterministic local tarball and provenance travel in the development package. The generated project embeds the tarball, provenance and verifier in its ordinary source inventory, within the existing 8 MiB compiler budget. The tarball dependency uses an exact `file:vendor/semwright-motion-canvas-vite-plugin-3.17.2-semwright.1.tgz` path and SHA512 integrity in the lock. It is not a directory dependency, override or peer exception.

The tar recipe is sorted regular USTAR entries under `package/`, mode 0644, uid/gid/mtime zero, followed by gzip with mtime zero. The provenance records all final sizes/hashes and the tar identity. The original source maps are retained for provenance; modified JS does not advertise those unchanged maps as mappings of the modified output. `.gitattributes` preserves vendor bytes across checkouts.

## Functional changes

- The asynchronous GLSL hook obtains `normalizePath` from Vite's ESM API. The managed renderer also normalizes its absolute project glob. Extension entry points are import specifiers and must use normalized paths.
- `settingsFile` is required. It is one ordinary filename within Vite's ordinary project root, such as `.semwright-settings.json`; there is no user-profile fallback. This keeps managed settings inside the owned staging directory.
- Settings reads and atomic replacements are bounded to 1 MiB, reject links/aliases and changed file identities, retain readback, and propagate invalid JSON, permission and filesystem errors. Missing settings alone produces `{}`. A failed write does not produce a meta ACK.
- Editor, scenes, metadata, assets, exporter, proxy and extension modules remain. The extension key is still `Symbol.for('@motion-canvas/vite-plugin/PLUGIN_OPTIONS')`, including `config`, `runtimeConfig` and `entryPoint`.
- Runtime probes report the fork's actual name/version. Core and 2d keep their separate 3.17.2 expectation; upstream plugin 3.17.2 cannot satisfy the fork pin.

## Runtime identity and exported dependency links

The Native helper first checks the driver-bound lock. It authenticates the verifier's bytes using an embedded SHA256 and imports those checked bytes with built-in Node imports only. The verifier pins the provenance digest, checks tar size/SHA256/SHA512 against the lock, enumerates the complete installed fork roster, checks every ordinary member, and checks the resolved entry. Only then does the helper import that exact entry URL and other third-party modules. The existing read-only runtime grant and process isolation remain necessary; these checks do not replace them or prove immunity to an external concurrent writer.

Native requires an ordinary `node_modules` root. Exported source projects retain the existing workflow that links `node_modules` to the installed runtime. Their separate read-only verifier resolves that explicit reference, verifies the same package bytes and resolved entry, and checks that the reference did not change. This does not grant Native authority or permit linked members inside the fork. The generated configuration imports the already verified entry URL, rather than resolving another copy after verification.

The helper, verifier, package, tarball, provenance and lock must be deployed together. `runtime-manifest.py` keeps its existing closed schema: its helper pin authenticates the verifier digest transitively. No receipt field or resource cap is widened.

## Focused controls

After exact-lock installation with scripts disabled:

```sh
npm ci --ignore-scripts --strict-peer-deps --engine-strict --no-audit --no-fund
node --test tests/runtime-contract.test.mjs tests/vite-fork-installed.mjs
npm run test:vite-fork
```

`tests/*.test.mjs` remains the source-only roster used without npm installation. The installed fork controls live in `tests/vite-fork-installed.mjs` and are invoked explicitly after exact-lock installation by the existing composition driver diagnostics job; they are not silently skipped when dependencies are absent.

The unit controls include tar/provenance/installed-member/extra-member/lock/entry/alias substitution, explicit settings ownership, invalid JSON, retained writes and ACK behavior. They separately prove the allowed exported dependency link and its rejection as a Native runtime root.

`test:vite-fork` performs real Vite builds of an exported Motion Canvas project and the managed `buildForEditor` branch. It uses the actual editor route, GLSL include handling, plugin extension callbacks, loopback WebSocket settings/project-meta ACKs with filesystem readback, module invalidation and a physical HMR source edit. It does not launch a browser or execute a renderer. Its temporary project is removed and its server is closed before the final result is emitted.

The current npm advisory result must retain its own source/lock/time identity. A local fork's behavior remains the maintainer's responsibility; zero advisories is not a runtime, security review, legal, Native, G2 or G3 acceptance. The existing SDK Rust/format/hosted gates and independent release review remain applicable.
