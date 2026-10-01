"""Synthetic evidence/ZIP hostile controls; only called by hosted selftest."""
from __future__ import annotations
import copy
import io
import stat
import tempfile
from pathlib import Path
import warnings
import zipfile
from artifact_io import MAX_FILE, read_evidence_archive
from collect_evidence import validate_lane, immutable_write, target_for_lane
from product import build_target_kind
from isolation import SYSTEM_CONFIG_RO, DEFAULT_FILE_SIZE_BYTES, MAX_FILE_SIZE_BYTES
from lab_core import EvidenceError, digest, summarize, strict_json
from oracle_identity import payload_digest
from selftest_extra import target_only_retest, closure_rejects

SOURCE = "1" * 40
SUITE = "2" * 40

def rejects(call):
    try:
        call()
    except EvidenceError:
        return True
    return False

def archive(entries):
    buffer = io.BytesIO()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as z:
            for name, value, mode in entries:
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = mode << 16
                z.writestr(info, value)
    return buffer.getvalue()

def rejected_archive(entries):
    data = archive(entries)
    return rejects(lambda: read_evidence_archive(data, "selftest", digest(data)))

def healthy_archive(two=False):
    entries = [("selftest.json", b"{}", stat.S_IFREG | 0o600)]
    if two:
        entries.append(("sandbox-setup.log", b"synthetic provisioning", stat.S_IFREG | 0o600))
    data = archive(entries)
    result = read_evidence_archive(data, "selftest", digest(data))
    return result.get("selftest.json") == b"{}" and len(result) == len(entries)

def example():
    cases = [{"id": "G-PLAN-001", "scope": "product_contract"}]
    lock = {"targets": {"A": SOURCE, "main": SOURCE}, "contract_sha": SOURCE}
    run = {"id": 17, "run_attempt": 1}
    rows = [{"case_id": "G-PLAN-001", "source_sha": SOURCE, "suite_sha": SUITE,
             "scope": "product_contract", "outcome": "PASS", "isolation_verified": True}]
    report = {"schema_version": 1, "role": "G", "lane": "composition", "source_sha": SOURCE,
              "suite_sha": SUITE, "github_sha": SUITE, "run_id": "17", "run_attempt": "1",
              "requested_cases": ["G-PLAN-001"], "skipped_cases": [], "contract_sha": SOURCE,
              "dependency_shas": lock["targets"], "product_target_sha": SOURCE,
              "results": rows, "cleanup_verified": True, "infrastructure_blockers": [],
              "summary": summarize(["G-PLAN-001"], rows, SOURCE, SUITE, scope="product_contract")}
    return report, cases, lock, run

def evidence_rejected(change):
    report, cases, lock, run = copy.deepcopy(example())
    change(report)
    return rejects(lambda: validate_lane(report, "composition", cases, lock, SUITE, run))

def evidence_positive():
    report, cases, lock, run = example()
    return validate_lane(report, "composition", cases, lock, SUITE, run)["status"] == "NO_OPEN_BLOCKING_FINDINGS_IN_TESTED_SCOPE"

def infrastructure_blocked():
    report, cases, lock, run = example()
    report["infrastructure_blockers"] = ["synthetic cleanup blocker"]
    report["summary"]["status"] = "BLOCKED"
    return validate_lane(report, "composition", cases, lock, SUITE, run)["status"] == "BLOCKED"

def rejected_oracle_report():
    report, cases, lock, run = example()
    report["oracle_tree_sha256"] = "c" * 64
    return rejects(lambda: validate_lane(report, "composition", cases, lock, SUITE, run, expected_oracle="b" * 64))

def identity_control(change, equal):
    files = {"probe.rs": b"synthetic-probe", "registry.json": b"synthetic-expectations"}
    lock = {"schema_version": 1, "targets": {"A": SOURCE}, "pins": {"runtime": "synthetic"},
            "limits": {"operations": 4}, "contract_sha": SOURCE, "selected_lanes": ["composition"]}
    before = payload_digest(files, lock)
    change(files, lock)
    return (payload_digest(files, lock) == before) is equal


