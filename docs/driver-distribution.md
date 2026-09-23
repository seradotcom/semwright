# Driver distribution

Semwright driver distribution is owner-facing local tooling. Installing a package does **not**
execute the driver, run conformance, edit broker policy, or grant the resulting `driver:<id>`
scope. The daemon continues to load only manifest paths explicitly selected by its owner
configuration.

## Package format

`.swdp` v1 deliberately is not a tar/zip archive. It contains a fixed magic header, one bounded
JSON metadata document and exactly one pinned ELF payload. There are no package-controlled
filenames, symlinks, install scripts or extraction entries. The installer writes only:

```text
<XDG_DATA_HOME>/semwright/drivers/<id>/<version>/driver
<XDG_DATA_HOME>/semwright/drivers/<id>/<version>/manifest.json
<XDG_DATA_HOME>/semwright/drivers/<id>/<version>/receipt.json
<XDG_CONFIG_HOME>/semwright/drivers/<id>.json
```

The package metadata contains the strict Driver SDK manifest plus a SemVer requirement for the
compatible Semwright runtime. Both the package and executable are SHA-256 pinned.

Create and inspect a package:

```sh
semwright --json driver package create ./driver.json ./my-driver.swdp
semwright --json driver package inspect ./my-driver.swdp
```

Package creation copies bytes; it does not execute the driver.

## Static/local index

Index v1 is a bounded JSON file whose package paths are relative to the index directory. Entries
contain driver ID/version/publisher, package SHA-256 and size, Semwright SemVer compatibility, and
optional exact application versions. Duplicate ID/version entries, traversal paths, malformed
hashes and invalid compatibility expressions are rejected.

```sh
semwright --json driver index validate ./registry/index.json
semwright --json driver index search ./registry/index.json libreoffice   --application-version 24.2
```

The first implementation intentionally resolves only local/static indexes. Remote catalog
transport, signatures and a hosted marketplace are separate concerns; no network fetch is hidden
inside index resolution.

## Installation and updates

```sh
semwright --json --dry-run driver install ./registry/index.json libreoffice   --application-version 24.2

semwright --json driver install ./registry/index.json libreoffice   --application-version 24.2

semwright --json driver update ./registry/index.json libreoffice   --application-version 24.2
```

Installation verifies the index, package digest and size, package/entry identity, compatibility,
ELF digest and all Driver SDK manifest constraints. Files are staged in a private directory and
renamed into the versioned store. The active manifest is mode `0600`; the ELF is mode `0700`.
Older version directories remain available for inspection/removal. Protocol v1 does not claim an automatic rollback command.

A constrained index entry requires an explicit application version. Semwright does not infer a
version from arbitrary process output during installation.

Remove one receipt-bound version:

```sh
semwright --json driver remove libreoffice 1.0.0
```

Removal validates the canonical driver ID and SemVer before constructing paths, reads the
installation receipt, and removes the active manifest only when it points at that exact version.

## Security boundary

Distribution establishes **integrity and compatibility**, not trust or authority:

- packages cannot run install hooks;
- package-controlled archive paths do not exist;
- local package resolution cannot escape the canonical index directory;
- a package mismatch fails closed;
- install/update never changes policy grants;
- a newly installed driver is not contacted during installation;
- normal `driver conformance` remains a separate explicit action;
- the runtime DriverProvider still re-verifies the executable digest and applies Bubblewrap +
  Landlock before execution.

A future signing model can authenticate publisher identity in addition to the v1 hash/source
evidence. V1 does not pretend SHA-256 alone proves publisher identity.
