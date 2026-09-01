import argparse
import asyncio
import ctypes
import hashlib
import json
import os
import re
import shlex
import shutil
import stat
import statistics
import subprocess
import sys
import time
import uuid
from collections import Counter
from collections.abc import Callable, Iterable, Mapping, Sequence
from contextlib import suppress
from dataclasses import replace
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .agents import (
    CleanupReceipt,
    SessionBaseline,
    SessionReceipt,
    benchmark_session_name,
    record_process_cleanup_complete,
    run_ctx_agent,
    sanitize,
)

DEFAULT_AGENTS = ("architect", "executor", "product-manager")
AGENT_NAME_PATTERN = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
ALLOWED_CATEGORIES = frozenset(
    {"general", "logic", "knowledge", "translation", "policy"}
)
ALLOWED_DIFFICULTIES = frozenset({"easy", "medium", "hard"})
COMMAND_VALIDATION_STDOUT_LIMIT = 256 * 1024
ARTIFACT_MANIFEST_NAME = "manifest.sha256"
SEALED_FILE_MODE = stat.S_IRUSR
SEALED_DIRECTORY_MODE = stat.S_IRUSR | stat.S_IXUSR
OWNED_UNIT_POLL_SECONDS = 0.005
OWNED_MEMORY_EVENT_KEYS = frozenset({"low", "high", "max", "oom", "oom_kill"})


def _sanitized_dict(value: object, limit: int) -> dict[str, Any]:
    sanitized = sanitize(value, limit)
    if not isinstance(sanitized, dict):
        raise TypeError("sanitized benchmark evidence must remain an object")
    return sanitized


def _command(
    command: list[str],
    timeout: float,
    *,
    stdout_validator: Callable[[str], bool] | None = None,
) -> dict[str, Any]:
    started = time.perf_counter()
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        success = result.returncode == 0 and (
            stdout_validator is None or stdout_validator(result.stdout)
        )
        value = {
            "command": command,
            "success": success,
            "exit_code": result.returncode,
            "latency_seconds": time.perf_counter() - started,
            "stdout": result.stdout[-8192:],
            "stderr": result.stderr[-8192:],
        }
        return _sanitized_dict(value, 8192)
    except (OSError, subprocess.TimeoutExpired) as error:
        value = {
            "command": command,
            "success": False,
            "exit_code": -1,
            "latency_seconds": time.perf_counter() - started,
            "stdout": "",
            "stderr": str(error),
        }
        return _sanitized_dict(value, 8192)


def _healthy_doctor_output(stdout: str) -> bool:
    if len(stdout.encode()) > COMMAND_VALIDATION_STDOUT_LIMIT:
        return False
    lines = [line for line in stdout.splitlines() if line]
    return bool(lines) and all(line.startswith("ok ") for line in lines)


def _preflight(ctx_path: str, agents: Sequence[str], timeout: float) -> dict[str, Any]:
    health = {
        "status": _command([ctx_path, "status"], min(timeout, 30.0)),
        "doctor": _command(
            [ctx_path, "doctor"],
            min(timeout, 30.0),
            stdout_validator=_healthy_doctor_output,
        ),
        "mount": _command(
            ["findmnt", "-no", "TARGET,SOURCE,FSTYPE,OPTIONS", "/ctx"],
            min(timeout, 10.0),
        ),
        "pings": {},
        "agents": {},
    }
    status = health["status"]
    status_text = str(status["stdout"])
    if not status["success"] or not all(
        marker in status_text
        for marker in ("State: running", "Status: ready", "Mounted: yes")
    ):
        raise ValueError("ctx status is not running, ready, and mounted")
    doctor = health["doctor"]
    if not doctor["success"]:
        raise ValueError("ctx doctor reported an unhealthy ABI")
    mount = health["mount"]
    fields = str(mount["stdout"]).split(None, 3)
    if (
        not mount["success"]
        or len(fields) != 4
        or fields[:3] != ["/ctx", "cortexfs", "fuse"]
        or not {"default_permissions", "allow_other"}.issubset(
            {option.strip() for option in fields[3].split(",")}
        )
    ):
        raise ValueError("/ctx is not the expected cortexfs FUSE mount")
    for agent_name in agents:
        ping = _command([ctx_path, "ping", f"agent/{agent_name}"], min(timeout, 10.0))
        try:
            frames = [json.loads(line) for line in str(ping["stdout"]).splitlines()]
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid ping response for {agent_name}") from error
        if (
            not ping["success"]
            or len(frames) != 1
            or not isinstance(frames[0], dict)
            or frames[0].get("type") != "pong"
        ):
            raise ValueError(f"agent ping failed: {agent_name}")
        agent_status = _command(
            [ctx_path, "agent", "status", agent_name], min(timeout, 10.0)
        )
        state = str(agent_status["stdout"]).splitlines()
        if not agent_status["success"] or not state or state[0] != "idle":
            observed = state[0] if state else "<empty>"
            reason = sanitize(
                f"agent status must begin with idle: {agent_name}: {observed}"
            )
            raise ValueError(str(reason))
        health["pings"][agent_name] = ping
        health["agents"][agent_name] = agent_status
    return _sanitized_dict(health, 8192)


SESSION_TEXT_LIMIT = 256 * 1024


def _open_dir(path: Path) -> int:
    descriptor = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for component in path.absolute().parts[1:]:
            next_descriptor = os.open(
                component,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=descriptor,
            )
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except OSError:
        os.close(descriptor)
        raise


def _read_at(directory: int, name: str, *, missing: str | None = None) -> str:
    try:
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory)
    except FileNotFoundError:
        if missing is not None:
            return missing
        raise
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError(f"unsafe session file: {name}")
        content = os.read(descriptor, SESSION_TEXT_LIMIT + 1)
        if len(content) > SESSION_TEXT_LIMIT:
            raise ValueError(f"session file exceeds limit: {name}")
        return content.decode("utf-8")
    finally:
        os.close(descriptor)


def _session_root(root: Path, uid: int, agent_name: str) -> Path:
    return root / "home" / str(uid) / "agent" / agent_name / "session"


def _session_baseline(
    agent_name: str,
    session: str,
    *,
    root: Path = Path("/ctx"),
    uid: int | None = None,
) -> SessionBaseline:
    session_fd = _open_dir(
        _session_root(root, os.getuid() if uid is None else uid, agent_name)
    )
    try:
        try:
            metadata = os.stat(session, dir_fd=session_fd, follow_symlinks=False)
        except FileNotFoundError:
            absent = True
        else:
            absent = False
            if not stat.S_ISDIR(metadata.st_mode):
                raise ValueError("session collision is not a plain directory")
        index_fd = os.open(
            "index", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=session_fd
        )
        try:
            current = (
                _read_at(index_fd, "current", missing="default").strip() or "default"
            )
            sessions = tuple(
                line
                for line in _read_at(index_fd, "list", missing="").splitlines()
                if line
            )
        finally:
            os.close(index_fd)
    finally:
        os.close(session_fd)
    return SessionBaseline(session, absent, current, sessions)


def _session_after(
    agent_name: str,
    session: str,
    *,
    root: Path = Path("/ctx"),
    uid: int | None = None,
) -> tuple[bool, str | None, str]:
    session_fd = _open_dir(
        _session_root(root, os.getuid() if uid is None else uid, agent_name)
    )
    try:
        try:
            owned_fd = os.open(
                session,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=session_fd,
            )
        except FileNotFoundError:
            appeared, state = False, None
        else:
            appeared = True
            try:
                state = _read_at(owned_fd, "state", missing="").strip() or None
            finally:
                os.close(owned_fd)
        index_fd = os.open(
            "index", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=session_fd
        )
        try:
            current = (
                _read_at(index_fd, "current", missing="default").strip() or "default"
            )
        finally:
            os.close(index_fd)
    finally:
        os.close(session_fd)
    return appeared, state, current


def _session_has_refs(
    agent_name: str,
    session: str,
    *,
    root: Path = Path("/ctx"),
    uid: int | None = None,
) -> bool:
    session_fd = _open_dir(
        _session_root(root, os.getuid() if uid is None else uid, agent_name)
    )
    try:
        owned_fd = os.open(
            session,
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
            dir_fd=session_fd,
        )
        try:
            try:
                context_fd = os.open(
                    "context",
                    os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                    dir_fd=owned_fd,
                )
            except FileNotFoundError:
                return False
            try:
                try:
                    child_fd = os.open(
                        "child",
                        os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                        dir_fd=context_fd,
                    )
                except FileNotFoundError:
                    return False
                try:
                    return bool(os.listdir(child_fd))
                finally:
                    os.close(child_fd)
            finally:
                os.close(context_fd)
        finally:
            os.close(owned_fd)
    finally:
        os.close(session_fd)


def _gc_candidates(stdout: str) -> tuple[str, ...]:
    candidates: list[str] = []
    for line in stdout.splitlines():
        if not line:
            continue
        if line == "ctx: dry-run; pass --yes to archive":
            continue
        if line == "ctx: no matching agent sessions":
            continue
        if line.startswith("archive ") and len(line) > len("archive "):
            candidates.append(line.removeprefix("archive "))
            continue
        raise ValueError(f"unexpected GC preview line: {line}")
    return tuple(candidates)


