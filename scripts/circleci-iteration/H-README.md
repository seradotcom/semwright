# H CircleCI iteration

Config/checkout branch: `ci/h-circleci-iteration-06e6f5a`.
No pipeline parameter is needed. Default execution runs only the 55 lightweight
H harness controls against immutable lab `06e6f5a194f8f69d84ec3466ad0a0eba3367a0e0`.
The product target stays `cd518748f742025a251b78028613aa1b16919e73`; no product tests,
engine installation, native applications, model API or packaging run here.

Set both Config Source and Checkout Source to this branch when manually launching
the existing CircleCI project pipeline. Credentials remain in the authenticated
CircleCI web session; the agent currently has public read/monitor access only.
Configuration validation in Actions is not proof of actual CircleCI execution.

The wrapper binds lab/helper/technical product identities separately, requires all
55 controls and unmodified lab files, preserves failures and publishes hashed logs
with job/workflow IDs. Native evidence and final certification remain on Actions.
