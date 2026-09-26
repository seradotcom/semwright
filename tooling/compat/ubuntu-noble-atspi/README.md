# Ubuntu Noble AT-SPI crash backport

This directory carries a temporary compatibility backport for Ubuntu 24.04
(Noble) systems affected by GNOME Shell crashes in the AT-SPI/ATK bridge under
heavy accessibility automation.

The observed crash signature is:

```text
g_type_check_instance_is_fundamentally_a
g_object_ref
g_hash_table_foreach
libatk-bridge-2.0
libatspi
Mutter / GNOME Shell
```

Ubuntu bug 2158636 tracks this class of crash. Upstream at-spi2-core commit
`d442ee182ec8fa095c6bc5298a17663cfc70cf9a` changes SpiCache lifetime
management so it owns its weak references directly.

## What we patch

The builder starts from Ubuntu's exact Noble source commit
`f55067272e0eaa54b4208fa46541ba7d435a3ac2`
(`at-spi2-core 2.52.0-1build1`) and applies only the upstream 24-line/9-line
delta in this directory.

The local package version is `2.52.0-1build1+semwright1`. It sorts above the
stock Noble build but should be superseded by a later official Ubuntu revision.

## Safety model

Do not build or stress-test this on the user's primary GNOME session. GitHub
Actions builds the packages. The local installer is a separate explicit step
and requires sudo.

Semwright also has an automatic Noble legacy guard. On an unpatched 24.04 host
it caps traversal depth/node fanout and avoids the broad Object event
subscription. The guard disables automatically for the Semwright package or an
upstream at-spi2-core version >= 2.55.2.

The environment override `SEMWRIGHT_ATSPI_NOBLE_GUARD=on|off` exists for
controlled diagnosis only.

## Build

Run the workflow `Ubuntu Noble AT-SPI backport`, or build in a disposable
Ubuntu 24.04 environment:

```sh
tooling/compat/ubuntu-noble-atspi/prepare-source.sh /tmp/at-spi2-core
cd /tmp/at-spi2-core
dpkg-buildpackage -b -us -uc
```

After build, run `verify-debs.sh` against the directory containing the
packages. Installation requires a full logout/login because GNOME Shell loads
`libatk-bridge-2.0.so` into its own process.