def _archive_session(
    ctx_path: str,
    agent_name: str,
    baseline: SessionBaseline,
    timeout: float,
    *,
    root: Path = Path("/ctx"),
    uid: int | None = None,
) -> SessionReceipt:
    session = baseline.session
    try:
        appeared, state, current = _session_after(
            agent_name, session, root=root, uid=uid
        )
    except (OSError, UnicodeError, ValueError) as error:
        return SessionReceipt(
            session,
            baseline,
            False,
            None,
            False,
            False,
            detail=str(sanitize(f"session inspection failed: {error}")),
        )
    owned = baseline.absent and appeared
    refusal: str | None = None
    if not owned:
        refusal = "session was pre-existing or did not appear"
    elif session == "default":
        refusal = "default session is protected"
    elif state not in {"idle", "done", "error"}:
        refusal = f"session is not terminal: {state!r}"
    else:
        try:
            if _session_has_refs(agent_name, session, root=root, uid=uid):
                refusal = "session has child references"
        except (OSError, ValueError) as error:
            refusal = f"session reference inspection failed: {error}"
    if refusal is None and current == session:
        if baseline.current not in baseline.sessions:
            refusal = "baseline current session was not present in baseline index"
        else:
            restore = _command(
                [
                    ctx_path,
                    "agent",
                    "session",
                    "select",
                    agent_name,
                    baseline.current,
                    "--from",
                    session,
                ],
                min(timeout, 30.0),
            )
            if not restore["success"]:
                refusal = str(
                    sanitize(
                        str(restore["stderr"])
                        or "current session restoration command failed"
                    )
                )
            else:
                try:
                    _appeared, _state, current = _session_after(
                        agent_name, session, root=root, uid=uid
                    )
                except (OSError, UnicodeError, ValueError) as error:
                    refusal = f"restored current session verification failed: {error}"
                if refusal is None and current != baseline.current:
                    refusal = "current session restoration verification mismatch"
    elif refusal is None and current != baseline.current:
        refusal = "current session changed since baseline"
    if refusal is not None:
        return SessionReceipt(
            session,
            baseline,
            owned,
            state,
            False,
            False,
            current_after=current,
            detail=str(sanitize(refusal)),
        )
    dry = _command(
        [
            ctx_path,
            "agent",
            "session",
            "gc",
            agent_name,
            "--dry-run",
            "--match",
            session,
        ],
        min(timeout, 30.0),
    )
    try:
        candidates = _gc_candidates(str(dry["stdout"]))
    except ValueError as error:
        return SessionReceipt(
            session,
            baseline,
            True,
            state,
            False,
            False,
            current_after=current,
            detail=str(sanitize(str(error))),
        )
    if not dry["success"] or candidates != (session,):
        return SessionReceipt(
            session,
            baseline,
            True,
            state,
            False,
            False,
            current_after=current,
            candidates=candidates,
            detail="GC dry-run did not select exactly the owned session",
        )
    archive = _command(
        [ctx_path, "agent", "session", "gc", agent_name, "--yes", "--match", session],
        min(timeout, 30.0),
    )
    archive_lines = tuple(line for line in str(archive["stdout"]).splitlines() if line)
    archived = bool(archive["success"] and archive_lines == (f"archived {session}",))
    return SessionReceipt(
        session,
        baseline,
        True,
        state,
        True,
        archived,
        current_after=current,
        candidates=candidates,
        detail=None
        if archived
        else str(
            sanitize(str(archive["stderr"]) or "GC archive confirmation mismatch")
        ),
    )


def _agents(values: Iterable[str]) -> list[str]:
    result: list[str] = []
    for value in values:
        for part in value.split(","):
            name = part.strip()
            if not name or name in result:
                continue
            if AGENT_NAME_PATTERN.fullmatch(name) is None:
                raise ValueError(f"invalid benchmark agent name: {name}")
            result.append(name)
    return result or list(DEFAULT_AGENTS)


