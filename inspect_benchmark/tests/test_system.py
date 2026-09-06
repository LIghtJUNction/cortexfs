from __future__ import annotations

import asyncio
import hashlib
import json
import os
import stat
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path
from types import SimpleNamespace

import pytest

from agent_benchmark.agents import _redact, _usage
from agent_benchmark.system import (
    COMMAND_VALIDATION_STDOUT_LIMIT,
    OutputDirectory,
    OwnedUserUnit,
    _agents,
    _aggregate,
    _comparison_fingerprint,
    _comparison_status,
    _dataset,
    _gc_candidates,
    _healthy_doctor_output,
    _percentile,
    _preflight,
    _publish_output,
    _report,
)


def _write_dataset(path: Path, records: list[dict[str, str]]) -> None:
    content = "".join(json.dumps(record) + "\n" for record in records)
    path.write_text(content, encoding="utf-8")


def _restore_output_permissions(path: Path) -> None:
    if not path.exists():
        return
    path.chmod(0o700)
    for child in path.iterdir():
        child.chmod(0o600)


def _owned_unit_evidence(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    membership: tuple[int, ...] = (101,),
    foreign_pid: int | None = None,
    peak: str = "4096\n",
    systemd_peak: int | None = 4096,
    events: str = "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n",
    cleanup_success: bool = True,
    stop_requested: bool = False,
) -> dict[str, object]:
    unit = OwnedUserUnit(
        cwd=str(tmp_path),
        environment={"PATH": "/bin"},
        command=["/bin/true"],
        timeout=1,
    )
    procs = tmp_path / "cgroup.procs"
    peak_path = tmp_path / "memory.peak"
    events_path = tmp_path / "memory.events"
    procs.write_text("".join(f"{pid}\n" for pid in membership), encoding="ascii")
    peak_path.write_text(peak, encoding="ascii")
    events_path.write_text(events, encoding="ascii")
    descriptors = {
        "directory": os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY),
        "cgroup.procs": os.open(procs, os.O_RDONLY),
        "memory.peak": os.open(peak_path, os.O_RDONLY),
        "memory.events": os.open(events_path, os.O_RDONLY),
    }
    invocation = "a" * 32
    calls = 0

    def snapshot(_unit: str, _timeout: float) -> dict[str, object]:
        nonlocal calls
        calls += 1
        if calls == 1:
            return {
                "success": True,
                "Id": unit.unit,
                "LoadState": "loaded",
                "ActiveState": "active",
                "SubState": "running",
                "InvocationID": invocation,
                "MainPID": 101,
                "ControlGroup": f"/fixture/{unit.unit}",
                "MemoryPeak": 2048,
            }
        if calls == 2:
            procs.write_text("", encoding="ascii")
            return {
                "success": True,
                "Id": unit.unit,
                "LoadState": "loaded",
                "ActiveState": "active",
                "SubState": "exited",
                "InvocationID": invocation,
                "MainPID": 0,
                "ControlGroup": "",
                "MemoryPeak": systemd_peak,
            }
        return {"success": False, "LoadState": "not-found"}

    actions: list[str] = []

    def control(_unit: str, action: str, _timeout: float) -> dict[str, object]:
        actions.append(action)
        return {"success": cleanup_success}

    monkeypatch.setattr("agent_benchmark.system._owned_unit_snapshot", snapshot)
    monkeypatch.setattr("agent_benchmark.system._owned_unit_control", control)
    monkeypatch.setattr(
        "agent_benchmark.system._process_uids",
        lambda pid: (999,) * 4 if pid == foreign_pid else (os.getuid(),) * 4,
    )
    monkeypatch.setattr(
        "agent_benchmark.system._process_identity",
        lambda pid: {"pid": pid, "start_time": 123, "complete": True},
    )
    monkeypatch.setattr(unit, "_open_cgroup", lambda _control_group: descriptors)

    async def observe() -> dict[str, object]:
        stop = asyncio.Event()
        if stop_requested:
            stop.set()
        return await unit.observe(SimpleNamespace(returncode=None), stop)  # type: ignore[arg-type]

    evidence = asyncio.run(observe())
    evidence["test_actions"] = actions
    return evidence


