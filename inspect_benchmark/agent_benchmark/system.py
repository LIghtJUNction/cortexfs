import argparse
import asyncio
import ctypes
import hashlib
import json
import os
import stat
import statistics
import subprocess
import time
import uuid
from collections import Counter
from dataclasses import replace
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from .agents import (
    CleanupReceipt,
    SessionBaseline,
    SessionReceipt,
    benchmark_session_name,
    run_ctx_agent,
    sanitize,
)


DEFAULT_AGENTS = ("architect", "coder", "reviewer", "worker")
ALLOWED_CATEGORIES = frozenset(
    {"general", "logic", "knowledge", "translation", "policy"}
)
ALLOWED_DIFFICULTIES = frozenset({"easy", "medium", "hard"})


def _command(command: list[str], timeout: float) -> dict[str, Any]:
    started = time.perf_counter()
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        value = {
            "command": command,
            "success": result.returncode == 0,
            "exit_code": result.returncode,
            "latency_seconds": time.perf_counter() - started,
            "stdout": result.stdout[-8192:],
            "stderr": result.stderr[-8192:],
        }
        sanitized = sanitize(value, 8192)
        assert isinstance(sanitized, dict)
        return sanitized
    except (OSError, subprocess.TimeoutExpired) as error:
        value = {
            "command": command,
            "success": False,
            "exit_code": -1,
            "latency_seconds": time.perf_counter() - started,
            "stdout": "",
            "stderr": str(error),
        }
        sanitized = sanitize(value, 8192)
        assert isinstance(sanitized, dict)
        return sanitized


