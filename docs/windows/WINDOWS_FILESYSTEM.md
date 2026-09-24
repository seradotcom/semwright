# Windows filesystem confinement

Semwright must not translate the Linux `openat2` guarantee into a misleading `canonicalize + CreateFile` claim. The source implementation uses opened HANDLE metadata and a deliberately narrow capability.

Accepted initial root: absolute local path; readable=true; writable=false. Rejected roots include UNC, `\?\` extended paths and device paths. The root itself is opened with `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`, must not be a reparse point, and its volume/file ID is pinned.

A confined read accepts exactly one normal child component. It rejects nested separators, ADS colon syntax, dot/parent components, CON/PRN/AUX/NUL/CLOCK$, COM1..9/LPT1..9, trailing dots/spaces, reparse points, hard-link count != 1, another volume and files above 64 MiB/caller budget. It verifies final handle path and root identity before and after I/O.

Atomic writes and secure nested traversal intentionally return SandboxDenied/Unsupported until a root-relative handle traversal/rename design is proven under junction/reparse races. Linux and macOS implementations keep their existing stronger/different guarantees.
