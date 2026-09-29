"""Bounded evidence-archive reader. Never extract entries onto a filesystem."""
from __future__ import annotations
import io
import re
import stat
import zipfile
from lab_core import EvidenceError, LANES, digest

MAX_ARCHIVE = 4 * 1024 * 1024
MAX_FILE = 1024 * 1024

def read_evidence_archive(blob: bytes, lane: str, expected_digest: str) -> dict[str, bytes]:
    if lane not in LANES:
        raise EvidenceError("unknown artifact lane")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest) or digest(blob) != expected_digest:
        raise EvidenceError("artifact digest mismatch")
    if len(blob) > MAX_ARCHIVE:
        raise EvidenceError("archive byte budget")
    allowed = {lane + ".json", "sandbox-setup.log"}
    result = {}
    try:
        with zipfile.ZipFile(io.BytesIO(blob)) as archive:
            entries = archive.infolist()
            if not 1 <= len(entries) <= len(allowed):
                raise EvidenceError("archive entry budget")
            if sum(i.file_size for i in entries) > 2 * MAX_FILE:
                raise EvidenceError("archive expansion budget")
            for info in entries:
                mode = info.external_attr >> 16
                name = info.filename
                if name not in allowed or name in result or info.is_dir():
                    raise EvidenceError("unexpected/duplicate/path-like archive member")
                if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in {0, stat.S_IFREG}):
                    raise EvidenceError("nonregular archive member")
                if info.flag_bits & 1 or info.file_size > MAX_FILE:
                    raise EvidenceError("encrypted or oversized archive member")
                with archive.open(info) as stream:
                    data = stream.read(MAX_FILE + 1)
                if len(data) != info.file_size or len(data) > MAX_FILE:
                    raise EvidenceError("archive member length mismatch")
                result[name] = data
    except (zipfile.BadZipFile, RuntimeError, NotImplementedError, OSError) as exc:
        raise EvidenceError("invalid evidence archive") from exc
    if lane + ".json" not in result:
        raise EvidenceError("missing structured lane receipt")
    return result