def _preflight(ctx_path: str, agents: Sequence[str], timeout: float) -> dict[str, Any]:
    health = {
        "status": _command([ctx_path, "status"], min(timeout, 30.0)),
        "doctor": _command([ctx_path, "doctor"], min(timeout, 30.0)),
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
    doctor_lines = [line for line in str(doctor["stdout"]).splitlines() if line]
    if (
        not doctor["success"]
        or not doctor_lines
        or any(not line.startswith("ok ") for line in doctor_lines)
    ):
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
    sanitized = sanitize(health, 8192)
    assert isinstance(sanitized, dict)
    return sanitized


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
    elif current == session:
        refusal = "session is current"
    else:
        try:
            if _session_has_refs(agent_name, session, root=root, uid=uid):
                refusal = "session has child references"
        except (OSError, ValueError) as error:
            refusal = f"session reference inspection failed: {error}"
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
            if name not in DEFAULT_AGENTS:
                raise ValueError(f"unknown benchmark agent: {name}")
            result.append(name)
    return result or list(DEFAULT_AGENTS)


def _dataset(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    identifiers: set[str] = set()
    with path.open(encoding="utf-8") as source:
        for number, line in enumerate(source, 1):
            if not line.strip():
                continue
            value = json.loads(line)
            if not isinstance(value, dict):
                raise ValueError(f"invalid dataset record at line {number}")
            identifier = value.get("id")
            prompt = value.get("prompt")
            target = value.get("target")
            category = value.get("category", "general")
            difficulty = value.get("difficulty", "easy")
            for field, field_value in (
                ("id", identifier),
                ("prompt", prompt),
                ("target", target),
            ):
                if not isinstance(field_value, str) or not field_value.strip():
                    raise ValueError(f"invalid dataset {field} at line {number}")
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


def _percentile(values: list[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    weight = position - lower
    return ordered[lower] * (1.0 - weight) + ordered[upper] * weight


def _timing(values: list[float]) -> dict[str, float | None]:
    milliseconds = [value * 1000.0 for value in values]
    return {
        "mean": statistics.fmean(milliseconds) if milliseconds else None,
        "p50": _percentile(milliseconds, 0.50),
        "p95": _percentile(milliseconds, 0.95),
    }


def _aggregate(records: list[dict[str, Any]]) -> dict[str, Any]:
    successes = [record for record in records if record["runtime_success"]]
    latencies = [float(record["latency_seconds"]) for record in successes]
    ttfts = [
        float(record["ttft_seconds"])
        for record in successes
        if record["ttft_seconds"] is not None
    ]
    input_tokens = sum(int(record["input_tokens"]) for record in successes)
    output_tokens = sum(int(record["output_tokens"]) for record in successes)
    token_samples = sum(int(record["output_tokens"]) > 0 for record in successes)
    output_chars = sum(len(str(record["completion"])) for record in successes)
    elapsed = sum(float(record["latency_seconds"]) for record in successes)
    errors = Counter(
        str(record["error_code"] or f"exit_{record['exit_code']}")
        for record in records
        if not record["runtime_success"]
    )
    throughput = {
        "tokens_per_second": output_tokens / elapsed
        if elapsed and token_samples
        else None,
        "chars_per_second": output_chars / elapsed if elapsed else None,
    }
    total = len(records)
    return {
        "requests": total,
        "runtime_successes": len(successes),
        "runtime_success_rate": len(successes) / total if total else 0.0,
        "exact_matches": sum(bool(record["exact_match"]) for record in records),
        "exact_accuracy": sum(bool(record["exact_match"]) for record in records) / total
        if total
        else 0.0,
        "errors": dict(sorted(errors.items())),
        "latency_ms": _timing(latencies),
        "latency_samples": len(latencies),
        "ttft_ms": _timing(ttfts),
        "ttft_samples": len(ttfts),
        "token_samples": token_samples,
        "successful_samples": len(successes),
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "output_chars": output_chars,
        "throughput": throughput,
    }


async def _samples(
    *,
    ctx_path: str,
    agents: list[str],
    dataset: list[dict[str, Any]],
    repeat: int,
    timeout: float,
    retain_frames: bool = False,
) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for agent_name in agents:
        for epoch in range(repeat):
            for index, sample in enumerate(dataset):
                for _attempt in range(8):
                    session = benchmark_session_name("benchmark", agent_name)
                    baseline = await asyncio.to_thread(
                        _session_baseline, agent_name, session
                    )
                    if baseline.absent:
                        break
                else:
                    raise ValueError(
                        f"cannot allocate a unique benchmark session for {agent_name}"
                    )
                try:
                    result = await run_ctx_agent(
                        ctx_path=ctx_path,
                        agent_name=agent_name,
                        session=session,
                        prompt=str(sample["prompt"]),
                        timeout=timeout,
                        retain_frames=retain_frames,
                    )
                except (asyncio.CancelledError, KeyboardInterrupt):
                    await asyncio.to_thread(
                        _archive_session, ctx_path, agent_name, baseline, timeout
                    )
                    raise
                session_receipt = await asyncio.to_thread(
                    _archive_session,
                    ctx_path,
                    agent_name,
                    baseline,
                    timeout,
                )
                result = replace(
                    result,
                    cleanup=CleanupReceipt(session_receipt, result.cancellation),
                )
                record = result.metadata()
                sample_metadata = sanitize(
                    {
                        "sample_id": sample.get("id", f"sample-{index + 1}"),
                        "category": sample.get("category", "general"),
                        "difficulty": sample.get("difficulty", "easy"),
                        "epoch": epoch + 1,
                        "target": str(sample["target"]),
                        "completion": result.completion,
                        "exact_match": result.completion.strip()
                        == str(sample["target"]).strip(),
                        "cleanup_success": session_receipt.archived,
                    }
                )
                assert isinstance(sample_metadata, dict)
                record.update(sample_metadata)
                output.append(record)
    return output


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


class OutputDirectory:
    """Own the output and run directory descriptors for safe publication."""

    def __init__(self, root_fd: int, run_fd: int, path: Path) -> None:
        self._root_fd = root_fd
        self._run_fd = run_fd
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
                try:
                    os.mkdir(component, mode=0o700, dir_fd=current_fd)
                except FileExistsError:
                    pass
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
            os.fsync(self._run_fd)
        finally:
            if not published:
                try:
                    os.unlink(temporary, dir_fd=self._run_fd)
                except FileNotFoundError:
                    pass


def _runtime_identity(
    ctx_path: str, agents: list[str], timeout: float
) -> dict[str, object]:
    model_routes = {
        agent_name: _command(
            [ctx_path, "cat", f"agent/{agent_name}.d/model"], min(timeout, 10.0)
        )["stdout"].strip()
        for agent_name in agents
    }
    package = _command(["pacman", "-Q", "cortexfs-git"], min(timeout, 10.0))
    repository = Path(__file__).resolve().parents[2]
    commit = _command(
        ["git", "-C", str(repository), "rev-parse", "HEAD"], min(timeout, 10.0)
    )
    return {
        "ctx_path": str(Path(ctx_path).expanduser().absolute()),
        "package": package["stdout"].strip() if package["success"] else None,
        "commit": commit["stdout"].strip() if commit["success"] else None,
        "model_routes": model_routes,
    }


def _comparison_fingerprint(configuration: dict[str, object]) -> str:
    canonical = json.dumps(
        configuration, ensure_ascii=True, sort_keys=True, separators=(",", ":")
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _comparison_status(reference: Path | None, fingerprint: str) -> dict[str, object]:
    if reference is None:
        return {"reference": None, "equivalent": None}
    try:
        value = json.loads(reference.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read comparison summary: {reference}") from error
    if not isinstance(value, dict):
        raise ValueError(f"invalid comparison summary: {reference}")
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


def _report(summary: dict[str, Any]) -> str:
    lines = [
        "# CTX Agent Benchmark",
        "",
        f"Run: `{summary['run_id']}`",
        "",
        "| Agent | Requests | Runtime success | Exact accuracy | Latency mean/p50/p95 | Latency samples | TTFT mean/p50/p95 | TTFT samples | Input/output tokens | Token samples | tokens/s | chars/s |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for agent_name, metrics in summary["agents"].items():
        latency = metrics["latency_ms"]
        ttft = metrics["ttft_ms"]
        throughput = metrics["throughput"]
        tokens_per_second = throughput["tokens_per_second"]
        chars_per_second = throughput["chars_per_second"]
        lines.append(
            f"| {agent_name} | {metrics['requests']} | {metrics['runtime_success_rate']:.1%} | "
            f"{metrics['exact_accuracy']:.1%} | "
            f"{latency['mean'] or 0:.1f}/{latency['p50'] or 0:.1f}/{latency['p95'] or 0:.1f} ms | "
            f"{metrics['latency_samples']}/{metrics['requests']} | "
            f"{ttft['mean'] or 0:.1f}/{ttft['p50'] or 0:.1f}/{ttft['p95'] or 0:.1f} ms | "
            f"{metrics['ttft_samples']}/{metrics['successful_samples']} | "
            f"{metrics['input_tokens']}/{metrics['output_tokens']} | "
            f"{metrics['token_samples']}/{metrics['successful_samples']} | "
            f"{tokens_per_second if tokens_per_second is not None else 'n/a'} | "
            f"{chars_per_second if chars_per_second is not None else 'n/a'} |"
        )
    lines.extend(
        [
            "",
            "## Comparison",
            "",
            f"Fingerprint: `{summary['fingerprint']}`",
            f"Equivalent reference: `{summary['comparison']['equivalent']}`",
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
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path(__file__).with_name("data") / "agent_benchmark.jsonl",
    )
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument("--output-dir", type=Path, default=Path("results"))
    parser.add_argument("--ctx-path", default="/usr/bin/ctx")
    parser.add_argument("--compare-summary", type=Path)
    parser.add_argument("--allow-mismatch", action="store_true")
    parser.add_argument("--retain-frames", action="store_true")
    args = parser.parse_args()
    if args.repeat < 1 or args.timeout <= 0:
        parser.error("--repeat and --timeout must be positive")

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
    configuration: dict[str, object] = {
        "dataset": _dataset_identity(args.dataset, dataset),
        "agents": agents,
        "repeat": args.repeat,
        "timeout": args.timeout,
        "runtime": runtime_identity,
        "retain_frames": args.retain_frames,
    }
    fingerprint = _comparison_fingerprint(configuration)
    try:
        comparison = _comparison_status(args.compare_summary, fingerprint)
    except ValueError as error:
        parser.error(str(error))
    if comparison["equivalent"] is False and not args.allow_mismatch:
        parser.error("comparison summary configuration does not match this run")
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    samples = asyncio.run(
        _samples(
            ctx_path=args.ctx_path,
            agents=agents,
            dataset=dataset,
            repeat=args.repeat,
            timeout=args.timeout,
            retain_frames=args.retain_frames,
        )
    )
    by_agent = {
        agent_name: _aggregate(
            [record for record in samples if record["agent"] == agent_name]
        )
        for agent_name in agents
    }
    summary_value = {
        "run_id": run_id,
        "configuration": configuration,
        "fingerprint": fingerprint,
        "comparison": comparison,
        "repeat": args.repeat,
        "health": health,
        "overall": _aggregate(samples),
        "agents": by_agent,
        "lifecycle": {
            "preflight": health,
            "sessions": [record.get("cleanup") for record in samples],
        },
    }
    summary = sanitize(summary_value, 8192)
    assert isinstance(summary, dict)
    sample_content = "".join(
        json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n"
        for record in samples
    )
    try:
        with OutputDirectory.create(args.output_dir, run_id) as output:
            output.write("samples.jsonl", sample_content)
            output.write(
                "summary.json",
                json.dumps(summary, ensure_ascii=False, indent=2, sort_keys=True)
                + "\n",
            )
            output.write("report.md", _report(summary))
            print(output.path)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot publish benchmark output: {error}")
    _require_cleanup(samples)


if __name__ == "__main__":
    main()