def collector_role_targets():
    lock = {"targets": {"main": "0" * 40, "A": "a" * 40, "B": "b" * 40, "C": "c" * 40, "D": "d" * 40, "E": "e" * 40, "F": "f" * 40}}
    return (
        target_for_lane(lock, "selftest", SUITE) == SUITE
        and target_for_lane(lock, "graph", SUITE) == "c" * 40
        and target_for_lane(lock, "effects", SUITE) == "f" * 40
        and target_for_lane(lock, "routing", SUITE) == "c" * 40
        and target_for_lane(lock, "godot-native", SUITE) == "d" * 40
        and target_for_lane(lock, "blender-native", SUITE) == "e" * 40
        and target_for_lane(lock, "packaging", SUITE) == "0" * 40
    )


def routing_build_target_is_normal_bin():
    return build_target_kind("routing") == "bin" and build_target_kind("graph") == "example"


def native_summary_is_scope_bound():
    rows = [{"case_id":"G-GODOT-001","source_sha":SOURCE,"suite_sha":SUITE,
             "scope":"native_application","outcome":"PASS","isolation_verified":True}]
    native = summarize(["G-GODOT-001"], rows, SOURCE, SUITE, scope="native_application")
    contract_rows = [dict(rows[0], scope="product_contract")]
    contract = summarize(["G-GODOT-001"], contract_rows, SOURCE, SUITE, scope="product_contract")
    return native["native_acceptance"] is True and contract["native_acceptance"] is False

def native_system_config_is_minimal():
    return SYSTEM_CONFIG_RO == ("/etc/fonts", "/etc/xdg")

def native_address_space_budget_is_bounded():
    lock = strict_json((Path(__file__).resolve().parent / "targets.json").read_bytes())
    limits = lock["limits"]
    return limits["address_space_bytes"] == 1024 * 1024 * 1024 and limits["native_address_space_bytes"] == 4 * 1024 * 1024 * 1024

def secondary_runtime_mount_is_scoped():
    text = (Path(__file__).resolve().parent / "isolation.py").read_text()
    profile = (Path(__file__).resolve().parents[2] / "scripts/dev/ci-driver-bwrap-profile.sh").read_text()
    return (
        '"/plugin/tools"' in text
        and '"--ro-bind", str(runtime.resolve()), "/plugin/tools/godot"' in text
        and 'runtime: Path | None = None' in text
        and "allow ix /plugin/tools/**," in profile
        and "/plugin/runtime" not in text
    )


def native_file_size_budget_is_bounded():
    lock = strict_json((Path(__file__).resolve().parent / "targets.json").read_bytes())
    limits = lock["limits"]
    return (
        DEFAULT_FILE_SIZE_BYTES == 8 * 1024 * 1024
        and MAX_FILE_SIZE_BYTES == 256 * 1024 * 1024
        and limits["file_size_bytes"] == DEFAULT_FILE_SIZE_BYTES
        and limits["native_file_size_bytes"] == MAX_FILE_SIZE_BYTES
    )

def history_control(overwrite):
    with tempfile.TemporaryDirectory(prefix="g-synthetic-evidence-", dir="/out") as directory:
        path = Path(directory) / "receipt.json"
        immutable_write(path, b"synthetic-before-FAIL")
        if overwrite:
            return rejects(lambda: immutable_write(path, b"synthetic-after-PASS")) and path.read_bytes() == b"synthetic-before-FAIL"
        immutable_write(path, b"synthetic-before-FAIL")
        return path.read_bytes() == b"synthetic-before-FAIL"