def _dataset(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    identifiers: set[str] = set()
    with path.open(encoding="utf-8") as source:
        for number, line in enumerate(source, 1):
            if not line.strip():
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(f"invalid dataset JSON at line {number}") from error
            if not isinstance(value, dict):
                raise ValueError(f"invalid dataset record at line {number}")
            identifier = value.get("id")
            prompt = value.get("prompt")
            target = value.get("target")
            category = value.get("category", "general")
            difficulty = value.get("difficulty", "easy")
            if not isinstance(identifier, str) or not identifier.strip():
                raise ValueError(f"invalid dataset id at line {number}")
            if not isinstance(prompt, str) or not prompt.strip():
                raise ValueError(f"invalid dataset prompt at line {number}")
            if not isinstance(target, str) or not target.strip():
                raise ValueError(f"invalid dataset target at line {number}")
            if identifier in identifiers:
                raise ValueError(f"duplicate dataset id at line {number}: {identifier}")
            if category not in ALLOWED_CATEGORIES:
                raise ValueError(
                    f"invalid dataset category at line {number}: {category}"
                )
            if difficulty not in ALLOWED_DIFFICULTIES:
                raise ValueError(
                    f"invalid dataset difficulty at line {number}: {difficulty}"
                )
            identifiers.add(identifier)
            records.append(value)
    if not records:
        raise ValueError("benchmark dataset is empty")
    return records


def _percentile(values: Sequence[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = position.__floor__()
    upper = min(lower + 1, len(ordered) - 1)
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def _jsonl_content(records: Iterable[Mapping[str, object]]) -> str:
    return "".join(
        json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n"
        for record in records
    )


def _observation_metadata(
    sample: Mapping[str, object],
    sample_index: int,
    epoch: int,
    completion: str,
    runtime_success: bool,
) -> dict[str, object]:
    target = str(sample["target"])
    return {
        "sample_id": sample.get("id", f"sample-{sample_index + 1}"),
        "category": sample.get("category", "general"),
        "difficulty": sample.get("difficulty", "easy"),
        "epoch": epoch + 1,
        "target": target,
        "completion": completion,
        "exact_match": runtime_success and completion.strip() == target.strip(),
    }


def _distribution(values: list[float]) -> dict[str, float | None]:
    return {
        "mean": statistics.fmean(values) if values else None,
        "p50": _percentile(values, 0.50),
        "p95": _percentile(values, 0.95),
    }


def _timing(values: list[float]) -> dict[str, float | None]:
    return _distribution([value * 1000.0 for value in values])


def _nonnegative_number(value: object, field: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value < 0:
        raise ValueError(f"invalid benchmark metric {field}")
    return value * 1.0


def _nonnegative_count(value: object, field: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"invalid benchmark count {field}")
    return value


def _available_numbers(
    records: Sequence[Mapping[str, object]], field: str
) -> list[float]:
    values: list[float] = []
    for record in records:
        value = record.get(field)
        if value is not None:
            values.append(_nonnegative_number(value, field))
    return values


def _aggregate(records: list[dict[str, Any]]) -> dict[str, Any]:
    successes = [record for record in records if record["runtime_success"]]
    latencies = [
        _nonnegative_number(record["latency_seconds"], "latency_seconds")
        for record in successes
    ]
    ttfts = [
        _nonnegative_number(record["ttft_seconds"], "ttft_seconds")
        for record in successes
        if record["ttft_seconds"] is not None
    ]
    usage_records = [
        record
        for record in records
        if bool(
            record.get(
                "usage_available",
                _nonnegative_count(record["output_tokens"], "output_tokens") > 0,
            )
        )
    ]
    successful_usage_records = [
        record for record in usage_records if bool(record["runtime_success"])
    ]
    input_tokens = sum(
        _nonnegative_count(record["input_tokens"], "input_tokens")
        for record in usage_records
    )
    output_tokens = sum(
        _nonnegative_count(record["output_tokens"], "output_tokens")
        for record in usage_records
    )
    successful_output_tokens = sum(
        _nonnegative_count(record["output_tokens"], "output_tokens")
        for record in successful_usage_records
    )
    component_values: dict[str, list[int]] = {
        field: [
            _nonnegative_count(record[field], field)
            for record in usage_records
            if field in record
        ]
        for field in (
            "cache_read_tokens",
            "cache_write_tokens",
            "reasoning_tokens",
        )
    }
    component_totals = {
        field: sum(values) if len(values) == len(usage_records) else None
        for field, values in component_values.items()
    }
    component_coverage = {
        field: len(values) / len(usage_records) if usage_records else 0.0
        for field, values in component_values.items()
    }
    total_tokens = sum(
        _nonnegative_count(
            record.get(
                "total_tokens",
                _nonnegative_count(record["input_tokens"], "input_tokens")
                + _nonnegative_count(record["output_tokens"], "output_tokens"),
            ),
            "total_tokens",
        )
        for record in usage_records
    )
    visible_tokens = input_tokens + output_tokens
    usage_samples = len(usage_records)
    output_chars = sum(len(str(record["completion"])) for record in successes)
    elapsed = sum(latencies)
    peak_rss = _available_numbers(records, "peak_rss_bytes")
    peak_memory = _available_numbers(records, "peak_memory_bytes")
    complete_memory_samples = sum(
        bool(record.get("memory_complete")) for record in records
    )
    complete_protocol_samples = sum(
        bool(record.get("raw_frames_complete")) for record in records
    )
    complete_process_cleanup_samples = sum(
        record_process_cleanup_complete(record) for record in records
    )
    complete_runtime_process_identity_samples = sum(
        isinstance((evidence := record.get("runtime_process_evidence")), dict)
        and bool(evidence.get("complete"))
        for record in records
        if record.get("runtime") != "pi"
    )
    api_costs = _available_numbers(records, "api_equivalent_cost_usd")
    catalog_costs = _available_numbers(records, "provider_catalog_cost_usd")
    errors = Counter(
        str(record["error_code"] or f"exit_{record['exit_code']}")
        for record in records
        if not record["runtime_success"]
    )
    complete_usage = usage_samples == len(records) and bool(records)
    complete_success_usage = len(successful_usage_records) == len(successes) and bool(
        successes
    )
    throughput = {
        "tokens_per_second": successful_output_tokens / elapsed
        if elapsed and complete_success_usage
        else None,
        "chars_per_second": output_chars / elapsed if elapsed else None,
    }
    total = len(records)
    success_count = len(successes)
    return {
        "requests": total,
        "runtime_successes": success_count,
        "runtime_success_rate": success_count / total if total else 0.0,
        "exact_matches": sum(
            bool(record["runtime_success"] and record["exact_match"])
            for record in records
        ),
        "exact_accuracy": sum(
            bool(record["runtime_success"] and record["exact_match"])
            for record in records
        )
        / total
        if total
        else 0.0,
        "errors": dict(sorted(errors.items())),
        "latency_ms": _timing(latencies),
        "latency_samples": len(latencies),
        "ttft_ms": _timing(ttfts),
        "ttft_samples": len(ttfts),
        "token_samples": usage_samples,
        "usage_coverage": usage_samples / total if total else 0.0,
        "failed_attempts_with_usage": sum(
            not bool(record["runtime_success"]) for record in usage_records
        ),
        "successful_samples": success_count,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_read_tokens": component_totals["cache_read_tokens"],
        "cache_write_tokens": component_totals["cache_write_tokens"],
        "reasoning_tokens": component_totals["reasoning_tokens"],
        "token_component_coverage": component_coverage,
        "total_tokens": total_tokens,
        "visible_input_output_tokens": visible_tokens,
        "visible_tokens_per_success": visible_tokens / success_count
        if success_count and complete_usage
        else None,
        "tokens_per_success": total_tokens / success_count
        if success_count and complete_usage
        else None,
        "output_chars": output_chars,
        "peak_rss_bytes": _distribution(peak_rss),
        "rss_samples": len(peak_rss),
        "peak_memory_bytes": _distribution(peak_memory),
        "memory_samples": len(peak_memory),
        "complete_memory_samples": complete_memory_samples,
        "complete_protocol_evidence_samples": complete_protocol_samples,
        "complete_process_cleanup_samples": complete_process_cleanup_samples,
        "complete_runtime_process_identity_samples": complete_runtime_process_identity_samples,
        "provider_catalog_cost_samples": len(catalog_costs),
        "provider_catalog_cost_usd": sum(catalog_costs)
        if len(catalog_costs) == total and total
        else None,
        "api_equivalent_cost_samples": len(api_costs),
        "api_equivalent_cost_usd": sum(api_costs)
        if len(api_costs) == total and total
        else None,
        "api_equivalent_cost_per_success_usd": sum(api_costs) / success_count
        if len(api_costs) == total and success_count
        else None,
        "attributable_subscription_cost_usd": None,
        "throughput": throughput,
    }


def _memory_value(fields: Mapping[str, str], key: str) -> int | None:
    value = fields.get(key, "")
    return int(value) if value.isdecimal() else None


def _systemctl_fields(
    unit: str,
    properties: Sequence[str],
    timeout: float,
    *,
    user: bool = False,
) -> tuple[bool, dict[str, str], str]:
    systemctl = ["systemctl"]
    if user:
        systemctl.append("--user")
    command = _hashed_command(
        [
            *systemctl,
            "show",
            "--no-pager",
            *[f"--property={name}" for name in properties],
            unit,
        ],
        min(timeout, 10.0),
        retain_stdout=True,
    )
    fields: dict[str, str] = {}
    for line in str(command["stdout"]).splitlines():
        key, separator, value = line.partition("=")
        if separator and key not in fields:
            fields[key] = value
    complete = bool(command["success"] and command["complete"])
    sanitized_hash = hashlib.sha256(str(command["stdout"]).encode()).hexdigest()
    return complete, fields, sanitized_hash


def _unit_memory(unit: str, timeout: float) -> dict[str, object]:
    success, fields, _configuration_hash = _systemctl_fields(
        unit,
        ("InvocationID", "MemoryCurrent", "MemoryPeak", "ActiveState"),
        timeout,
    )
    return {
        "unit": unit,
        "success": success,
        "invocation_id": fields.get("InvocationID") or None,
        "active_state": fields.get("ActiveState") or None,
        "memory_current_bytes": _memory_value(fields, "MemoryCurrent"),
        "memory_peak_bytes": _memory_value(fields, "MemoryPeak"),
    }


def _memory_evidence(
    before_agent: Mapping[str, object],
    after_agent: Mapping[str, object],
    before_mount: Mapping[str, object],
    after_mount: Mapping[str, object],
) -> dict[str, object]:
    before_invocation = before_agent.get("invocation_id")
    after_invocation = after_agent.get("invocation_id")
    attributable = bool(
        after_agent.get("success")
        and after_invocation
        and after_invocation != before_invocation
    )
    peak = after_agent.get("memory_peak_bytes") if attributable else None
    return {
        "agent_before": dict(before_agent),
        "agent_after": dict(after_agent),
        "mount_before": dict(before_mount),
        "mount_after": dict(after_mount),
        "agent_invocation_attributed": attributable,
        "peak_memory_bytes": peak if isinstance(peak, int) else None,
        "scope": "agent-cgroup-memory-peak-plus-mount-endpoints",
        "complete_stack_peak": False,
        "limitation": "persistent cortexfs.service has endpoint samples, not a request-scoped cgroup peak",
    }


async def _ctx_observation(
    *,
    ctx_path: str,
    agent_name: str,
    sample: Mapping[str, object],
    sample_index: int,
    epoch: int,
    timeout: float,
    retain_frames: bool,
    measure_memory: bool,
) -> dict[str, Any]:
    for _attempt in range(8):
        session = benchmark_session_name("benchmark", agent_name)
        baseline = await asyncio.to_thread(_session_baseline, agent_name, session)
        if baseline.absent:
            break
    else:
        raise ValueError(f"cannot allocate a unique benchmark session for {agent_name}")
    if measure_memory:
        before_agent = await asyncio.to_thread(
            _unit_memory, f"cortexfs-agent@{agent_name}.service", timeout
        )
        before_mount = await asyncio.to_thread(
            _unit_memory, "cortexfs.service", timeout
        )
    else:
        before_agent = {"success": False}
        before_mount = {"success": False}
    run_task = asyncio.create_task(
        run_ctx_agent(
            ctx_path=ctx_path,
            agent_name=agent_name,
            session=session,
            prompt=str(sample["prompt"]),
            timeout=timeout,
            retain_frames=retain_frames,
        )
    )
    process_identity_task = asyncio.create_task(
        _observe_unit_processes(f"cortexfs-agent@{agent_name}.service", run_task)
    )
    try:
        result = await run_task
        if measure_memory:
            after_agent = await asyncio.to_thread(
                _unit_memory, f"cortexfs-agent@{agent_name}.service", timeout
            )
            after_mount = await asyncio.to_thread(
                _unit_memory, "cortexfs.service", timeout
            )
        else:
            after_agent = {"success": False}
            after_mount = {"success": False}
        memory = _memory_evidence(before_agent, after_agent, before_mount, after_mount)
    finally:
        try:
            runtime_process_evidence = await process_identity_task
        finally:
            active_error = sys.exception()
            session_receipt = await asyncio.to_thread(
                _archive_session,
                ctx_path,
                agent_name,
                baseline,
                timeout,
            )
            if active_error is not None and not session_receipt.archived:
                raise RuntimeError(
                    f"benchmark session cleanup also failed: {session_receipt.detail}"
                ) from active_error
    result = replace(
        result,
        cleanup=CleanupReceipt(session_receipt, result.cancellation),
    )
    record = result.metadata()
    sample_metadata = sanitize(
        {
            **_observation_metadata(
                sample,
                sample_index,
                epoch,
                result.completion,
                result.runtime_success,
            ),
            "cleanup_success": session_receipt.archived,
            "peak_rss_bytes": None,
            "peak_memory_bytes": memory["peak_memory_bytes"],
            "memory_complete": False,
            "memory": memory,
            "runtime_process_evidence": runtime_process_evidence,
        }
    )
    if not isinstance(sample_metadata, dict):
        raise TypeError("sanitized sample metadata must remain an object")
    record.update(sample_metadata)
    return record


async def _samples(
    *,
    ctx_path: str,
    agents: list[str],
    dataset: list[dict[str, Any]],
    repeat: int,
    timeout: float,
    retain_frames: bool = False,
    measure_memory: bool = True,
) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for agent_name in agents:
        for epoch in range(repeat):
            for index, sample in enumerate(dataset):
                output.append(
                    await _ctx_observation(
                        ctx_path=ctx_path,
                        agent_name=agent_name,
                        sample=sample,
                        sample_index=index,
                        epoch=epoch,
                        timeout=timeout,
                        retain_frames=retain_frames,
                        measure_memory=measure_memory,
                    )
                )
    return output


def _runtime_contracts_complete(identity: Mapping[str, object]) -> bool:
    sections = [identity.get("agent_contracts"), identity.get("model_contracts")]
    if any(not isinstance(section, dict) or not section for section in sections):
        return False
    controls_complete = all(
        isinstance(contract, dict)
        and bool(contract)
        and all(
            isinstance(evidence, dict) and bool(evidence.get("success"))
            for evidence in contract.values()
        )
        for section in sections
        if isinstance(section, dict)
        for contract in section.values()
    )
    units = identity.get("unit_configurations")
    if not isinstance(units, dict):
        return False
    mount = units.get("mount")
    agents = units.get("agents")
    return bool(
        controls_complete
        and isinstance(mount, dict)
        and mount.get("complete")
        and isinstance(agents, dict)
        and agents
        and all(
            isinstance(unit, dict) and unit.get("complete") for unit in agents.values()
        )
    )


def _runtime_has_no_visible_tools(identity: Mapping[str, object]) -> bool:
    contracts = identity.get("agent_contracts")
    if not isinstance(contracts, dict) or not contracts:
        return False
    return all(
        isinstance(contract, dict)
        and isinstance((tools := contract.get("tools")), dict)
        and bool(tools.get("success"))
        and not str(tools.get("stdout", "")).strip()
        for contract in contracts.values()
    )


def _validate_model_controls(
    identity: Mapping[str, object], provider: str, model: str, thinking: str
) -> None:
    expected_route = f"{provider}/{model}"
    routes = identity.get("model_routes")
    efforts = identity.get("model_efforts")
    if not isinstance(routes, dict) or any(
        route != expected_route for route in routes.values()
    ):
        raise ValueError(
            f"declared provider/model does not match every agent route: expected {expected_route}"
        )
    if not isinstance(efforts, dict) or efforts.get(expected_route) != thinking:
        raise ValueError(
            f"declared thinking does not match model effort for {expected_route}"
        )


def _dataset_identity(
    path: Path, records: Sequence[Mapping[str, object]]
) -> dict[str, object]:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return {
        "path": str(path),
        "sha256": digest,
        "ids": [str(record["id"]) for record in records],
        "samples": len(records),
    }


_LIBC = ctypes.CDLL(None, use_errno=True)
_LIBC.renameat2.argtypes = (
    ctypes.c_int,
    ctypes.c_char_p,
    ctypes.c_int,
    ctypes.c_char_p,
    ctypes.c_uint,
)
_LIBC.renameat2.restype = ctypes.c_int
_RENAME_NOREPLACE = 1


def _rename_noreplace(directory_fd: int, source: str, target: str) -> None:
    result = _LIBC.renameat2(
        directory_fd,
        source.encode(),
        directory_fd,
        target.encode(),
        _RENAME_NOREPLACE,
    )
    if result != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), target)


def _artifact_names(directory_fd: int) -> set[str]:
    descriptor = os.open(
        ".",
        os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
        dir_fd=directory_fd,
    )
    try:
        return set(os.listdir(descriptor))
    finally:
        os.close(descriptor)


def _artifact_mode(directory_fd: int, name: str) -> int:
    descriptor = os.open(
        name,
        os.O_RDONLY | os.O_NOFOLLOW,
        dir_fd=directory_fd,
    )
    try:
        return stat.S_IMODE(os.fstat(descriptor).st_mode)
    finally:
        os.close(descriptor)


def _artifact_digest(directory_fd: int, name: str) -> str:
    descriptor = os.open(
        name,
        os.O_RDONLY | os.O_NOFOLLOW,
        dir_fd=directory_fd,
    )
    digest = hashlib.sha256()
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise ValueError(f"benchmark artifact is not a regular file: {name}")
        while block := os.read(descriptor, 64 * 1024):
            digest.update(block)
    finally:
        os.close(descriptor)
    return digest.hexdigest()


class OutputDirectory:
    """Own the output and run directory descriptors for safe publication."""

    def __init__(self, root_fd: int, run_fd: int, path: Path) -> None:
        self._root_fd = root_fd
        self._run_fd = run_fd
        self._names: set[str] = set()
        self._sealed = False
        self.path = path

    @classmethod
    def create(cls, output_path: Path, run_id: str) -> "OutputDirectory":
        expanded = output_path.expanduser()
        if ".." in expanded.parts:
            raise ValueError("output path must not contain '..'")
        absolute = expanded.absolute()
        components = absolute.parts[1:]
        current_fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            for component in components:
                if component in {"", ".", ".."}:
                    raise ValueError(f"invalid output path component: {component!r}")
                with suppress(FileExistsError):
                    os.mkdir(component, mode=0o700, dir_fd=current_fd)
                next_fd = os.open(
                    component,
                    os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                    dir_fd=current_fd,
                )
                os.close(current_fd)
                current_fd = next_fd
            os.mkdir(run_id, mode=0o700, dir_fd=current_fd)
            run_fd = os.open(
                run_id,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=current_fd,
            )
            os.fsync(current_fd)
            return cls(current_fd, run_fd, absolute / run_id)
        except (OSError, ValueError):
            os.close(current_fd)
            raise

    def __enter__(self) -> "OutputDirectory":
        return self

    def __exit__(self, exc_type: object, exc_value: object, traceback: object) -> None:
        self.close()

    def close(self) -> None:
        if self._run_fd >= 0:
            os.close(self._run_fd)
            self._run_fd = -1
        if self._root_fd >= 0:
            os.close(self._root_fd)
            self._root_fd = -1

    def write(self, name: str, content: str) -> None:
        if self._sealed:
            raise ValueError("benchmark output is already sealed")
        if not name or name in {".", ".."} or "/" in name:
            raise ValueError(f"invalid output file name: {name!r}")
        temporary = f".{name}.tmp-{uuid.uuid4().hex}"
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
        descriptor = os.open(temporary, flags, 0o600, dir_fd=self._run_fd)
        published = False
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as target:
                target.write(content)
                target.flush()
                os.fsync(target.fileno())
            _rename_noreplace(self._run_fd, temporary, name)
            published = True
            self._names.add(name)
            os.fsync(self._run_fd)
        finally:
            if not published:
                with suppress(FileNotFoundError):
                    os.unlink(temporary, dir_fd=self._run_fd)

    def seal(self) -> None:
        if self._sealed:
            raise ValueError("benchmark output is already sealed")
        if _artifact_names(self._run_fd) != self._names:
            raise ValueError("benchmark output contains an untracked artifact")
        digests = {
            name: _artifact_digest(self._run_fd, name) for name in sorted(self._names)
        }
        manifest = "".join(f"{digest}  {name}\n" for name, digest in digests.items())
        self.write(ARTIFACT_MANIFEST_NAME, manifest)
        expected = {
            **digests,
            ARTIFACT_MANIFEST_NAME: hashlib.sha256(manifest.encode()).hexdigest(),
        }
        for name, digest in expected.items():
            if _artifact_digest(self._run_fd, name) != digest:
                raise ValueError(f"benchmark artifact changed before sealing: {name}")
            descriptor = os.open(
                name,
                os.O_RDONLY | os.O_NOFOLLOW,
                dir_fd=self._run_fd,
            )
            try:
                os.fchmod(descriptor, SEALED_FILE_MODE)
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
        os.fchmod(self._run_fd, SEALED_DIRECTORY_MODE)
        os.fsync(self._run_fd)
        os.fsync(self._root_fd)
        if _artifact_names(self._run_fd) != set(expected):
            raise ValueError("benchmark output changed while sealing")
        for name, digest in expected.items():
            if _artifact_digest(self._run_fd, name) != digest:
                raise ValueError(f"benchmark artifact changed while sealing: {name}")
            if _artifact_mode(self._run_fd, name) != SEALED_FILE_MODE:
                raise ValueError(f"benchmark artifact is not sealed: {name}")
        if stat.S_IMODE(os.fstat(self._run_fd).st_mode) != SEALED_DIRECTORY_MODE:
            raise ValueError("benchmark output directory is not sealed")
        self._sealed = True


def _publish_output(
    output_dir: Path,
    run_id: str,
    files: Mapping[str, str],
    summary: Mapping[str, object],
    report: str,
) -> Path:
    with OutputDirectory.create(output_dir, run_id) as output:
        for name, content in files.items():
            output.write(name, content)
        output.write(
            "summary.json",
            json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        )
        output.write("report.md", report)
        output.seal()
        return output.path


def _add_baseline_arguments(
    parser: argparse.ArgumentParser, default_output: Path
) -> None:
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path(__file__).with_name("data") / "agent_benchmark.jsonl",
    )
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument("--output-dir", type=Path, default=default_output)
    parser.add_argument("--compare-summary", type=Path)
    parser.add_argument("--allow-mismatch", action="store_true")
    parser.add_argument("--retain-frames", action="store_true")


def _validated_comparison(
    summary_path: Path | None, fingerprint: str, allow_mismatch: bool
) -> dict[str, object]:
    comparison = _comparison_status(summary_path, fingerprint)
    if (
        comparison["equivalent"] is not None
        and not comparison["equivalent"]
        and not allow_mismatch
    ):
        raise ValueError("comparison summary workload does not match this run")
    return comparison


def _hashed_command(
    command: list[str], timeout: float, *, retain_stdout: bool = False
) -> dict[str, object]:
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return {
            "success": False,
            "complete": False,
            "sha256": None,
            "bytes": None,
            "stdout": "",
        }
    size = len(result.stdout)
    if size > COMMAND_VALIDATION_STDOUT_LIMIT:
        return {
            "success": False,
            "complete": False,
            "sha256": None,
            "bytes": size,
            "stdout": "",
        }
    try:
        stdout = result.stdout.decode("utf-8")
    except UnicodeDecodeError:
        return {
            "success": False,
            "complete": False,
            "sha256": None,
            "bytes": size,
            "stdout": "",
        }
    success = result.returncode == 0
    return {
        "success": success,
        "complete": True,
        "sha256": hashlib.sha256(result.stdout).hexdigest() if success else None,
        "bytes": size,
        "stdout": str(sanitize(stdout, COMMAND_VALIDATION_STDOUT_LIMIT))
        if retain_stdout
        else "",
    }


def _hash_executable(path: Path) -> tuple[str, int]:
    descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise OSError("executable is not a regular file")
        digest = hashlib.sha256()
        size = 0
        while chunk := os.read(descriptor, 1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    finally:
        os.close(descriptor)
    return digest.hexdigest(), size


def _benchmark_source_identity(names: Sequence[str]) -> dict[str, object]:
    directory = Path(__file__).parent
    files: dict[str, dict[str, object]] = {}
    for name in sorted(set(names)):
        if Path(name).name != name or not name.endswith(".py"):
            raise ValueError("invalid benchmark source name")
        digest, size = _hash_executable((directory / name).resolve())
        files[name] = {"sha256": digest, "bytes": size}
    canonical = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {"sha256": hashlib.sha256(canonical).hexdigest(), "files": files}


def _executable_identity(command: str, timeout: float) -> dict[str, object]:
    resolved = shutil.which(command)
    if resolved is None:
        return {"resolved_path": None, "sha256": None, "bytes": None, "version": None}
    path = Path(resolved).resolve()
    try:
        digest, size = _hash_executable(path)
    except OSError:
        return {
            "resolved_path": str(path),
            "sha256": None,
            "bytes": None,
            "version": None,
        }
    version = _command([str(path), "--version"], timeout)
    return {
        "resolved_path": str(path),
        "sha256": digest,
        "bytes": size,
        "version": str(version["stdout"]).strip() if version["success"] else None,
    }


def _file_identity(path_value: str) -> dict[str, object]:
    path = Path(path_value).expanduser().resolve()
    try:
        digest, size = _hash_executable(path)
    except OSError:
        return {"path": str(path), "sha256": None, "bytes": None}
    return {"path": str(path), "sha256": digest, "bytes": size}


def _unit_configuration_identity(unit: str, timeout: float) -> dict[str, object]:
    success, fields, systemctl_hash = _systemctl_fields(
        unit,
        (
            "FragmentPath",
            "DropInPaths",
            "ExecStart",
            "Environment",
            "EnvironmentFiles",
            "User",
            "Group",
        ),
        timeout,
    )
    fragment = fields.get("FragmentPath", "")
    try:
        drop_ins = shlex.split(fields.get("DropInPaths", ""))
    except ValueError:
        drop_ins = []
    executable_match = re.search(
        r"(?:^|[ {;])path=([^ ;}]+)", fields.get("ExecStart", "")
    )
    executable = executable_match.group(1) if executable_match else ""
    nonsecret = {
        key: str(sanitize(value))
        for key, value in fields.items()
        if key not in {"FragmentPath", "DropInPaths"}
    }
    environment_hash = hashlib.sha256(
        json.dumps(nonsecret, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()
    files = {
        "fragment": _file_identity(fragment) if fragment else None,
        "drop_ins": [_file_identity(path) for path in drop_ins],
        "executable": _file_identity(executable) if executable else None,
    }
    complete_files = all(
        item is None or bool(item.get("sha256"))
        for item in [files["fragment"], files["executable"], *files["drop_ins"]]
    )
    return {
        "unit": unit,
        "success": success,
        "configuration_sha256": environment_hash,
        "systemctl_stdout_sha256": systemctl_hash,
        "files": files,
        "complete": bool(success and fragment and executable and complete_files),
    }


def _unit_runtime_snapshot(unit: str, timeout: float) -> dict[str, object]:
    success, fields, systemctl_hash = _systemctl_fields(
        unit, ("MainPID", "InvocationID", "ActiveState"), timeout
    )
    pid_text = fields.get("MainPID", "")
    pid = int(pid_text) if pid_text.isdecimal() else 0
    process = _process_identity(pid) if pid > 1 else None
    return {
        "unit": unit,
        "success": success,
        "main_pid": pid or None,
        "invocation_id": fields.get("InvocationID") or None,
        "active_state": fields.get("ActiveState") or None,
        "systemctl_stdout_sha256": systemctl_hash,
        "process": process,
        "complete": bool(
            success
            and pid > 1
            and fields.get("ActiveState") == "active"
            and fields.get("InvocationID")
            and process is not None
            and process.get("complete")
        ),
    }


def _runtime_snapshots_stable(
    before: Mapping[str, object], after: Mapping[str, object]
) -> bool:
    if before != after:
        return False
    mount = before.get("mount")
    agents = before.get("agents")
    return bool(
        isinstance(mount, dict)
        and mount.get("complete")
        and isinstance(agents, dict)
        and agents
        and all(
            isinstance(snapshot, dict) and snapshot.get("complete")
            for snapshot in agents.values()
        )
    )


def _proc_value(pid: int, name: str, limit: int = 1024 * 1024) -> bytes | None:
    descriptor = -1
    try:
        descriptor = os.open(
            f"/proc/{pid}/{name}",
            os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW,
        )
        chunks: list[bytes] = []
        size = 0
        while chunk := os.read(descriptor, min(64 * 1024, limit + 1 - size)):
            chunks.append(chunk)
            size += len(chunk)
            if size > limit:
                return None
        return b"".join(chunks)
    except OSError:
        return None
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def _process_identity(pid: int) -> dict[str, object]:
    stat_bytes = _proc_value(pid, "stat", 4096)
    environ = _proc_value(pid, "environ")
    start_time: int | None = None
    if stat_bytes is not None:
        try:
            start_time = int(stat_bytes.rsplit(b") ", 1)[1].split()[19])
        except IndexError:
            start_time = None
        except ValueError:
            start_time = None
    try:
        executable = os.readlink(f"/proc/{pid}/exe")
    except OSError:
        executable = ""
    executable = executable.removesuffix(" (deleted)")
    executable_identity = _file_identity(executable) if executable else None
    environment_entries: list[str] = []
    if environ is not None:
        for raw in environ.split(b"\0"):
            if not raw:
                continue
            text = raw.decode("utf-8", errors="replace")
            key, separator, value = text.partition("=")
            normalized = key.lower()
            if separator and not any(
                marker in normalized
                for marker in (
                    "secret",
                    "token",
                    "password",
                    "credential",
                    "auth",
                    "key",
                )
            ):
                environment_entries.append(f"{key}={value}")
    environment_hash = (
        hashlib.sha256("\0".join(sorted(environment_entries)).encode()).hexdigest()
        if environ is not None
        else None
    )
    return {
        "pid": pid,
        "start_time": start_time,
        "executable": executable_identity,
        "environment_sha256": environment_hash,
        "environment_keys": sorted(
            entry.partition("=")[0] for entry in environment_entries
        ),
        "complete": bool(
            start_time
            and executable_identity
            and executable_identity.get("sha256")
            and environment_hash
        ),
    }


def _unit_cgroup_pids(unit: str) -> tuple[int, ...]:
    if not re.fullmatch(r"[A-Za-z0-9@_.-]+", unit):
        return ()
    raw = b""
    candidates = (
        Path("/sys/fs/cgroup/system.slice") / unit / "cgroup.procs",
        Path("/sys/fs/cgroup/user.slice") / unit / "cgroup.procs",
    )
    for path in candidates:
        descriptor = -1
        try:
            descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
            raw = os.read(descriptor, 256 * 1024)
        except OSError:
            continue
        finally:
            if descriptor >= 0:
                os.close(descriptor)
        break
    values: list[int] = []
    for line in raw.splitlines():
        try:
            value = int(line)
        except ValueError:
            continue
        if value > 1:
            values.append(value)
    return tuple(sorted(set(values)))


async def _observe_unit_processes(
    unit: str, task: asyncio.Task[Any]
) -> dict[str, object]:
    observations: dict[tuple[int, int], dict[str, object]] = {}
    runtime_snapshot: dict[str, object] | None = None
    while True:
        pids = await asyncio.to_thread(_unit_cgroup_pids, unit)
        if pids and runtime_snapshot is None:
            runtime_snapshot = await asyncio.to_thread(
                _unit_runtime_snapshot, unit, 10.0
            )
        for pid in pids:
            identity = await asyncio.to_thread(_process_identity, pid)
            start_time = identity.get("start_time")
            if isinstance(start_time, int):
                observations[(pid, start_time)] = identity
        if task.done():
            break
        await asyncio.sleep(0.02)
    values = list(observations.values())
    return {
        "unit": unit,
        "observations": values,
        "runtime_snapshot": runtime_snapshot,
        "complete": bool(
            values
            and all(item.get("complete") for item in values)
            and runtime_snapshot
            and runtime_snapshot.get("complete")
            and runtime_snapshot.get("invocation_id")
        ),
    }


def _owned_unit_snapshot(unit: str, timeout: float) -> dict[str, object]:
    success, fields, _digest = _systemctl_fields(
        unit,
        (
            "Id",
            "LoadState",
            "ActiveState",
            "SubState",
            "InvocationID",
            "MainPID",
            "ControlGroup",
            "ExecMainStatus",
            "MemoryPeak",
        ),
        timeout,
        user=True,
    )
    values: dict[str, object] = {"success": success, **fields}
    for name in ("MainPID", "ExecMainStatus", "MemoryPeak"):
        raw = fields.get(name)
        try:
            values[name] = int(raw) if raw and raw != "[not set]" else None
        except ValueError:
            values[name] = None
    return values


def _process_uids(pid: int) -> tuple[int, ...] | None:
    status = _proc_value(pid, "status", 256 * 1024)
    if status is None:
        return None
    for line in status.splitlines():
        if line.startswith(b"Uid:"):
            try:
                return tuple(int(value) for value in line.split()[1:])
            except ValueError:
                return None
    return None


def _owned_cgroup_path(control_group: str, uid: int, unit: str) -> Path | None:
    prefix = f"/user.slice/user-{uid}.slice/user@{uid}.service/"
    if not control_group.startswith(prefix) or Path(control_group).name != unit:
        return None
    path = Path("/sys/fs/cgroup") / control_group.removeprefix("/")
    return path if ".." not in path.parts else None


def _read_cgroup_file(descriptor: int) -> bytes | None:
    try:
        os.lseek(descriptor, 0, os.SEEK_SET)
        return os.read(descriptor, 256 * 1024)
    except OSError:
        return None


def _cgroup_pids(raw: bytes | None) -> tuple[int, ...]:
    if raw is None:
        return ()
    values: list[int] = []
    for line in raw.splitlines():
        try:
            pid = int(line)
        except ValueError:
            continue
        if pid > 1:
            values.append(pid)
    return tuple(sorted(set(values)))


def _memory_events(raw: bytes | None) -> dict[str, int] | None:
    if raw is None:
        return None
    events: dict[str, int] = {}
    for line in raw.splitlines():
        key, separator, value = line.partition(b" ")
        if not separator:
            return None
        try:
            events[key.decode("ascii")] = int(value)
        except (UnicodeDecodeError, ValueError):
            return None
    return events if events.keys() >= OWNED_MEMORY_EVENT_KEYS else None


def _owned_unit_control(unit: str, action: str, timeout: float) -> dict[str, Any]:
    return _command(
        ["systemctl", "--user", action, unit],
        min(timeout, 10.0),
    )


class OwnedUserUnit:
    """Bind one cold CLI to a uniquely owned transient user service."""

    def __init__(
        self,
        *,
        cwd: str,
        environment: Mapping[str, str],
        command: Sequence[str],
        timeout: float,
    ) -> None:
        self.uid = os.getuid()
        self.unit = f"cortexfs-pi-benchmark-{self.uid}-{uuid.uuid4().hex}.service"
        self.gate = Path(cwd) / f".{self.unit}.gate"
        self.timeout = timeout
        clean_environment = [
            f"{key}={value}" for key, value in sorted(environment.items())
        ]
        self.command = [
            "systemd-run",
            "--user",
            "--quiet",
            "--wait",
            "--pipe",
            "--service-type=exec",
            f"--unit={self.unit}",
            "--property=MemoryAccounting=yes",
            "--property=KillMode=control-group",
            "--property=RemainAfterExit=yes",
            "--property=SendSIGKILL=yes",
            f"--working-directory={cwd}",
            "--",
            "/usr/bin/env",
            "-i",
            *clean_environment,
            "/bin/sh",
            "-c",
            'gate=$1; shift; while [ ! -f "$gate" ]; do sleep 0.005; done; exec "$@"',
            "cortexfs-pi-gate",
            str(self.gate),
            *command,
        ]

    async def preflight(self) -> bool:
        snapshot = await asyncio.to_thread(
            _owned_unit_snapshot, self.unit, self.timeout
        )
        return snapshot.get("LoadState") == "not-found"

    def _open_cgroup(self, control_group: str) -> dict[str, int] | None:
        path = _owned_cgroup_path(control_group, self.uid, self.unit)
        if path is None:
            return None
        directory = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        descriptors = {"directory": directory}
        try:
            for name in ("cgroup.procs", "memory.peak", "memory.events"):
                descriptors[name] = os.open(
                    name,
                    os.O_RDONLY | os.O_NOFOLLOW,
                    dir_fd=directory,
                )
        except OSError:
            for descriptor in descriptors.values():
                os.close(descriptor)
            return None
        return descriptors

    def _release(self) -> None:
        descriptor = os.open(
            self.gate,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o600,
        )
        os.close(descriptor)

    async def observe(
        self, wrapper: asyncio.subprocess.Process, stop_requested: asyncio.Event
    ) -> dict[str, object]:
        deadline = time.monotonic() + self.timeout
        initial: dict[str, object] | None = None
        final: dict[str, object] | None = None
        descriptors: dict[str, int] | None = None
        membership: tuple[int, ...] = ()
        foreign: tuple[int, ...] = ()
        issue: str | None = None
        stop_sent = False
        last_cgroup_peak: int | None = None
        last_events: dict[str, int] | None = None
        try:
            while time.monotonic() < deadline:
                snapshot = await asyncio.to_thread(
                    _owned_unit_snapshot, self.unit, self.timeout
                )
                if initial is None:
                    pid = snapshot.get("MainPID")
                    control_group = snapshot.get("ControlGroup")
                    invocation = snapshot.get("InvocationID")
                    if (
                        snapshot.get("Id") == self.unit
                        and isinstance(pid, int)
                        and pid > 1
                        and isinstance(control_group, str)
                        and control_group
                        and isinstance(invocation, str)
                        and re.fullmatch(r"[0-9a-fA-F]{32}", invocation)
                    ):
                        uids = await asyncio.to_thread(_process_uids, pid)
                        identity = await asyncio.to_thread(_process_identity, pid)
                        descriptors = self._open_cgroup(control_group)
                        if descriptors is None:
                            issue = "cannot open owned cgroup evidence"
                            break
                        membership = _cgroup_pids(
                            _read_cgroup_file(descriptors["cgroup.procs"])
                        )
                        foreign = tuple(
                            member
                            for member in membership
                            if _process_uids(member) != (self.uid,) * 4
                        )
                        if (
                            uids != (self.uid,) * 4
                            or pid not in membership
                            or foreign
                            or not identity.get("start_time")
                        ):
                            issue = "owned unit identity or membership mismatch"
                            break
                        initial = {
                            **snapshot,
                            "uid": self.uid,
                            "leader_start_time": identity["start_time"],
                            "membership": membership,
                        }
                        self._release()
                if descriptors is not None:
                    current_peak = _read_cgroup_file(descriptors["memory.peak"])
                    try:
                        parsed_peak = (
                            int(current_peak.strip()) if current_peak else None
                        )
                    except ValueError:
                        parsed_peak = None
                    if parsed_peak is not None:
                        last_cgroup_peak = parsed_peak
                    current_events = _memory_events(
                        _read_cgroup_file(descriptors["memory.events"])
                    )
                    if current_events is not None:
                        last_events = current_events
                if initial is not None and snapshot.get("InvocationID") not in {
                    initial.get("InvocationID"),
                    None,
                    "",
                }:
                    issue = "owned unit invocation identity changed"
                    break
                if stop_requested.is_set() and not stop_sent:
                    await asyncio.to_thread(
                        _owned_unit_control, self.unit, "stop", self.timeout
                    )
                    stop_sent = True
                if initial is not None and (
                    snapshot.get("SubState") in {"exited", "failed", "dead"}
                    or snapshot.get("ActiveState") in {"failed", "inactive"}
                ):
                    final = snapshot
                    break
                if wrapper.returncode is not None and initial is None:
                    issue = "owned user unit never became observable"
                    break
                await asyncio.sleep(OWNED_UNIT_POLL_SECONDS)
            else:
                issue = "owned user unit observation timed out"
            if final is None:
                await asyncio.to_thread(
                    _owned_unit_control, self.unit, "stop", self.timeout
                )
                final = await asyncio.to_thread(
                    _owned_unit_snapshot, self.unit, self.timeout
                )
            peak_raw = (
                _read_cgroup_file(descriptors["memory.peak"])
                if descriptors is not None
                else None
            )
            try:
                cgroup_peak = int(peak_raw.strip()) if peak_raw else None
            except ValueError:
                cgroup_peak = None
            if cgroup_peak is None:
                cgroup_peak = last_cgroup_peak
            events = (
                _memory_events(_read_cgroup_file(descriptors["memory.events"]))
                if descriptors is not None
                else None
            )
            if events is None:
                events = last_events
            final_pids = (
                _cgroup_pids(_read_cgroup_file(descriptors["cgroup.procs"]))
                if descriptors is not None
                else ()
            )
            if not stop_sent:
                stop_receipt = await asyncio.to_thread(
                    _owned_unit_control, self.unit, "stop", self.timeout
                )
            else:
                stop_receipt = {"success": True, "already_requested": True}
            reset_receipt = await asyncio.to_thread(
                _owned_unit_control, self.unit, "reset-failed", self.timeout
            )
            post = await asyncio.to_thread(
                _owned_unit_snapshot, self.unit, self.timeout
            )
            cleanup = {
                "stop": stop_receipt,
                "reset_failed": reset_receipt,
                "post_load_state": post.get("LoadState"),
                "cgroup_empty": not final_pids,
                "complete": bool(
                    stop_receipt.get("success")
                    and (
                        reset_receipt.get("success")
                        or post.get("LoadState") == "not-found"
                    )
                    and post.get("LoadState") == "not-found"
                    and not final_pids
                ),
            }
            systemd_peak = final.get("MemoryPeak") if final else None
            peak = cgroup_peak if isinstance(cgroup_peak, int) else systemd_peak
            complete = bool(
                issue is None
                and initial
                and final
                and final.get("InvocationID") == initial.get("InvocationID")
                and isinstance(peak, int)
                and peak >= 0
                and events is not None
                and not foreign
                and cleanup["complete"]
            )
            return {
                "unit": self.unit,
                "uid": self.uid,
                "initial": initial,
                "final": final,
                "memory_peak_bytes": peak,
                "cgroup_memory_peak_bytes": cgroup_peak,
                "memory_events": events,
                "initial_membership": membership,
                "foreign_pids": foreign,
                "final_membership": final_pids,
                "cleanup": cleanup,
                "issue": issue,
                "complete": complete,
            }
        except (OSError, ValueError) as error:
            await asyncio.to_thread(
                _owned_unit_control, self.unit, "stop", self.timeout
            )
            await asyncio.to_thread(
                _owned_unit_control, self.unit, "reset-failed", self.timeout
            )
            return {
                "unit": self.unit,
                "uid": self.uid,
                "issue": str(error),
                "complete": False,
            }
        finally:
            if descriptors is not None:
                for descriptor in descriptors.values():
                    os.close(descriptor)
            with suppress(FileNotFoundError):
                self.gate.unlink()


def _runtime_identity(
    ctx_path: str, agents: list[str], timeout: float
) -> dict[str, object]:
    command_timeout = min(timeout, 10.0)
    model_routes = {
        agent_name: str(
            _command([ctx_path, "cat", f"agent/{agent_name}.d/model"], command_timeout)[
                "stdout"
            ]
        ).strip()
        for agent_name in agents
    }
    model_efforts = {
        route: str(
            _command([ctx_path, "cat", f"model/{route}.d/effort"], command_timeout)[
                "stdout"
            ]
        ).strip()
        for route in sorted(set(model_routes.values()))
        if route
    }
    model_contracts: dict[str, dict[str, dict[str, object]]] = {}
    for route in sorted(set(model_routes.values())):
        if not route or not re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", route):
            continue
        control = f"model/{route}.d"
        model_contracts[route] = {
            name: _hashed_command(
                [ctx_path, "cat", f"{control}/{name}"], command_timeout
            )
            for name in (
                "id",
                "metadata.json",
                "driver",
                "cap",
                "effort",
                "default",
                "limit",
                "recommended",
                "compact",
                "session",
            )
        }
    agent_contracts: dict[str, dict[str, dict[str, object]]] = {}
    for agent_name in agents:
        control = f"agent/{agent_name}.d"
        agent_contracts[agent_name] = {
            "system": _hashed_command(
                [ctx_path, "cat", f"{control}/system.md"], command_timeout
            ),
            "prompt_template": _hashed_command(
                [ctx_path, "cat", f"{control}/prompt.template.md"], command_timeout
            ),
            "context_template": _hashed_command(
                [ctx_path, "cat", f"{control}/context.template.md"], command_timeout
            ),
            "tools": _hashed_command(
                [ctx_path, "agent", "tools", agent_name],
                command_timeout,
                retain_stdout=True,
            ),
            "root": _hashed_command(
                [ctx_path, "cat", f"{control}/root"], command_timeout
            ),
            "cwd": _hashed_command(
                [ctx_path, "cat", f"{control}/cwd"], command_timeout
            ),
            "mount": _hashed_command(
                [ctx_path, "cat", f"{control}/mount"], command_timeout
            ),
            "policy": _hashed_command(
                [ctx_path, "cat", f"{control}/policy"], command_timeout
            ),
            "perm": _hashed_command(
                [ctx_path, "cat", f"{control}/perm"], command_timeout
            ),
        }
    unit_configurations = {
        "mount": _unit_configuration_identity("cortexfs.service", command_timeout),
        "agents": {
            agent_name: _unit_configuration_identity(
                f"cortexfs-agent@{agent_name}.service", command_timeout
            )
            for agent_name in agents
        },
    }
    package = _command(["pacman", "-Q", "cortexfs-git"], command_timeout)
    repository = Path(__file__).resolve().parents[2]
    commit = _command(
        ["git", "-C", str(repository), "rev-parse", "HEAD"], command_timeout
    )
    return {
        "ctx_path": str(Path(ctx_path).expanduser().absolute()),
        "binary": _executable_identity(ctx_path, command_timeout),
        "package": package["stdout"].strip() if package["success"] else None,
        "commit": commit["stdout"].strip() if commit["success"] else None,
        "model_routes": model_routes,
        "model_efforts": model_efforts,
        "model_contracts": model_contracts,
        "agent_contracts": agent_contracts,
        "unit_configurations": unit_configurations,
    }


def _comparison_fingerprint(configuration: dict[str, object]) -> str:
    canonical = json.dumps(
        configuration, ensure_ascii=True, sort_keys=True, separators=(",", ":")
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _read_json_object(path: Path, limit: int = 1024 * 1024) -> dict[str, Any]:
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ValueError(f"cannot open JSON object: {path}") from error
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError(f"JSON object is not a bounded regular file: {path}")
        chunks: list[bytes] = []
        remaining = limit + 1
        while remaining > 0:
            chunk = os.read(descriptor, min(64 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        content = b"".join(chunks)
        if len(content) > limit:
            raise ValueError(f"JSON object exceeds {limit} bytes: {path}")
    except OSError as error:
        raise ValueError(f"cannot read JSON object: {path}") from error
    finally:
        os.close(descriptor)
    try:
        value = json.loads(content.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON object: {path}") from error
    if not isinstance(value, dict):
        raise ValueError(f"JSON value must be an object: {path}")
    return value


def _comparison_status(reference: Path | None, fingerprint: str) -> dict[str, object]:
    if reference is None:
        return {"reference": None, "equivalent": None}
    value = _read_json_object(reference)
    reference_fingerprint = value.get("fingerprint")
    equivalent = reference_fingerprint == fingerprint
    return {
        "reference": str(reference),
        "reference_fingerprint": reference_fingerprint,
        "equivalent": equivalent,
    }


def _require_cleanup(samples: Sequence[Mapping[str, object]]) -> None:
    if any(not bool(record.get("cleanup_success")) for record in samples):
        raise SystemExit("benchmark completed with session cleanup failures")


def _format_distribution(
    values: Mapping[str, object], *, decimals: int = 1, suffix: str = ""
) -> str:
    rendered: list[str] = []
    for name in ("mean", "p50", "p95"):
        value = values.get(name)
        rendered.append(
            f"{value:.{decimals}f}" if isinstance(value, (int, float)) else "n/a"
        )
    return "/".join(rendered) + suffix


def _report(summary: dict[str, Any]) -> str:
    lines = [
        "# CTX Agent Benchmark",
        "",
        f"Run: `{summary['run_id']}`",
        "",
        "| Agent | Requests | Runtime success | Exact accuracy | Latency mean/p50/p95 | Latency samples | TTFT mean/p50/p95 | TTFT samples | Input/output tokens | Token samples | cgroup memory peak mean/p50/p95 | Complete memory | tokens/s | chars/s |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for agent_name, metrics in summary["agents"].items():
        latency = metrics["latency_ms"]
        ttft = metrics["ttft_ms"]
        throughput = metrics["throughput"]
        peak_memory = metrics["peak_memory_bytes"]
        tokens_per_second = throughput["tokens_per_second"]
        chars_per_second = throughput["chars_per_second"]
        lines.append(
            f"| {agent_name} | {metrics['requests']} | {metrics['runtime_success_rate']:.1%} | "
            f"{metrics['exact_accuracy']:.1%} | "
            f"{_format_distribution(latency, suffix=' ms')} | "
            f"{metrics['latency_samples']}/{metrics['requests']} | "
            f"{_format_distribution(ttft, suffix=' ms')} | "
            f"{metrics['ttft_samples']}/{metrics['successful_samples']} | "
            f"{metrics['input_tokens']}/{metrics['output_tokens']} | "
            f"{metrics['token_samples']}/{metrics['requests']} | "
            f"{_format_distribution(peak_memory, decimals=0, suffix=' B')} | "
            f"{metrics['complete_memory_samples']}/{metrics['requests']} | "
            f"{tokens_per_second if tokens_per_second is not None else 'n/a'} | "
            f"{chars_per_second if chars_per_second is not None else 'n/a'} |"
        )
    lines.extend(
        [
            "",
            "## Comparison",
            "",
            f"Workload fingerprint: `{summary['fingerprint']}`",
            f"Treatment fingerprint: `{summary.get('treatment_fingerprint', 'legacy')}`",
            f"Equivalent workload reference: `{summary['comparison']['equivalent']}`",
            f"Samples SHA-256: `{summary.get('artifacts', {}).get('samples_jsonl_sha256', 'n/a')}`",
            "",
            "## Evidence status",
            "",
            f"`{summary.get('evidence_status', 'inconclusive')}`",
            f"Gates: `{json.dumps(summary.get('evidence_gates', {}), sort_keys=True)}`",
            "",
            "## Errors",
            "",
            "```json",
            json.dumps(summary["overall"]["errors"], ensure_ascii=False, indent=2),
            "```",
            "",
            "## Lifecycle",
            "",
            "```json",
            json.dumps(summary.get("lifecycle", {}), ensure_ascii=False, indent=2),
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Benchmark the live CortexFS agent system"
    )
    parser.add_argument("--agents", nargs="+", default=list(DEFAULT_AGENTS))
    _add_baseline_arguments(parser, Path("results"))
    parser.add_argument("--ctx-path", default="/usr/bin/ctx")
    parser.add_argument("--provider")
    parser.add_argument("--model")
    parser.add_argument("--thinking")
    parser.add_argument("--no-memory", action="store_true")
    args = parser.parse_args()
    if args.repeat < 1 or args.timeout <= 0:
        parser.error("--repeat and --timeout must be positive")
    model_controls = (args.provider, args.model, args.thinking)
    if any(model_controls) and not all(model_controls):
        parser.error("--provider, --model, and --thinking are required together")

    try:
        agents = _agents(args.agents)
        dataset = _dataset(args.dataset)
    except (OSError, UnicodeError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))
    try:
        health = _preflight(args.ctx_path, agents, args.timeout)
    except ValueError as error:
        parser.error(str(sanitize(str(error))))
    runtime_identity = _runtime_identity(args.ctx_path, agents, args.timeout)
    binary_identity = runtime_identity.get("binary")
    if (
        not isinstance(binary_identity, dict)
        or not binary_identity.get("sha256")
        or not binary_identity.get("version")
    ):
        parser.error("ctx executable identity is incomplete")
    if not _runtime_contracts_complete(runtime_identity):
        parser.error("ctx agent runtime contracts are incomplete")
    if args.provider is not None:
        try:
            _validate_model_controls(
                runtime_identity, args.provider, args.model, args.thinking
            )
        except ValueError as error:
            parser.error(str(error))
    workload: dict[str, object] = {
        "dataset": _dataset_identity(args.dataset, dataset),
        "repeat": args.repeat,
        "timeout": args.timeout,
        "provider": args.provider,
        "model": args.model,
        "thinking": args.thinking,
        "task_protocol": "single-prompt-final-answer-v1",
        "observation_plan": {
            "subjects_per_task": len(agents),
            "repeat_per_subject": args.repeat,
        },
    }
    runtime_snapshots_pre = {
        "mount": _unit_runtime_snapshot("cortexfs.service", args.timeout),
        "agents": {
            agent_name: _unit_runtime_snapshot(
                f"cortexfs-agent@{agent_name}.service", args.timeout
            )
            for agent_name in agents
        },
    }
    treatment: dict[str, object] = {
        "runtime": "ctx-agent-warm-socket",
        "agents": agents,
        "identity": runtime_identity,
        "retain_frames": args.retain_frames,
        "memory_measurement": "disabled"
        if args.no_memory
        else "systemd-unit-accounting",
        "price_table": None,
        "token_metric_semantics": "visible-input-output-v1",
        "benchmark_source": _benchmark_source_identity(["agents.py", "system.py"]),
    }
    configuration: dict[str, object] = {
        "workload": workload,
        "treatment": treatment,
    }
    fingerprint = _comparison_fingerprint(workload)
    treatment_fingerprint = _comparison_fingerprint(treatment)
    try:
        comparison = _validated_comparison(
            args.compare_summary, fingerprint, args.allow_mismatch
        )
    except ValueError as error:
        parser.error(str(error))
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    samples = asyncio.run(
        _samples(
            ctx_path=args.ctx_path,
            agents=agents,
            dataset=dataset,
            repeat=args.repeat,
            timeout=args.timeout,
            retain_frames=args.retain_frames,
            measure_memory=not args.no_memory,
        )
    )
    overall = _aggregate(samples)
    by_agent = {
        agent_name: _aggregate(
            [record for record in samples if record["agent"] == agent_name]
        )
        for agent_name in agents
    }
    sample_content = _jsonl_content(samples)
    post_runtime_identity = _runtime_identity(args.ctx_path, agents, args.timeout)
    runtime_snapshots_post = {
        "mount": _unit_runtime_snapshot("cortexfs.service", args.timeout),
        "agents": {
            agent_name: _unit_runtime_snapshot(
                f"cortexfs-agent@{agent_name}.service", args.timeout
            )
            for agent_name in agents
        },
    }
    identity_stable = post_runtime_identity == runtime_identity
    summary_value = {
        "run_id": run_id,
        "configuration": configuration,
        "workload": workload,
        "treatment": treatment,
        "runtime_identity_post": post_runtime_identity,
        "runtime_process_snapshots": {
            "pre": runtime_snapshots_pre,
            "post": runtime_snapshots_post,
        },
        "fingerprint": fingerprint,
        "treatment_fingerprint": treatment_fingerprint,
        "comparison": comparison,
        "repeat": args.repeat,
        "health": health,
        "overall": overall,
        "agents": by_agent,
        "lifecycle": {
            "preflight": health,
            "sessions": [record.get("cleanup") for record in samples],
        },
        "artifacts": {
            "samples_jsonl_sha256": hashlib.sha256(sample_content.encode()).hexdigest(),
            "samples_jsonl_bytes": len(sample_content.encode()),
            "immutable": True,
        },
        "evidence_gates": {
            "warmup_present": False,
            "runtime_identity_stable": identity_stable,
            "runtime_snapshots_complete_and_stable": (
                _runtime_snapshots_stable(runtime_snapshots_pre, runtime_snapshots_post)
            ),
            "protocol_evidence_complete": overall["complete_protocol_evidence_samples"]
            == overall["requests"],
            "process_group_cleanup_complete": overall[
                "complete_process_cleanup_samples"
            ]
            == overall["requests"],
            "runtime_process_identity_complete": overall[
                "complete_runtime_process_identity_samples"
            ]
            == overall["requests"],
            "complete_cgroup_memory": overall["complete_memory_samples"]
            == overall["requests"],
            "immutable_artifacts": True,
            "independent_review": False,
            "all_pass": False,
        },
        "evidence_status": "inconclusive-baseline-only-use-paired-run-for-claims",
    }
    summary = _sanitized_dict(summary_value, 8192)
    try:
        print(
            _publish_output(
                args.output_dir,
                run_id,
                {"samples.jsonl": sample_content},
                summary,
                _report(summary),
            )
        )
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot publish benchmark output: {error}")
    _require_cleanup(samples)


if __name__ == "__main__":
    main()
