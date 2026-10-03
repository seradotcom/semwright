# H CircleCI iteration

Config/checkout branch: `ci/h-circleci-iteration-6fe1994`.
No pipeline parameter is needed. Default execution runs only the 60 lightweight
H harness controls against immutable lab `6fe1994d9ab25f44631ef8c41e9dad8f05aee0ca`.
The product target stays `cd518748f742025a251b78028613aa1b16919e73`; no product tests,
engine installation, native applications, model API or packaging run here.

Set both Config Source and Checkout Source to this branch when manually launching
the existing CircleCI project pipeline. Credentials remain in the authenticated
CircleCI web session; the agent currently has public read/monitor access only.
Configuration validation in Actions is not proof of actual CircleCI execution.

The wrapper binds lab/helper/technical product identities separately, requires all
60 controls and unmodified lab files, preserves failures and publishes hashed logs
with job/workflow IDs. Native evidence and final certification remain on Actions.
