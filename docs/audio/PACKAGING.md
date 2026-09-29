# Audio development packaging

GitHub Actions builds three real SWDP driver packages: faust-audio, audio-analysis and ardour-audio. It also creates a deterministic semwright-audio-production Skill ZIP and a PACKAGES.json digest manifest.

SWDP v2 establishes driver executable integrity and compatibility; it does not grant driver scopes and does not execute conformance during installation.

Faust's fixed interpreter, the analysis meter and Ardour utilities are sealed executable tools. Current Driver Package v2 companion installation is data-oriented and does not automatically provision executable tool grants. Therefore those tools remain explicit owner-provisioned runtime prerequisites, pinned by SHA-256 in the generated driver manifest.

Faust and Ardour runtime JSON files are included as data companions for reproducibility, not as ambient authority. The owner must materialize the declared workspace/tool grants through normal Semwright configuration.

The package lane validates each generated manifest, creates each SWDP through the repository CLI, re-inspects package metadata/digests, statically validates/inspects the Skill, bundles the Skill, and uploads only the compact package directory as CI evidence.

No release/tag is created and no package is published to a remote registry.
