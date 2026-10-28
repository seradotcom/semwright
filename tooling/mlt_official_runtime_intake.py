#!/usr/bin/env python3
"""Authenticate two Ubuntu MLT runtime files without installing or loading them.

The signing authority is the full Ubuntu Archive 2018 fingerprint pinned below.
The runner keyring supplies candidate key material, whose original bytes and
metadata are retained without modifying permissions. This source-only intake
preserves signed metadata and package bytes for separate admission. Its receipt
is evidence, not a replacement for an externally pinned receipt digest or for a
physical loaded-module observation.
"""
from __future__ import annotations

import argparse
import datetime as dt
import email.utils
import hashlib
import io
import json
import lzma
import os
from pathlib import Path, PurePosixPath
import platform
import re
import ssl
import stat
import subprocess
import sys
import tarfile
import urllib.request

SCHEMA = "motionwright.mlt.official-runtime-intake/1"
ARCHIVE = "https://archive.ubuntu.com/ubuntu/"
RELEASE = "dists/noble/InRelease"
INDEX = "universe/binary-amd64/Packages"
VERSION = "7.22.0-1build6"
KEYRING = Path("/usr/share/keyrings/ubuntu-archive-keyring.gpg")
# Ubuntu Security FAQ, GPG Keys: Archive 2018, RSA 4096. This fixed Noble
# profile authorizes that primary key directly, not arbitrary signing subkeys.
ARCHIVE_SIGNING_FINGERPRINT = "F6ECB3762474EDA9D21B7022871920D1991BC93C"
SIGNING_AUTHORITY_URL = "https://wiki.ubuntu.com/Security/FAQ#GPG_Keys"
PACKAGE_MEMBERS = {
    "libmlt7": ("original_module", "/usr/lib/x86_64-linux-gnu/mlt-7/libmltavformat.so"),
    "melt": ("melt", "/usr/bin/melt-7"),
}
# This fixed package exposes its ordinary ELF through one signed, relative
# alias. Verify that archive relationship without following filesystem links.
PACKAGE_ALIASES = {
    "/usr/bin/melt-7": {"archive_path": "usr/bin/melt",
                       "linkname": "melt-7", "target_path": "usr/bin/melt-7"},
}
LIMIT_METADATA = 4 * 1024 * 1024
LIMIT_COMPRESSED = 64 * 1024 * 1024
LIMIT_INDEX = 256 * 1024 * 1024
LIMIT_PACKAGE = 64 * 1024 * 1024
LIMIT_TAR = 128 * 1024 * 1024
LIMIT_MEMBER = 32 * 1024 * 1024
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
FINGERPRINT = re.compile(r"(?:[0-9A-F]{40}|[0-9A-F]{64})\Z")


class IntakeError(RuntimeError):
    """The source authentication or evidence contract was not satisfied."""


