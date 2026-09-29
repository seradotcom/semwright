# Owner-pinned Faust interpreter helper

Original Semwright MIT/Apache-2.0 helper, dynamically linked to the system libfaust and libsndfile. No upstream architecture source is copied or modified. The unmodified interpreter header has its upstream LGPL-2.1-or-later architecture exception. Runtime distribution must retain the system libraries and their notices/obligations; development packages do not embed them.

The trusted Rust translator produces bounded Faust source. On Linux, Driver Host materializes this fixed executable from sealed, verified bytes; the driver runs it inside the existing Bubblewrap/Landlock sandbox. Protocol-v4 Host-mediated RPC is currently Windows-only and does not carry these audio workspace grants. There is no generated native executable, C++ compiler, shell, network, microphone or physical output device in this runtime. The official interpreter executes DSP in bounded buffers. It is not a claim that the upstream compiler is immune to hostile inputs: Host CPU/memory/process/output/time bounds remain required.

Runtime baseline: Ubuntu 24.04 package Faust 2.70.3+ds-1.1build2. Build and all native execution occur on GitHub-hosted runners. The render helper supports WAV PCM16/24/32 and FLAC PCM16/24; FLAC32 is rejected rather than silently quantized. Integer conversion saturates; the receipt counts out-of-range pre-conversion samples. No loudness/true-peak guarantee is implied.

The owner-provisioned Faust library grant is a materialized read-only tree of regular `.lib` files. Semwright recursively inventories bounded relative paths, rejects symlinks/untrusted writers, and verifies the exact SHA-256 of every declared library before invoking the helper. This preserves the real standard-library import closure without granting an ambient `/usr/share/faust` mount.

The output directory is private scratch within an owner-granted output mount. Rust validates/hash-pins the resulting artifact before no-clobber publication. Interrupted scratch is never a published final output.
