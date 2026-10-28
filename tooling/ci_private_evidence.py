#!/usr/bin/env python3
"""Bounded private original-evidence snapshots and authenticated sealing.

The runner receives only an admitted RSA public key. Selected regular files are
captured without writing their contents and encrypted once with a fresh AES-256-GCM key.
A closed authenticated header binds the ciphertext to its exact source and run.
No native operation is started or retried by this module.
"""
from __future__ import annotations

import base64
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import tarfile

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from mlt_official_runtime_intake import capture_regular

SEAL_SCHEMA = "motionwright.ci.private-evidence-seal/1"
MAGIC = b"MWCISEAL1\n"
MAX_FILE = 256 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
MAX_FILES = 4096
MAX_JSON = 512 * 1024
HEX = re.compile(r"[0-9a-f]{64}\Z")
SOURCE = re.compile(r"[0-9a-f]{40}\Z")
CONTEXT_KEYS = {"source_sha", "semwright_sha", "run_id", "run_attempt"}


class CustodyRejected(ValueError):
    pass


def require(ok, code):
    if not ok:
        raise CustodyRejected(code)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def strict(raw):
    require(0 < len(raw) <= MAX_JSON, "json_size")
    def pairs(values):
        result = {}
        for key, value in values:
            require(key not in result, "duplicate_json_key")
            result[key] = value
        return result
    result = json.loads(raw, object_pairs_hook=pairs,
                        parse_constant=lambda _: (_ for _ in ()).throw(CustodyRejected("json_constant")))
    require(type(result) is dict, "json_object_required")
    return result


def regular(path, limit=MAX_FILE):
    path = Path(path)
    require(path.is_absolute() and path.resolve(strict=True) == path
            and not any(p.is_symlink() for p in path.parents), "noncanonical_input")
    return capture_regular(path, limit)


def new_bytes(path, data, *, owner=None, mode=0o600):
    path = Path(path)
    require(path.parent.is_dir() and path.parent.resolve(strict=True) == path.parent,
            "output_parent_not_canonical")
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), mode)
    try:
        with os.fdopen(fd, "wb", closefd=False) as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if owner is not None:
            os.fchown(fd, *owner)
        require(regular(path)[0] == data, "exclusive_write_readback")
    finally:
        os.close(fd)


def new_json(path, value, **kwargs):
    new_bytes(path, json_bytes(value), **kwargs)


def public_key(recipient):
    require(type(recipient) is dict and set(recipient) == {"spki_der_base64", "sha256"}
            and HEX.fullmatch(recipient["sha256"]) is not None, "recipient_contract")
    encoded = recipient["spki_der_base64"]
    require(type(encoded) is str and len(encoded) <= 2048, "recipient_size")
    der = base64.b64decode(encoded, validate=True)
    require(base64.b64encode(der).decode() == encoded and sha(der) == recipient["sha256"],
            "recipient_fingerprint")
    key = serialization.load_der_public_key(der)
    require(isinstance(key, rsa.RSAPublicKey) and key.key_size == 3072
            and key.public_numbers().e == 65537, "recipient_must_be_RSA_3072_65537")
    require(key.public_bytes(serialization.Encoding.DER,
                             serialization.PublicFormat.SubjectPublicKeyInfo) == der,
            "recipient_not_canonical_SPKI")
    return key


def metadata_identity(info):
    """Exclude access time, which a read may legitimately change."""
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def snapshot_member(value):
    require(type(value) is str and 0 < len(value) <= 4096
            and "\\" not in value and not value.startswith("/")
            and all(p not in ("", ".", "..") for p in value.split("/"))
            and all(ord(c) >= 32 and ord(c) != 127 for c in value), "snapshot_member_path")
    return value


def gather_snapshot(roots, *, preserve_partial=False):
    """Capture originals once; failed observations are retained, never retried.

    The default strict mode is useful for gates. Sealing uses preserve_partial
    to recover every independently readable original after an unknown outcome.
    A partial capture cannot establish the scoped verification result.
    """
    listing, entries, total, complete = {}, [], 0, True
    def failed(error):
        return {"error_type": type(error).__name__, "error": str(error),
                "automatic_retry": False, "original_retained_in_place": True}
    for label, root in sorted(roots.items()):
        require(re.fullmatch(r"[a-z][a-z-]*", label) is not None, "snapshot_label_invalid")
        root = Path(root)
        captured = []
        try:
            if not root.exists():
                require(preserve_partial, "snapshot_root_absent")
                listing[label] = {"status": "ABSENT", "automatic_retry": False}
                complete = False
                continue
            require(root.is_dir() and root.resolve(strict=True) == root
                    and not any(p.is_symlink() for p in root.parents), "snapshot_root_invalid")
            root_identity = metadata_identity(root.lstat())
            paths = sorted(root.rglob("*"))
            require(len(paths) <= MAX_FILES * 4, "snapshot_namespace_budget")
            inventory = [p.relative_to(root).as_posix() for p in paths]
            for path in paths:
                name = label + "/" + path.relative_to(root).as_posix()
                try:
                    snapshot_member(name)
                    observed = path.lstat()
                    if not stat.S_ISREG(observed.st_mode) and not stat.S_ISDIR(observed.st_mode):
                        require(label == "owner", "snapshot_nonregular_input")
                        target = os.readlink(path) if stat.S_ISLNK(observed.st_mode) else None
                        require(metadata_identity(path.lstat()) == metadata_identity(observed),
                                "snapshot_nonregular_changed")
                        captured.append({"archive_path": name, "kind": "metadata-only-nonregular",
                                         "mode": observed.st_mode, "device": observed.st_dev,
                                         "inode": observed.st_ino, "uid": observed.st_uid,
                                         "gid": observed.st_gid, "link_target": target,
                                         "followed_or_opened": False})
                        continue
                    if stat.S_ISDIR(observed.st_mode):
                        continue
                    require(len(entries) < MAX_FILES, "snapshot_file_count")
                    data, observation = regular(path, MAX_FILE)
                    require(total + len(data) <= MAX_TOTAL, "snapshot_total_size")
                    total += len(data)
                    entries.append((name, data))
                    captured.append({"archive_path": name, "observation": observation})
                except Exception as error:
                    if not preserve_partial:
                        raise
                    complete = False
                    captured.append({"archive_path": name, "status": "CAPTURE_FAILED",
                                     **failed(error)})
            require(inventory == [p.relative_to(root).as_posix() for p in sorted(root.rglob("*"))]
                    and metadata_identity(root.lstat()) == root_identity,
                    "snapshot_inventory_changed")
            listing[label] = {"status": "CAPTURED", "files": captured}
        except Exception as error:
            if not preserve_partial:
                raise
            complete = False
            listing[label] = {"status": "PARTIAL", "files": captured, **failed(error)}
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w") as archive:
        for name, data in entries + [("snapshot-manifest.json", json_bytes({
                "schema": "motionwright.ci.private-snapshot/1",
                "scope": "selected-original-files-at-read-time", "roots": listing,
                "selected_originals_complete": complete,
                "complete_owner_state_claimed": False}))]:
            member = tarfile.TarInfo(name)
            member.mode, member.mtime = 0o600, 0
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
    raw = stream.getvalue()
    require(len(raw) <= MAX_TOTAL + MAX_FILES * 4096, "snapshot_archive_size")
    return (raw, complete) if preserve_partial else raw