def require(condition, message):
    if not condition:
        raise IntakeError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def exclusive(path, data):
    with Path(path).open("xb") as handle:
        handle.write(data)
        handle.flush()
        os.fsync(handle.fileno())


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def capture_regular(path, limit):
    """Read one stable handle and preserve observed metadata, without granting trust."""
    path = Path(path)
    require(not path.is_symlink(), "symlink input")
    # A substituted FIFO must reach fstat without waiting for a writer.
    flags = (os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
             | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0))
    fd = os.open(path, flags)
    primary = None
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and 0 <= before.st_size <= limit,
                "input is not a bounded regular file")
        chunks, total = [], 0
        while True:
            chunk = os.read(fd, min(65536, limit + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            require(total <= limit, "input exceeds size limit")
        after = os.fstat(fd)
        namespace = path.lstat()
        def identity(value):
            return {"device": value.st_dev, "inode": value.st_ino,
                    "size": value.st_size, "mode": value.st_mode,
                    "uid": value.st_uid, "gid": value.st_gid,
                    "mtime_ns": value.st_mtime_ns, "ctime_ns": value.st_ctime_ns}
        observations = [identity(value) for value in (before, after, namespace)]
        require(observations[0] == observations[1] == observations[2],
                "input changed during read")
        require(total == before.st_size, "input read size differs")
        data = b"".join(chunks)
        return data, {"path": str(path), "sha256": digest(data), "size": len(data),
                      "before": observations[0], "after": observations[1],
                      "namespace_after": observations[2]}
    except BaseException as error:
        primary = error
        raise
    finally:
        try:
            os.close(fd)
        except BaseException as close_error:
            if primary is None:
                raise
            # The descriptor may already be closed. Never retry close or let
            # its secondary failure replace the original read/cancel error.
            try:
                primary.add_note("capture_regular close failed: " + repr(close_error))
            except BaseException:
                pass


def read_regular(path, limit):
    return capture_regular(path, limit)[0]


def descriptor(path, root, limit):
    data = read_regular(path, limit)
    return {"file": Path(path).relative_to(root).as_posix(),
            "sha256": digest(data), "size": len(data)}


def deb822_stanzas(text):
    """Parse bounded deb822 records, rejecting case-insensitive duplicate fields."""
    fields, current = {}, None
    for line in text.split("\n"):
        if line.endswith("\r"):
            line = line[:-1]
        require(len(line) <= 1024 * 1024, "metadata line too long")
        require(not any(ord(c) < 32 and c != "\t" or ord(c) == 127 for c in line),
                "metadata control character")
        if not line:
            if fields:
                yield fields
                fields, current = {}, None
            continue
        if line[0] in " \t":
            require(current is not None, "orphan continuation")
            fields[current] += "\n" + line[1:]
            continue
        match = re.fullmatch(r"([A-Za-z0-9][A-Za-z0-9-]*):[ \t]*(.*)", line)
        require(match is not None, "malformed metadata field")
        current = match[1].lower()
        require(current not in fields, "duplicate metadata field")
        fields[current] = match[2].strip(" \t")
    if fields:
        yield fields


def date_value(value):
    try:
        result = email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError, OverflowError) as error:
        raise IntakeError("invalid signed metadata date") from error
    require(result is not None and result.tzinfo is not None, "date lacks timezone")
    return result.astimezone(dt.timezone.utc)


def parse_release(data, now):
    require(0 < len(data) <= LIMIT_METADATA, "release payload size")
    records = list(deb822_stanzas(data.decode("utf-8", "strict")))
    require(len(records) == 1, "expected exactly one signed Release record")
    row = records[0]
    for key, value in (("origin", "Ubuntu"), ("label", "Ubuntu"), ("suite", "noble"),
                       ("codename", "noble"), ("version", "24.04")):
        require(row.get(key) == value, "wrong signed release " + key)
    require("amd64" in row.get("architectures", "").split(), "amd64 absent")
    require("universe" in row.get("components", "").split(), "universe absent")
    signed_date = date_value(row.get("date"))
    require(signed_date <= now, "signed Release date is in the future")
    until_text = row.get("valid-until")
    if until_text is not None:
        until = date_value(until_text)
        require(until > signed_date and until > now, "signed Release is expired")
    sums = {}
    require("sha256" in row, "signed SHA256 table absent")
    for line in row["sha256"].splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"\s*([0-9a-f]{64})\s+([0-9]+)\s+(\S+)\s*", line)
        require(match is not None, "malformed signed SHA256 entry")
        name = match[3]
        require(name not in sums, "duplicate signed SHA256 path")
        sums[name] = {"sha256": match[1], "size": int(match[2])}
    for name, limit in ((INDEX, LIMIT_INDEX), (INDEX + ".xz", LIMIT_COMPRESSED)):
        require(name in sums and 0 < sums[name]["size"] <= limit, "index absent or too large")
    return {"date": row["date"], "valid_until": until_text,
            "valid_until_policy": ("supplied_date_enforced" if until_text is not None
                                   else "fixed_release_pocket_no_freshness_claim"),
            "uncompressed": sums[INDEX], "compressed": sums[INDEX + ".xz"]}