def evidence_cases():
    regular = stat.S_IFREG | 0o600
    return [
        ("G-SELF-055", healthy_archive),
        ("G-SELF-056", lambda: rejects(lambda: read_evidence_archive(b"synthetic", "selftest", "a" * 64))),
        ("G-SELF-057", lambda: rejected_archive([("../selftest.json", b"{}", regular)])),
        ("G-SELF-058", lambda: rejected_archive([("selftest.json", b"synthetic-target", stat.S_IFLNK | 0o777)])),
        ("G-SELF-059", lambda: rejected_archive([("selftest.json", b"{}", regular), ("selftest.json", b"{}", regular)])),
        ("G-SELF-060", lambda: rejected_archive([("sandbox-setup.log", b"not a receipt", regular)])),
        ("G-SELF-061", lambda: rejected_archive([("arbitrary.bin", b"{}", regular)])),
        ("G-SELF-062", lambda: rejects(lambda: read_evidence_archive(b"not-a-zip", "selftest", digest(b"not-a-zip")))),
        ("G-SELF-063", lambda: rejected_archive([("selftest.json", b"x" * (MAX_FILE + 1), regular)])),
        ("G-SELF-064", lambda: healthy_archive(two=True)),
        ("G-SELF-065", lambda: rejected_archive([("/selftest.json", b"{}", regular)])),
        ("G-SELF-066", lambda: rejected_archive([("selftest.json/", b"", stat.S_IFDIR | 0o700)])),
        ("G-SELF-067", evidence_positive),
        ("G-SELF-068", lambda: evidence_rejected(lambda r: r.update(source_sha="3" * 40))),
        ("G-SELF-069", lambda: evidence_rejected(lambda r: r.update(suite_sha="3" * 40))),
        ("G-SELF-070", lambda: evidence_rejected(lambda r: r.update(run_attempt="2"))),
        ("G-SELF-071", lambda: evidence_rejected(lambda r: r.update(requested_cases=[]))),
        ("G-SELF-072", lambda: evidence_rejected(lambda r: r["summary"].update(executed_count=0))),
        ("G-SELF-073", lambda: evidence_rejected(lambda r: r["summary"].update(native_acceptance=True))),
        ("G-SELF-074", lambda: evidence_rejected(lambda r: r.update(product_target_sha=None))),
        ("G-SELF-075", lambda: evidence_rejected(lambda r: r.update(contract_sha="3" * 40))),
        ("G-SELF-076", lambda: evidence_rejected(lambda r: r.update(dependency_shas={}))),
        ("G-SELF-077", lambda: evidence_rejected(lambda r: r["results"][0].update(outcome="FAIL"))),
        ("G-SELF-078", lambda: evidence_rejected(lambda r: r.update(results=[]))),
        ("G-SELF-079", infrastructure_blocked),
        ("G-SELF-080", lambda: evidence_rejected(lambda r: r.update(skipped_cases=["G-PLAN-001"]))),
        ("G-SELF-081", lambda: evidence_rejected(lambda r: r["summary"].update(executed_count=True))),
        ("G-SELF-082", lambda: evidence_rejected(lambda r: r["summary"].update(r16_closed=0))),
        ("G-SELF-083", target_only_retest),
        ("G-SELF-084", lambda: closure_rejects(lambda b, a: a.update(oracle_tree_sha256="c" * 64))),
        ("G-SELF-085", rejected_oracle_report),
        ("G-SELF-086", lambda: closure_rejects(lambda b, a: a.pop("oracle_tree_sha256"))),
        ("G-SELF-087", lambda: identity_control(lambda f, c: c["targets"].update(A="3" * 40), True)),
        ("G-SELF-088", lambda: identity_control(lambda f, c: c["limits"].update(operations=99), False)),
        ("G-SELF-089", lambda: identity_control(lambda f, c: f.update({"probe.rs": b"modified guard"}), False)),
        ("G-SELF-090", lambda: identity_control(lambda f, c: f.update({"registry.json": b"relaxed expectations"}), False)),
        ("G-SELF-091", lambda: history_control(False)),
        ("G-SELF-092", lambda: history_control(True)),
        ("G-SELF-093", collector_role_targets),
        ("G-SELF-094", routing_build_target_is_normal_bin),
        ("G-SELF-095", native_summary_is_scope_bound),
        ("G-SELF-096", native_system_config_is_minimal),
        ("G-SELF-097", native_address_space_budget_is_bounded),
        ("G-SELF-098", native_file_size_budget_is_bounded),
        ("G-SELF-099", secondary_runtime_mount_is_scoped),
    ]
