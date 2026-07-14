from __future__ import annotations

import json
import os
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

import pytest

from agent_benchmark.system import (
    COMMAND_VALIDATION_STDOUT_LIMIT,
    OutputDirectory,
    _agents,
    _aggregate,
    _comparison_fingerprint,
    _comparison_status,
    _dataset,
    _gc_candidates,
    _healthy_doctor_output,
    _percentile,
    _preflight,
    _report,
)
from agent_benchmark.agents import _redact, _usage


def _write_dataset(path: Path, records: list[dict[str, str]]) -> None:
    content = "".join(json.dumps(record) + "\n" for record in records)
    path.write_text(content, encoding="utf-8")


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


def test_agents_deduplicate_and_reject_unknown_roles() -> None:
    assert _agents(["coder,coder", "reviewer"]) == ["coder", "reviewer"]
    with pytest.raises(ValueError, match="unknown"):
        _agents(["executor"])


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


def test_aggregate_scopes_tokens_and_throughput_to_successes() -> None:
    failed = _record(success=False, tokens=100, latency=100.0, chars="secret")
    failed["ttft_seconds"] = 50.0
    metrics = _aggregate(
        [
            _record(success=True, tokens=4, latency=2.0, chars="abcd"),
            failed,
        ]
    )
    assert metrics["output_tokens"] == 4
    assert metrics["token_samples"] == 1
    assert metrics["ttft_samples"] == 1
    assert metrics["latency_samples"] == 1
    assert metrics["latency_ms"] == {"mean": 2000.0, "p50": 2000.0, "p95": 2000.0}
    assert metrics["ttft_ms"] == {"mean": 1000.0, "p50": 1000.0, "p95": 1000.0}
    assert metrics["throughput"]["tokens_per_second"] == 2.0
    assert metrics["throughput"]["chars_per_second"] == 2.0


def test_aggregate_reports_chars_when_tokens_are_unavailable() -> None:
    metrics = _aggregate([_record(success=True, tokens=0, latency=2.0, chars="abcd")])
    assert metrics["throughput"]["tokens_per_second"] is None
    assert metrics["throughput"]["chars_per_second"] == 2.0


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
