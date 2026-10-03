# Authenticated CircleCI iteration launcher

Dispatch the registered `semantic-creation-integration.yml` workflow at ref
`ci/semwright-circleci-launcher`, with target `h-harness` or `portable` and the
selected portable lane. H needs no lane parameter in CircleCI itself.

The secret `CIRCLE_TOKEN` is injected by GitHub only into the hosted launch step.
The launcher uses a fixed project/repository, immutable reviewed helper targets,
and validates branch identity before a single POST. It records definition,
pipeline/workflow identities without token/error response bodies. A transport
failure after POST is ambiguous: inspect the pipeline list instead of retrying.

The workflow ends after observing created workflows; CircleCI performs actual
iteration work independently. This receipt is launch evidence, never proof that
tests passed, native acceptance, R16 review or a model productivity result.