def _record(
    *, success: bool, tokens: int, latency: float, chars: str = "x"
) -> dict[str, object]:
    return {
        "runtime_success": success,
        "latency_seconds": latency,
        "ttft_seconds": latency / 2 if success else None,
        "input_tokens": tokens,
        "output_tokens": tokens,
        "completion": chars,
        "error_code": None if success else "EIO",
        "exit_code": 0,
        "exact_match": success,
    }


def test_owned_user_unit_fake_records_complete_peak_and_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evidence = _owned_unit_evidence(tmp_path, monkeypatch)
    assert evidence["complete"]
    assert evidence["memory_peak_bytes"] == 4096
    assert evidence["memory_events"] == {
        "low": 0,
        "high": 0,
        "max": 0,
        "oom": 0,
        "oom_kill": 0,
    }
    assert evidence["final_membership"] == ()
    assert evidence["cleanup"]["complete"]  # type: ignore[index]


def test_owned_user_unit_reuse_fails_preflight(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    unit = OwnedUserUnit(
        cwd=str(tmp_path),
        environment={"PATH": "/bin"},
        command=["/bin/true"],
        timeout=1,
    )
    monkeypatch.setattr(
        "agent_benchmark.system._owned_unit_snapshot",
        lambda _unit, _timeout: {"LoadState": "loaded"},
    )
    assert not asyncio.run(unit.preflight())


def test_owned_user_unit_foreign_pid_is_incomplete(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evidence = _owned_unit_evidence(
        tmp_path,
        monkeypatch,
        membership=(101, 202),
        foreign_pid=202,
    )
    assert not evidence["complete"]
    assert evidence["foreign_pids"] == (202,)


def test_owned_user_unit_timeout_still_verifies_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evidence = _owned_unit_evidence(
        tmp_path,
        monkeypatch,
        stop_requested=True,
    )
    assert evidence["complete"]
    assert evidence["cleanup"]["complete"]  # type: ignore[index]
    actions = evidence["test_actions"]
    assert isinstance(actions, list)
    assert "stop" in actions


def test_owned_user_unit_cleanup_failure_is_incomplete(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evidence = _owned_unit_evidence(
        tmp_path,
        monkeypatch,
        cleanup_success=False,
    )
    assert not evidence["complete"]
    assert not evidence["cleanup"]["complete"]  # type: ignore[index]


def test_owned_user_unit_missing_peak_is_incomplete(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    evidence = _owned_unit_evidence(tmp_path, monkeypatch, peak="", systemd_peak=None)
    assert not evidence["complete"]
    assert evidence["memory_peak_bytes"] is None


def test_dataset_accepts_valid_unique_records(tmp_path: Path) -> None:
    path = tmp_path / "data.jsonl"
    _write_dataset(
        path,
        [
            {
                "id": "one",
                "prompt": "question",
                "target": "answer",
                "category": "logic",
                "difficulty": "easy",
            }
        ],
    )
    assert [record["id"] for record in _dataset(path)] == ["one"]


@pytest.mark.parametrize("field", ["id", "prompt", "target"])
def test_dataset_rejects_empty_required_strings(tmp_path: Path, field: str) -> None:
    record = {"id": "one", "prompt": "question", "target": "answer"}
    record[field] = " "
    path = tmp_path / "data.jsonl"
    _write_dataset(path, [record])
    with pytest.raises(ValueError, match=field):
        _dataset(path)


def test_dataset_rejects_duplicate_ids_and_unknown_labels(tmp_path: Path) -> None:
    path = tmp_path / "duplicate.jsonl"
    record = {"id": "one", "prompt": "q", "target": "a"}
    _write_dataset(path, [record, record])
    with pytest.raises(ValueError, match="duplicate"):
        _dataset(path)
    path = tmp_path / "category.jsonl"
    _write_dataset(path, [{**record, "category": "unknown"}])
    with pytest.raises(ValueError, match="category"):
        _dataset(path)


def test_agents_deduplicate_and_reject_unsafe_names() -> None:
    assert _agents(["executor,executor", "field-notes"]) == [
        "executor",
        "field-notes",
    ]
    with pytest.raises(ValueError, match="invalid"):
        _agents(["../executor"])


def test_redaction_and_usage_validation() -> None:
    assert _redact(
        {
            "authorization": "Bearer abc",
            "nested": {"token": "abc"},
            "text": "Authorization: xyz provider_api_key=def tokenization: enabled",
        }
    ) == {
        "authorization": "[REDACTED]",
        "nested": {"token": "[REDACTED]"},
        "text": (
            "Authorization: [REDACTED] provider_api_key=[REDACTED] "
            "tokenization: enabled"
        ),
    }
    assert _redact("sk-" + "a" * 24) == "[REDACTED_TOKEN]"
    assert (
        _redact("eyJ" + "a" * 12 + "." + "b" * 12 + "." + "c" * 12) == "[REDACTED_JWT]"
    )
    assert _usage(3, "tokens") == 3
    with pytest.raises(ValueError, match="tokens"):
        _usage("3", "tokens")


@pytest.mark.parametrize(
    ("status_output", "accepted"),
    [
        ("idle\nmodel=main\n", True),
        ("mystery\nmodel=main\n", False),
        ("", False),
        ("idle active\nmodel=main\n", False),
        ("active\nmodel=main\n", False),
    ],
)
def test_preflight_requires_exact_idle_first_status_line(
    monkeypatch: pytest.MonkeyPatch, status_output: str, accepted: bool
) -> None:
    def command(
        values: list[str], _timeout: float, **_kwargs: object
    ) -> dict[str, object]:
        if values[1:] == ["status"]:
            stdout = "ctx\n    State: running\n   Status: ready\n  Mounted: yes\n"
        elif values[1:] == ["doctor"]:
            stdout = "ok root /ctx\nok status\n"
        elif values[0] == "findmnt":
            stdout = "/ctx cortexfs fuse rw,default_permissions,allow_other\n"
        elif values[1:2] == ["ping"]:
            stdout = '{"type":"pong"}\n'
        else:
            stdout = status_output
        return {"success": True, "stdout": stdout, "stderr": "", "exit_code": 0}

    monkeypatch.setattr("agent_benchmark.system._command", command)
    if accepted:
        _preflight("ctx", ["coder"], 1)
    else:
        with pytest.raises(ValueError, match="must begin with idle"):
            _preflight("ctx", ["coder"], 1)


def test_preflight_validates_complete_doctor_output_before_bounding_evidence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    doctor_stdout = [
        "ok root /ctx\n"
        + "ok session "
        + "x" * 8300
        + "\nok Authorization: benchmark-secret\nok final\n"
    ]

    def run(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        if command[1:] == ["status"]:
            stdout = "ctx\n    State: running\n   Status: ready\n  Mounted: yes\n"
        elif command[1:] == ["doctor"]:
            stdout = doctor_stdout[0]
        elif command[0] == "findmnt":
            stdout = "/ctx cortexfs fuse rw,default_permissions,allow_other\n"
        else:
            raise AssertionError(command)
        return subprocess.CompletedProcess(command, 0, stdout, "")

    monkeypatch.setattr(subprocess, "run", run)
    health = _preflight("ctx", [], 1)
    published = str(health["doctor"]["stdout"])
    assert len(published.encode()) <= 8192
    assert "benchmark-secret" not in published
    doctor_stdout[0] = "bad root /ctx\n" + "ok session " + "x" * 8300 + "\nok final\n"
    with pytest.raises(ValueError, match="doctor"):
        _preflight("ctx", [], 1)


def test_healthy_doctor_output_enforces_validation_boundary() -> None:
    at_limit = "ok " + "x" * (COMMAND_VALIDATION_STDOUT_LIMIT - 4) + "\n"
    over_limit = "ok " + "x" * (COMMAND_VALIDATION_STDOUT_LIMIT - 3) + "\n"
    assert len(at_limit.encode()) == COMMAND_VALIDATION_STDOUT_LIMIT
    assert len(over_limit.encode()) == COMMAND_VALIDATION_STDOUT_LIMIT + 1
    assert _healthy_doctor_output(at_limit)
    assert not _healthy_doctor_output(over_limit)


def test_gc_preview_parser_uses_literal_production_grammar() -> None:
    assert _gc_candidates(
        "ctx: dry-run; pass --yes to archive\narchive benchmark-coder-abc123\n"
    ) == ("benchmark-coder-abc123",)
    assert _gc_candidates("ctx: no matching agent sessions\n") == ()


def test_percentile_interpolates_edges() -> None:
    assert _percentile([], 0.95) is None
    assert _percentile([2.0], 0.95) == 2.0
    assert _percentile([1.0, 3.0], 0.5) == 2.0


def test_aggregate_charges_failed_usage_but_scopes_throughput_to_successes() -> None:
    failed = _record(success=False, tokens=100, latency=100.0, chars="secret")
    failed["ttft_seconds"] = 50.0
    metrics = _aggregate(
        [
            _record(success=True, tokens=4, latency=2.0, chars="abcd"),
            failed,
        ]
    )
    assert metrics["output_tokens"] == 104
    assert metrics["total_tokens"] == 208
    assert metrics["tokens_per_success"] == 208
    assert metrics["token_samples"] == 2
    assert metrics["failed_attempts_with_usage"] == 1
    assert metrics["ttft_samples"] == 1
    assert metrics["latency_samples"] == 1
    assert metrics["latency_ms"] == {"mean": 2000.0, "p50": 2000.0, "p95": 2000.0}
    assert metrics["ttft_ms"] == {"mean": 1000.0, "p50": 1000.0, "p95": 1000.0}
    assert metrics["throughput"]["tokens_per_second"] == 2.0
    assert metrics["throughput"]["chars_per_second"] == 2.0
    assert metrics["cache_read_tokens"] is None
    assert metrics["token_component_coverage"]["cache_read_tokens"] == 0.0


def test_aggregate_charges_failed_api_cost_to_each_success() -> None:
    success = _record(success=True, tokens=2, latency=1.0)
    failure = _record(success=False, tokens=3, latency=1.0)
    success["api_equivalent_cost_usd"] = 1.0
    failure["api_equivalent_cost_usd"] = 2.0
    metrics = _aggregate([success, failure])
    assert metrics["api_equivalent_cost_usd"] == 3.0
    assert metrics["api_equivalent_cost_per_success_usd"] == 3.0


def test_aggregate_reports_chars_when_tokens_are_unavailable() -> None:
    metrics = _aggregate([_record(success=True, tokens=0, latency=2.0, chars="abcd")])
    assert metrics["throughput"]["tokens_per_second"] is None
    assert metrics["throughput"]["chars_per_second"] == 2.0


def test_aggregate_does_not_price_or_normalize_partial_usage() -> None:
    complete = _record(success=True, tokens=4, latency=1.0)
    complete.update(
        usage_available=True,
        api_equivalent_cost_usd=0.01,
        provider_catalog_cost_usd=0.01,
    )
    missing = _record(success=True, tokens=0, latency=1.0)
    missing["usage_available"] = False
    metrics = _aggregate([complete, missing])
    assert metrics["token_samples"] == 1
    assert metrics["tokens_per_success"] is None
    assert metrics["api_equivalent_cost_usd"] is None
    assert metrics["provider_catalog_cost_usd"] is None


def test_report_contains_required_metric_fields() -> None:
    metrics = _aggregate([_record(success=True, tokens=4, latency=2.0)])
    summary = {
        "run_id": "run",
        "agents": {"coder": metrics},
        "overall": metrics,
        "fingerprint": "abc",
        "comparison": {"equivalent": True},
    }
    report = _report(summary)
    for expected in (
        "Latency mean/p50/p95",
        "TTFT mean/p50/p95",
        "Input/output tokens",
        "tokens/s",
        "chars/s",
        "## Errors",
    ):
        assert expected in report


def test_fingerprint_is_normalized_and_comparison_detects_mismatch(
    tmp_path: Path,
) -> None:
    left = {"agents": ["coder"], "timeout": 10.0}
    right = {"timeout": 10.0, "agents": ["coder"]}
    fingerprint = _comparison_fingerprint(left)
    assert fingerprint == _comparison_fingerprint(right)
    reference = tmp_path / "summary.json"
    reference.write_text(json.dumps({"fingerprint": fingerprint}), encoding="utf-8")
    assert _comparison_status(reference, fingerprint)["equivalent"] is True
    assert _comparison_status(reference, "different")["equivalent"] is False


def test_output_rejects_symlink_and_dotdot(tmp_path: Path) -> None:
    real = tmp_path / "real"
    real.mkdir()
    link = tmp_path / "link"
    link.symlink_to(real, target_is_directory=True)
    with pytest.raises(OSError):
        OutputDirectory.create(link / "results", "run")
    with pytest.raises(ValueError, match="must not contain"):
        OutputDirectory.create(Path("results") / ".." / "escape", "run")


def test_output_rejects_run_and_final_name_collisions(tmp_path: Path) -> None:
    output_path = tmp_path / "results"
    with OutputDirectory.create(output_path, "run") as output:
        output.write("summary.json", "{}\n")
        with pytest.raises(FileExistsError):
            output.write("summary.json", "changed\n")
        assert (output.path / "summary.json").read_text(encoding="utf-8") == "{}\n"
        assert not list(output.path.glob(".*.tmp-*"))
    with pytest.raises(FileExistsError):
        OutputDirectory.create(output_path, "run")


def test_publish_output_seals_manifest_and_modes(tmp_path: Path) -> None:
    samples = '{"sample":1}\n'
    summary = {"artifacts": {"immutable": True}}
    path = _publish_output(
        tmp_path / "results",
        "run",
        {"samples.jsonl": samples},
        summary,
        "report\n",
    )
    expected = {
        "report.md": "report\n",
        "samples.jsonl": samples,
        "summary.json": json.dumps(summary, indent=2, sort_keys=True) + "\n",
    }
    manifest = "".join(
        f"{hashlib.sha256(content.encode()).hexdigest()}  {name}\n"
        for name, content in sorted(expected.items())
    )
    try:
        assert (path / "manifest.sha256").read_text(encoding="utf-8") == manifest
        assert stat.S_IMODE(path.stat().st_mode) == 0o500
        for name in (*expected, "manifest.sha256"):
            assert stat.S_IMODE((path / name).stat().st_mode) == 0o400
    finally:
        _restore_output_permissions(path)


def test_output_rejects_writes_and_untracked_files_during_seal(
    tmp_path: Path,
) -> None:
    sealed_path = tmp_path / "sealed" / "run"
    try:
        with OutputDirectory.create(tmp_path / "sealed", "run") as output:
            output.write("samples.jsonl", "{}\n")
            output.seal()
            with pytest.raises(ValueError, match="already sealed"):
                output.write("late.json", "{}\n")
    finally:
        _restore_output_permissions(sealed_path)
    with OutputDirectory.create(tmp_path / "untracked", "run") as output:
        (output.path / "injected").write_text("unexpected", encoding="utf-8")
        with pytest.raises(ValueError, match="untracked"):
            output.seal()


def test_output_component_swap_fails_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    original_open = os.open
    replacement = tmp_path / "replacement"
    replacement.mkdir()
    swapped = False

    def swapping_open(
        path: str, flags: int, mode: int = 0o777, *, dir_fd: int | None = None
    ) -> int:
        nonlocal swapped
        if path == "parent" and dir_fd is not None and not swapped:
            os.rmdir(path, dir_fd=dir_fd)
            os.symlink(replacement, path, dir_fd=dir_fd, target_is_directory=True)
            swapped = True
        return original_open(path, flags, mode, dir_fd=dir_fd)

    monkeypatch.setattr(os, "open", swapping_open)
    with pytest.raises(OSError):
        OutputDirectory.create(tmp_path / "parent" / "results", "run")
    assert not (replacement / "results").exists()


def test_distribution_contains_entry_point_and_dataset(tmp_path: Path) -> None:
    project = Path(__file__).resolve().parents[1]
    output = tmp_path / "dist"
    result = subprocess.run(
        ["uv", "build", "--offline", "--out-dir", str(output)],
        cwd=project,
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    wheel = next(output.glob("*.whl"))
    source = next(output.glob("*.tar.gz"))
    with zipfile.ZipFile(wheel) as archive:
        names = set(archive.namelist())
        assert "agent_benchmark/data/agent_benchmark.jsonl" in names
        entry_points = next(name for name in names if name.endswith("entry_points.txt"))
        assert "agent_benchmark._registry" in archive.read(entry_points).decode()
    with tarfile.open(source, "r:gz") as archive:
        assert any(
            name.endswith("agent_benchmark/data/agent_benchmark.jsonl")
            for name in archive.getnames()
        )
    environment = tmp_path / "venv"
    subprocess.run(
        ["uv", "venv", "--python", sys.executable, str(environment)], check=True
    )
    python = environment / "bin" / "python"
    subprocess.run(
        ["uv", "pip", "install", "--python", str(python), "--no-deps", str(wheel)],
        check=True,
    )
    probe = subprocess.run(
        [
            str(python),
            "-c",
            "from importlib.resources import files; print(files('agent_benchmark').joinpath('data/agent_benchmark.jsonl').read_text())",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    assert probe.returncode == 0, probe.stderr
    assert '"id":"qa-001"' in probe.stdout
