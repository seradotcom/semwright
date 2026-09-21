# Text statistics example

`semwright-example-plugin` implements `plugin.textstats.count`. It counts UTF-8 bytes,
Unicode scalar values, whitespace-delimited words and Rust text lines. It has no network,
filesystem mounts or arbitrary evaluation interface. It demonstrates the actual SDK
framing contract, not the entire application adapter surface.

Build with the workspace, create the real manifest digest using
`../../scripts/plugin-manifest.py`, then follow [the plugin guide](../../docs/plugins.md).
`manifest.template.json` is intentionally not installable: its executable and digest
must be generated from an actual ELF build. Neither the Rust plugin nor its sandbox was
executed in this handoff.
