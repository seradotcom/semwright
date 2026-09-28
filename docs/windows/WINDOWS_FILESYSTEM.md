# Windows filesystem confinement

Semwright does not translate the Linux `openat2` guarantee into a misleading pathname-equivalence claim. Windows scoped filesystem authority is enforced with pinned directory HANDLEs, HANDLE-relative NT opens and object metadata checks. The reported confinement is `WindowsHandleRelativeNoReparse`, deliberately distinct from `LinuxOpenat2NoSymlinksNoMounts`.

Accepted roots are explicit absolute local directories below a volume root. UNC, extended-length and device paths, volume roots and reparse-point roots are rejected. Read and write authority are independent owner grants. The root HANDLE pins its volume/file identity and remains authoritative even if the directory pathname is renamed after opening.

Relative paths are bounded to 64 normal components. Every component rejects NULs, ADS colon syntax, dot/parent traversal, reserved DOS device names, trailing dots/spaces and oversized names. Intermediate directories are opened relative to the already-pinned parent with `NtCreateFile`, `OBJECT_ATTRIBUTES.RootDirectory` and `FILE_OPEN_REPARSE_POINT`; each resulting HANDLE must be a same-volume, non-reparse directory. No confined operation falls back to `canonicalize + join + open`.

Reads open the final file relative to its pinned parent HANDLE. The final object must be a same-volume, non-reparse, non-directory, single-link file and stay within the caller/scoped byte budget. Root identity is rechecked around I/O.

Atomic writes create an exclusive UUID-named temporary file inside the pinned destination directory, without `FILE_SHARE_WRITE`; the temp is written and flushed, then renamed relative to the same parent HANDLE using `NtSetInformationFile(FileRenameInformation)` with `FILE_RENAME_INFORMATION.RootDirectory`. The committed destination is reopened by HANDLE and must have the same file identity as the temp. Failed pre-rename transactions mark the temp for deletion; failures after an ambiguous commit point are reported with uncertain outcome rather than claiming success.

Native Windows x64 and ARM64 CI exercises nested read/write and replacement, independent read/write grants, hard-link rejection, junction escape rejection, and continued access through the pinned root after the root pathname is renamed. This is a Windows-native confinement contract, not a claim of full semantic equivalence with Linux mount/no-xdev rules.