def parse_signature_status(data, returncode, now):
    require(returncode == 0, "gpgv failed")
    require(0 < len(data) <= LIMIT_METADATA, "gpgv status size")
    failures = {"BADSIG", "ERRSIG", "NO_PUBKEY", "EXPSIG", "EXPKEYSIG", "REVKEYSIG",
                "KEYREVOKED", "NODATA", "BADARMOR", "FAILURE", "ERROR"}
    signatures, good_ids = [], []
    for line in data.decode("utf-8", "strict").splitlines():
        require(line.startswith("[GNUPG:] "), "unframed gpgv status")
        tokens = line[len("[GNUPG:] "):].split()
        require(tokens, "empty gpgv status")
        tag, args = tokens[0], tokens[1:]
        require(tag not in failures, "gpgv negative status: " + tag)
        if tag == "GOODSIG":
            require(args and args[0] in {
                ARCHIVE_SIGNING_FINGERPRINT[-16:], ARCHIVE_SIGNING_FINGERPRINT},
                "GOODSIG is not the authorized signing key")
            good_ids.append(args[0])
        if tag != "VALIDSIG":
            continue
        require(len(args) == 10, "unexpected OpenPGP VALIDSIG fields")
        fpr, creation, timestamp, expiry, version, reserved, public_algo, hash_algo, cls, primary = args
        require(FINGERPRINT.fullmatch(fpr) and FINGERPRINT.fullmatch(primary),
                "invalid signer fingerprint")
        require(fpr == primary == ARCHIVE_SIGNING_FINGERPRINT,
                "VALIDSIG signing and primary fingerprints are not authorized")
        require(timestamp.isdecimal() and expiry.isdecimal(), "invalid signature times")
        created, expires = int(timestamp), int(expiry)
        require(created <= int(now.timestamp()) and (expires == 0 or expires > now.timestamp()),
                "signature is future or expired")
        require(dt.datetime.fromtimestamp(created, dt.timezone.utc).date().isoformat() == creation,
                "signature date disagrees")
        require(version in {"4", "5"} and reserved == "0" and cls == "01",
                "not a supported cleartext signature")
        require(hash_algo in {"8", "9", "10"} and public_algo.isdecimal(),
                "weak or malformed signature algorithm")
        signatures.append({"fingerprint": fpr, "primary_fingerprint": primary,
                           "creation_date": creation, "created_epoch": created,
                           "expires_epoch": expires, "public_key_algorithm": int(public_algo),
                           "hash_algorithm": int(hash_algo), "signature_class": cls})
    require(len(signatures) == 1, "expected exactly one authorized valid signature")
    require(len(good_ids) == len(signatures) and len(set(good_ids)) == len(good_ids),
            "missing or duplicate good signature status")
    require(good_ids[0] in {signatures[0]["fingerprint"],
                           signatures[0]["fingerprint"][-16:]},
            "GOODSIG and VALIDSIG identities differ")
    return signatures


def verify_bytes(data, pin):
    require(set(pin) == {"sha256", "size"} and HEX64.fullmatch(pin["sha256"]),
            "invalid digest binding")
    require(type(pin["size"]) is int and pin["size"] >= 0, "invalid byte count")
    require(len(data) == pin["size"] and digest(data) == pin["sha256"],
            "content differs from authenticated digest or size")


def expand_index(data, release):
    verify_bytes(data, release["compressed"])
    require(0 < len(data) <= LIMIT_COMPRESSED, "compressed index limit")
    expected = release["uncompressed"]
    require(type(expected["size"]) is int and 0 < expected["size"] <= LIMIT_INDEX,
            "uncompressed index limit")
    # A standalone XZ file may contain concatenated streams, including an empty
    # terminal stream, and zero padding in multiples of four bytes. Both signed
    # digests still bind the entire compressed and expanded file.
    remaining, result, streams = data, bytearray(), 0
    while remaining:
        streams += 1
        require(streams <= 32, "too many XZ index streams")
        try:
            decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=LIMIT_INDEX)
            chunk = decoder.decompress(remaining, max_length=expected["size"] - len(result) + 1)
        except lzma.LZMAError as error:
            raise IntakeError("invalid or excessive XZ stream") from error
        result.extend(chunk)
        require(len(result) <= expected["size"], "expanded index exceeds authenticated size")
        require(decoder.eof, "truncated XZ index stream")
        unused = decoder.unused_data
        require(len(unused) < len(remaining), "XZ decoder made no progress")
        padding = len(unused) - len(unused.lstrip(b"\x00"))
        require(padding % 4 == 0, "misaligned XZ stream padding")
        remaining = unused[padding:]
    result = bytes(result)
    verify_bytes(result, expected)
    return result


