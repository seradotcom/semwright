# CircleCI iteration helper

This helper preserves the frozen product branch. Its default `smoke` pipeline
checks connection/source identity without installing Rust or running product tests.

Branch: `ci/i-circleci-iteration-cd51874`. Configuration: `.circleci/config.yml`.
Default product: `cd518748f742025a251b78028613aa1b16919e73`.
Pipeline parameters: `candidate-sha` (full commit), `lane` (enum).

Choose `composition`, `graph-effects`, or `authoring` for an affected area.
Choose `all` to launch those three independent groups concurrently, subject to
the CircleCI account's available concurrency. There is no dependency between them.
`retest-affected` launches only composition and graph/effects after the first
configuration iteration; authoring already passed 129 controls in pipeline123.
The first failed pipeline stays preserved with its original source/suite IDs.
Compilation/test commands have a 25 minute shared budget per group. Registry
and Git dependency caches stay on CircleCI for seven days, keyed by Linux,
architecture, Rust 1.98.1, group, and exact Cargo.lock hash. No workstation build
or cache download is needed. No automatic full regression or paid account change
is performed.

Ubuntu prerequisites explicitly install Clang18 and its development headers,
bind libclang/resource paths, and keep native PipeWire bindgen compilation on
the runner. Audio model inventory uses its integration tests, not the empty
authoring library test target. Test-count guards retain their original floors.

The fixed command inventory comes from current owner Actions workflows. The
wrapper rejects zero/incomplete inventories, failures, ignored required tests,
and tracked source/lock changes. Blender/Godot authoring model tests remain
portable tests; this helper does not install native applications or claim their
acceptance. Existing GitHub-only owner evidence wrappers remain unchanged.

Each diagnostic records the product SHA separately from the helper SHA, actual
test counts, lock/log digests, job/workflow identity, duration and outcome under
`verification/circleci-integration/<group>/iteration.json`. Failed diagnostics
remain failed. `certification_eligible=false` is explicit. GitHub Actions remains
the certification authority; nothing here closes I, H, R16 or a release gate.

First use needs the CircleCI project connected to `seradotcom/semwright` and this
configuration branch. Project URL/access and actual pipeline execution must be
verified before reporting CircleCI operational. Never paste credentials in chat.