def validate_context(current):
    require(type(current) is dict and set(current) == CONTEXT_KEYS, "seal_context")
    require(all(type(current[key]) is str and SOURCE.fullmatch(current[key]) is not None
                for key in ("source_sha", "semwright_sha")), "seal_source_identity")
    require(all(type(current[key]) is str and re.fullmatch(r"[1-9][0-9]*", current[key]) is not None
                for key in ("run_id", "run_attempt")), "seal_run_identity")
    return current


def encrypt_snapshot(plaintext, recipient, current):
    validate_context(current)
    require(0 < len(plaintext) <= MAX_TOTAL + MAX_FILES * 4096, "seal_plaintext_size")
    key = public_key(recipient)
    secret = AESGCM.generate_key(bit_length=256)
    nonce = os.urandom(12)
    wrapped = key.encrypt(secret, padding.OAEP(
        mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=MAGIC))
    header = {"schema": SEAL_SCHEMA, "algorithm": "RSA-OAEP-SHA256+AES-256-GCM",
              "recipient_spki_sha256": recipient["sha256"], "context": current,
              "nonce": base64.b64encode(nonce).decode(),
              "wrapped_key": base64.b64encode(wrapped).decode(),
              "plaintext_sha256": sha(plaintext), "plaintext_bytes": len(plaintext)}
    aad = json_bytes(header)
    ciphertext = AESGCM(secret).encrypt(nonce, plaintext, aad)
    return MAGIC + len(aad).to_bytes(4, "big") + aad + ciphertext


def decrypt_snapshot(envelope, private_key):
    """For independent offline recovery; the runner never receives this key."""
    require(type(envelope) is bytes and envelope.startswith(MAGIC)
            and len(MAGIC) + 4 < len(envelope) <= MAX_TOTAL + MAX_FILES * 4096 + 8192 + 64,
            "seal_magic_or_envelope_size")
    offset = len(MAGIC)
    length = int.from_bytes(envelope[offset:offset + 4], "big")
    require(0 < length <= 8192, "seal_header_size")
    aad = envelope[offset + 4:offset + 4 + length]
    header = strict(aad)
    require(set(header) == {"schema", "algorithm", "recipient_spki_sha256", "context",
                            "nonce", "wrapped_key", "plaintext_sha256", "plaintext_bytes"}
            and header["schema"] == SEAL_SCHEMA
            and header["algorithm"] == "RSA-OAEP-SHA256+AES-256-GCM"
            and json_bytes(header) == aad, "seal_header_contract")
    validate_context(header["context"])
    require(type(header["plaintext_bytes"]) is int
            and 0 < header["plaintext_bytes"] <= MAX_TOTAL + MAX_FILES * 4096
            and type(header["plaintext_sha256"]) is str
            and HEX.fullmatch(header["plaintext_sha256"]) is not None, "seal_plaintext_contract")
    require(isinstance(private_key, rsa.RSAPrivateKey) and private_key.key_size == 3072
            and private_key.public_key().public_numbers().e == 65537,
            "recovery_key_invalid")
    der = private_key.public_key().public_bytes(serialization.Encoding.DER,
                                               serialization.PublicFormat.SubjectPublicKeyInfo)
    require(sha(der) == header["recipient_spki_sha256"], "recovery_key_differs")
    secret = private_key.decrypt(base64.b64decode(header["wrapped_key"], validate=True),
                                 padding.OAEP(mgf=padding.MGF1(hashes.SHA256()),
                                              algorithm=hashes.SHA256(), label=MAGIC))
    nonce = base64.b64decode(header["nonce"], validate=True)
    require(len(secret) == 32 and len(nonce) == 12, "seal_key_or_nonce_size")
    plaintext = AESGCM(secret).decrypt(nonce, envelope[offset + 4 + length:], aad)
    require(len(plaintext) == header["plaintext_bytes"] and sha(plaintext) == header["plaintext_sha256"],
            "seal_plaintext_binding")
    return plaintext, header["context"]