def select_packages(data):
    require(0 < len(data) <= LIMIT_INDEX, "Packages size")
    result = {}
    for fields in deb822_stanzas(data.decode("utf-8", "strict")):
        name = fields.get("package")
        if name not in PACKAGE_MEMBERS:
            continue
        require(name not in result, "duplicate selected package")
        require(fields.get("version") == VERSION and fields.get("architecture") == "amd64",
                "selected package version or architecture differs")
        filename = "pool/universe/m/mlt/" + name + "_" + VERSION + "_amd64.deb"
        require(fields.get("filename") == filename, "selected package URL differs")
        require(fields.get("source") in ("mlt", "mlt (" + VERSION + ")"),
                "selected source package differs")
        require(HEX64.fullmatch(fields.get("sha256", "")), "package SHA256 absent or malformed")
        require(re.fullmatch(r"[0-9]+", fields.get("size", "")), "package size malformed")
        size = int(fields["size"])
        require(0 < size <= LIMIT_PACKAGE, "package size limit")
        result[name] = {"package": name, "version": VERSION, "architecture": "amd64",
                        "filename": filename, "url": ARCHIVE + filename,
                        "sha256": fields["sha256"], "size": size}
    require(set(result) == set(PACKAGE_MEMBERS), "required package missing")
    return result


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        raise IntakeError("archive redirect refused")


def download(relative, destination, limit, pin=None):
    require(relative in {RELEASE, "dists/noble/" + INDEX + ".xz"} or
            relative in {"pool/universe/m/mlt/" + n + "_" + VERSION + "_amd64.deb"
                         for n in PACKAGE_MEMBERS}, "download path not allowlisted")
    url = ARCHIVE + relative
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}), NoRedirect(),
        urllib.request.HTTPSHandler(context=ssl.create_default_context()))
    request = urllib.request.Request(url, headers={"Accept-Encoding": "identity"})
    with opener.open(request, timeout=60) as response:
        require(response.status == 200 and response.geturl() == url, "archive response changed")
        require(response.headers.get("Content-Encoding", "identity") == "identity",
                "unexpected HTTP encoding")
        announced = response.headers.get("Content-Length")
        if announced is not None:
            require(re.fullmatch(r"[0-9]+", announced) and int(announced) <= limit,
                    "HTTP size exceeds limit")
        total, checksum = 0, hashlib.sha256()
        with Path(destination).open("xb") as handle:
            while True:
                chunk = response.read(65536)
                if not chunk:
                    break
                total += len(chunk)
                require(total <= limit, "download exceeds size limit")
                checksum.update(chunk)
                handle.write(chunk)
            handle.flush()
            os.fsync(handle.fileno())
        require(announced is None or total == int(announced), "HTTP size mismatch")
    if pin is not None:
        require(total == pin["size"] and checksum.hexdigest() == pin["sha256"],
                "download differs from authenticated digest or size")
    return {"url": url, "size": total, "sha256": checksum.hexdigest()}


