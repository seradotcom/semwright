# Audio development packaging

GitHub Actions builds three real SWDP driver packages: faust-audio, audio-analysis and ardour-audio. It also creates a deterministic `semwright-audio-production` Skill ZIP, a deterministic `semwright-audio-agent-b-source.zip` recovery bundle with an internal per-file SHA-256 manifest, and a `PACKAGES.json` digest manifest.

SWDP v2 establishes driver executable integrity and compatibility; it does not grant driver scopes and does not execute conformance during installation.

Faust's fixed interpreter, the analysis meter and Ardour utilities are sealed executable tools. Current Driver Package v2 companion installation is data-oriented and does not automatically provision executable tool grants. Therefore those tools remain explicit owner-provisioned runtime prerequisites, pinned by SHA-256 in the generated driver manifest.

Faust and Ardour runtime JSON files are included as data companions for reproducibility, not as ambient authority. Faust runtime JSON pins the bounded recursive relative `.lib` inventory; production owners materialize that tree as ordinary read-only files because the runtime rejects symlinks and undeclared library bytes. The owner must materialize the declared workspace/tool grants through normal Semwright configuration.

The package lane validates each generated manifest, creates each SWDP through the repository CLI, re-inspects package metadata/digests, statically validates/inspects the Skill, bundles the Skill, and uploads only the compact package directory as CI evidence.

No release/tag is created and no package is published to a remote registry.

The pinned Faust standard-library closure is recursive but bounded by path length, 32 directory levels, file count and aggregate bytes; package/runtime/test code share the same limits.

The Ardour manifest allows up to 128 sandboxed processes/threads because native DAW initialization uses worker and backend threads; CPU, address-space, file-size and network bounds remain enforced.