def _limit_child_output():
    import resource
    resource.setrlimit(resource.RLIMIT_FSIZE, (LIMIT_TAR, LIMIT_TAR))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def run_tool(argv, output, error, root, *, status=None, stdin=None):
    env = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C",
           "TZ": "UTC", "TMPDIR": str(root / "tmp"), "DPKG_COLORS": "never"}
    pass_fds = () if status is None else (status.fileno(),)
    with output.open("xb") as stdout, error.open("xb") as stderr:
        completed = subprocess.run(
            argv, input=stdin, stdout=stdout, stderr=stderr, env=env,
            pass_fds=pass_fds, timeout=120, check=False, preexec_fn=_limit_child_output)
    require(output.stat().st_size <= LIMIT_TAR and error.stat().st_size <= LIMIT_METADATA,
            "tool output limit")
    return completed.returncode


def package_container(data):
    """Require the fixed three-member Debian 2.0 profile before decoding data."""
    require(0 < len(data) <= LIMIT_PACKAGE and data.startswith(b"!<arch>\n"),
            "not a bounded Debian ar archive")
    offset, names = 8, []
    while offset < len(data):
        require(offset + 60 <= len(data), "truncated ar header")
        header = data[offset:offset + 60]
        require(header[58:60] == b"\x60\n", "malformed ar header")
        raw_name = header[:16].decode("ascii", "strict").rstrip(" ")
        name = raw_name[:-1] if raw_name.endswith("/") else raw_name
        require(name not in names and len(names) < 3, "duplicate or extra ar member")
        size_text = header[48:58].decode("ascii", "strict").strip(" ")
        require(re.fullmatch(r"[0-9]{1,10}", size_text), "malformed ar size")
        size = int(size_text)
        start, end = offset + 60, offset + 60 + size
        require(size > 0 and end <= len(data), "truncated or empty ar member")
        if not names:
            require(name == "debian-binary" and data[start:end] == b"2.0\n",
                    "unsupported Debian archive profile")
        elif len(names) == 1:
            require(name in {"control.tar", "control.tar.gz", "control.tar.xz", "control.tar.zst"},
                    "control member absent or out of order")
        else:
            require(name in {"data.tar", "data.tar.gz", "data.tar.xz", "data.tar.zst"},
                    "data member absent or out of order")
        names.append(name)
        if size % 2:
            require(data[end:end + 1] == b"\n", "invalid ar padding")
        offset = end + size % 2
    require(offset == len(data) and len(names) == 3, "incomplete Debian archive")
    return names


def extract_regular_member(tar_data, installed_path):
    require(0 < len(tar_data) <= LIMIT_TAR, "tar size limit")
    target = installed_path.removeprefix("/")
    require(installed_path in {p for _, p in PACKAGE_MEMBERS.values()}, "member not allowlisted")
    seen, result, total, last_end = set(), None, 0, 0
    alias = PACKAGE_ALIASES.get(installed_path)
    alias_seen = False
    parents = set(PurePosixPath(target).parents)
    if alias is not None:
        parents.update(PurePosixPath(alias["archive_path"]).parents)
    try:
        with tarfile.open(fileobj=io.BytesIO(tar_data), mode="r:") as archive:
            for member in archive:
                last_end = member.offset_data + ((member.size + 511) // 512) * 512
                name = member.name.rstrip("/") if member.isdir() else member.name
                if name.startswith("./"):
                    name = name[2:]
                if not name and member.isdir():
                    name = "."
                require(name not in seen, "duplicate archive member")
                seen.add(name)
                require(len(seen) <= 50000, "archive member limit")
                parts = name.split("/")
                require(not name.startswith("/") and "\\" not in name and
                        not any(ord(c) < 32 or ord(c) == 127 for c in name) and
                        all(p not in ("", "..") for p in parts) and
                        (name == "." or "." not in parts), "unsafe archive member path")
                require(member.size >= 0, "negative archive member size")
                total += member.size
                require(total <= LIMIT_TAR, "expanded archive size limit")
                if PurePosixPath(name) in parents:
                    require(member.isdir(), "archive target parent is not a directory")
                if alias is not None and name == alias["archive_path"]:
                    require(member.type == tarfile.SYMTYPE and member.size == 0 and
                            not member.sparse and member.linkname == alias["linkname"] and
                            (PurePosixPath(name).parent / member.linkname).as_posix() ==
                            alias["target_path"], "package executable alias differs")
                    alias_seen = True
                    continue
                if name != target:
                    continue
                require(member.type in (tarfile.REGTYPE, tarfile.AREGTYPE) and
                        not member.sparse and 0 < member.size <= LIMIT_MEMBER,
                        "target is not one bounded ordinary file")
                handle = archive.extractfile(member)
                require(handle is not None, "target data absent")
                with handle:
                    result = handle.read(LIMIT_MEMBER + 1)
                require(len(result) == member.size, "target size differs")
    except (tarfile.TarError, EOFError, OSError) as error:
        raise IntakeError("invalid package data archive") from error
    require(len(tar_data) % 512 == 0 and len(tar_data) - last_end >= 1024 and
            not any(tar_data[last_end:]), "truncated, concatenated or trailing tar data")
    require(result is not None, "expected package member absent")
    require(alias is None or alias_seen, "expected package executable alias absent")
    require(len(result) >= 64 and result[:7] == b"\x7fELF\x02\x01\x01" and
            int.from_bytes(result[18:20], "little") == 62 and
            int.from_bytes(result[16:18], "little") in (2, 3) and
            int.from_bytes(result[52:54], "little") == 64, "target is not an amd64 ELF")
    return result


def build_receipt(context, release, signatures, files, packages, members, keyring_observation):
    require(len(signatures) == 1 and
            signatures[0]["fingerprint"] == signatures[0]["primary_fingerprint"] ==
            ARCHIVE_SIGNING_FINGERPRINT, "receipt lacks the authorized signer")
    require(files["keyring"]["sha256"] == keyring_observation["sha256"] and
            files["keyring"]["size"] == keyring_observation["size"],
            "receipt keyring snapshot differs")
    require(set(packages) == set(members) == set(PACKAGE_MEMBERS), "incomplete runtime custody")
    bindings = {}
    for name, (binding, installed_path) in PACKAGE_MEMBERS.items():
        row = members[name]
        require(row["installed_path"] == installed_path and HEX64.fullmatch(row["sha256"]),
                "runtime binding differs")
        require(row["package_sha256"] == packages[name]["sha256"], "member package binding differs")
        require(row.get("archive_alias") == PACKAGE_ALIASES.get(installed_path),
                "member archive alias binding differs")
        bindings[binding] = {"installed_path": installed_path, "sha256": row["sha256"]}
    return {"schema": SCHEMA, "scope": "signed-distribution-files-only",
            "source_authentication": "PASS", "context": context,
            "distribution": {"origin": "Ubuntu", "version": "24.04", "suite": "noble",
                             "architecture": "amd64", "component": "universe",
                             "upstream_version": "7.22.0", "package_version": VERSION},
            "release": release, "signatures": signatures,
            "trust_anchor": {"kind": "source-pinned-ubuntu-archive-signing-key",
                             "signing_fingerprint": ARCHIVE_SIGNING_FINGERPRINT,
                             "primary_fingerprint": ARCHIVE_SIGNING_FINGERPRINT,
                             "authority_reference": SIGNING_AUTHORITY_URL,
                             "key_material_role": "observed-candidate-keyring",
                             "original_path": str(KEYRING),
                             "source_observation": keyring_observation,
                             "keyring": files["keyring"],
                             "fresh_key_revocation_check": "NOT_ESTABLISHED_BY_GPGV"},
            "chain_files": files, "packages": packages, "members": members,
            "runtime_bindings": bindings,
            "observer_source": "NOT_RUN", "repaired_only_compile": "NOT_RUN",
            "package_installation": "NOT_RUN", "native_execution": "NOT_RUN",
            "loaded_module_observation": "NOT_RUN", "physical_AAC_master": "NOT_RUN",
            "human_acceptance": False, "release_authorized": False}


def collect(root, context):
    now = dt.datetime.now(dt.timezone.utc)
    require(platform.system() == "Linux" and platform.machine() == "x86_64",
            "intake requires an amd64 Linux runner")
    os_release = read_regular("/usr/lib/os-release", LIMIT_METADATA).decode("utf-8")
    for expected in ('ID=ubuntu', 'VERSION_ID="24.04"'):
        require(expected in os_release.splitlines(), "runner is not Ubuntu 24.04")
    for name in ("tmp", "gnupg", "raw", "packages", "members", "commands"):
        (root / name).mkdir(mode=0o700)
    files, packages, members = {}, {}, {}
    tracked = {}

    def remember(label, path, limit=LIMIT_METADATA):
        files[label] = descriptor(path, root, limit)
        tracked[path] = (files[label], limit)
        return files[label]

    keyring, keyring_observation = capture_regular(KEYRING, LIMIT_METADATA)
    keyring_pin = {key: keyring_observation[key] for key in ("sha256", "size")}
    # The snapshot is evidence supplied to gpgv, not a new trust anchor. Its
    # ownership or mode cannot authorize any signer; the full pin below does.
    exclusive(root / "raw/keyring-source-observation.json", json_bytes(keyring_observation))
    remember("keyring_source_observation", root / "raw/keyring-source-observation.json")
    key_copy = root / "raw/ubuntu-archive-keyring.gpg"
    exclusive(key_copy, keyring)
    remember("keyring", key_copy)
    verify_bytes(read_regular(key_copy, LIMIT_METADATA), keyring_pin)
    for name in ("gpgv", "dpkg-deb"):
        tool = Path("/usr/bin") / name
        data = read_regular(tool, LIMIT_MEMBER)
        files[name + "_tool"] = {"system_path": str(tool), "sha256": digest(data), "size": len(data)}
    exclusive(root / "raw/os-release", os_release.encode("utf-8"))
    remember("runner_os_release", root / "raw/os-release")
    inrelease = root / "raw/InRelease"
    download(RELEASE, inrelease, LIMIT_METADATA)
    remember("inrelease", inrelease)
    signed_payload = root / "raw/Release.verified"
    status_path, log_path = root / "raw/gpgv.status", root / "raw/gpgv.stderr"
    with status_path.open("xb") as status_handle:
        command = ["/usr/bin/gpgv", "--homedir", str(root / "gnupg"),
                   "--keyring", str(key_copy), "--status-fd", str(status_handle.fileno()),
                   "--weak-digest", "SHA1", "--output", "-", str(inrelease)]
        exclusive(root / "commands/gpgv.json", json_bytes({"argv": command}))
        code = run_tool(command, signed_payload, log_path, root, status=status_handle)
    exclusive(root / "commands/gpgv-exit.json", json_bytes({"exit_code": code}))
    verify_bytes(read_regular(key_copy, LIMIT_METADATA), keyring_pin)
    signatures = parse_signature_status(read_regular(status_path, LIMIT_METADATA), code, now)
    remember("signature_status", status_path)
    remember("signature_log", log_path)
    remember("signature_command", root / "commands/gpgv.json")
    remember("signature_exit", root / "commands/gpgv-exit.json")
    payload = read_regular(signed_payload, LIMIT_METADATA)
    release = parse_release(payload, now)
    remember("verified_release_payload", signed_payload)
    index_file = root / "raw/Packages.xz"
    download("dists/noble/" + INDEX + ".xz", index_file,
             release["compressed"]["size"], release["compressed"])
    index_bytes = expand_index(read_regular(index_file, LIMIT_COMPRESSED), release)
    exclusive(root / "raw/Packages", index_bytes)
    remember("packages_xz", index_file, LIMIT_COMPRESSED)
    remember("packages_index", root / "raw/Packages", LIMIT_INDEX)
    selected = select_packages(index_bytes)
    for name, selection in sorted(selected.items()):
        package = root / "packages" / Path(selection["filename"]).name
        pin = {key: selection[key] for key in ("sha256", "size")}
        download(selection["filename"], package, selection["size"], pin)
        package_data = read_regular(package, LIMIT_PACKAGE)
        verify_bytes(package_data, pin)
        container = package_container(package_data)
        packages[name] = {**selection, "file": package.relative_to(root).as_posix(),
                          "archive_members": container}
        tracked[package] = (pin, LIMIT_PACKAGE)
        data_tar, error = root / "raw" / (name + ".data.tar"), root / "raw" / (name + ".dpkg.stderr")
        command = ["/usr/bin/dpkg-deb", "--fsys-tarfile", "-"]
        exclusive(root / "commands" / (name + "-extract.json"),
                  json_bytes({"argv": command, "stdin": packages[name]}))
        code = run_tool(command, data_tar, error, root, stdin=package_data)
        exclusive(root / "commands" / (name + "-extract-exit.json"),
                  json_bytes({"exit_code": code}))
        require(code == 0, "dpkg-deb data decoding failed")
        _, installed_path = PACKAGE_MEMBERS[name]
        member = extract_regular_member(read_regular(data_tar, LIMIT_TAR), installed_path)
        destination = root / "members" / Path(installed_path).name
        exclusive(destination, member)
        members[name] = {**descriptor(destination, root, LIMIT_MEMBER),
                         "installed_path": installed_path, "package_sha256": selection["sha256"],
                         "archive_alias": PACKAGE_ALIASES.get(installed_path),
                         "member_type": "regular", "architecture_header": "ELF64-LSB-x86_64"}
        tracked[destination] = (members[name], LIMIT_MEMBER)
        remember(name + "_data_tar", data_tar, LIMIT_TAR)
        remember(name + "_extract_stderr", error)
        remember(name + "_extract_command", root / "commands" / (name + "-extract.json"))
        remember(name + "_extract_exit", root / "commands" / (name + "-extract-exit.json"))
    for path, (pin, limit) in tracked.items():
        verify_bytes(read_regular(path, limit), {key: pin[key] for key in ("sha256", "size")})
    return build_receipt(context, release, signatures, files, packages, members, keyring_observation)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--source-tree", required=True)
    args = parser.parse_args(argv)
    require(re.fullmatch(r"[0-9a-f]{40}", args.source_sha), "invalid source SHA")
    require(re.fullmatch(r"[0-9a-f]{40}", args.source_tree), "invalid source tree")
    require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("RUNNER_OS") == "Linux",
            "intake is restricted to the selected CI source job")
    for name in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        require(re.fullmatch(r"[1-9][0-9]*", os.environ.get(name, "")), "invalid run identity")
    require(os.environ.get("GITHUB_SHA") == args.source_sha, "workflow source differs")
    root = args.output
    require(root.is_absolute() and root.parent.is_dir() and
            root.parent.resolve(strict=True) == root.parent, "output parent is not canonical")
    root.mkdir(mode=0o700, exist_ok=False)
    context = {"source_sha": args.source_sha, "source_tree": args.source_tree,
               "run_id": os.environ["GITHUB_RUN_ID"], "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
               "created_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
               "helper_sha256": digest(read_regular(__file__, LIMIT_METADATA))}
    exclusive(root / "intent.json", json_bytes({"schema": SCHEMA, "scope": "source-only",
                                               "context": context, "automatic_retry": False}))
    try:
        receipt = collect(root, context)
        exclusive(root / "official-runtime-receipt.json", json_bytes(receipt))
    except Exception as error:
        exclusive(root / "attempt.json", json_bytes(
            {"status": "FAILED_SOURCE_INTAKE", "error_type": type(error).__name__,
             "error": str(error), "native_execution": "NOT_RUN",
             "package_installation": "NOT_RUN", "physical_AAC_master": "NOT_RUN"}))
        raise
    exclusive(root / "attempt.json", json_bytes(
        {"status": "SOURCE_CHAIN_VERIFIED", "native_execution": "NOT_RUN",
         "package_installation": "NOT_RUN", "physical_AAC_master": "NOT_RUN"}))
    print(json.dumps({"status": "SOURCE_CHAIN_VERIFIED",
                      "runtime_bindings": receipt["runtime_bindings"],
                      "native_execution": "NOT_RUN", "physical_AAC_master": "NOT_RUN"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
