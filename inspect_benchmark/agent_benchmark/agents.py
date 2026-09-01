# pyright: reportMissingImports=false
import asyncio
import json
import os
import re
import signal
import time
import uuid
from collections.abc import Mapping
from contextlib import suppress
from dataclasses import asdict, dataclass
from typing import Any, Protocol, cast

from inspect_ai.agent import (
    Agent,
    AgentState,
    agent,
    agent_bridge,
    sandbox_agent_bridge,
)
from inspect_ai.log import transcript
from inspect_ai.model import (
    ModelOutput,
    ModelUsage,
    messages_to_openai,
    user_prompt,
)
from inspect_ai.util import sandbox

MAX_LINE_BYTES = 256 * 1024
MAX_STDOUT_BYTES = 8 * 1024 * 1024
MAX_STDERR_BYTES = 64 * 1024
MAX_FRAMES = 4096
MAX_RETAINED_FRAME_BYTES = 256 * 1024
MAX_ERROR_BYTES = 4096
SECRET_KEYS = frozenset(
    {"authorization", "api_key", "apikey", "token", "secret", "password", "cookie"}
)
SECRET_VALUE_PATTERN = re.compile(
    r"(?i)\b((?:[a-z0-9]+[_-])*(?:authorization|api[_-]?key|token|secret|password|cookie))"
    r"(\s*[:=]\s*)([^\s,;]+)"
)
BEARER_PATTERN = re.compile(r"(?i)\bbearer\s+[^\s,;]+")
OPENAI_TOKEN_PATTERN = re.compile(r"(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]{20,}")
JWT_PATTERN = re.compile(
    r"(?<![A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\."
    r"[A-Za-z0-9_-]{10,}"
)


@dataclass(frozen=True)
class RunIdentity:
    client_id: str
    canonical_run_id: str | None
    session: str


@dataclass(frozen=True)
class CancellationReceipt:
    requested_run: str | None
    session: str
    server_exit_code: int | None
    terminal_status: str | None
    local_signal: str | None
    detail: str | None = None
    helper_process_cleanup: "ProcessCleanupReceipt | None" = None


@dataclass(frozen=True)
class SessionBaseline:
    session: str
    absent: bool
    current: str
    sessions: tuple[str, ...]


@dataclass(frozen=True)
class SessionReceipt:
    session: str
    baseline: SessionBaseline
    created_by_benchmark: bool
    terminal_state: str | None
    archive_allowed: bool
    archived: bool
    current_after: str | None = None
    candidates: tuple[str, ...] = ()
    detail: str | None = None


@dataclass(frozen=True)
class CleanupReceipt:
    session: SessionReceipt
    cancellation: CancellationReceipt | None = None
    errors: tuple[str, ...] = ()


class CommandRunner(Protocol):
    async def run(self, *command: str, timeout: float) -> tuple[int, str, str]:
        raise NotImplementedError


@dataclass(frozen=True)
class ProcessCleanupReceipt:
    spawned: bool
    pgid: int
    leader_start_time: int | None
    signal: str | None
    term_sent: bool
    kill_sent: bool
    verified_empty: bool
    remaining_pids: tuple[int, ...]
    detail: str | None


@dataclass(frozen=True)
class CtxRunResult:
    agent: str
    session: str
    completion: str
    frames: tuple[dict[str, Any], ...]
    runtime_success: bool
    exit_code: int
    timed_out: bool
    latency_seconds: float
    ttft_seconds: float | None
    input_tokens: int
    output_tokens: int
    usage_available: bool
    raw_frames_complete: bool
    dropped_frame_count: int
    dropped_frame_bytes: int
    redacted_field_count: int
    redacted_field_bytes: int
    truncated_field_count: int
    truncated_field_bytes: int
    done_status: str | None
    error_code: str | None
    error_message: str | None
    stderr: str
    identity: RunIdentity
    cancellation: CancellationReceipt | None
    process_cleanup: ProcessCleanupReceipt
    cleanup: CleanupReceipt | None

    def metadata(self, include_frames: bool = True) -> dict[str, Any]:
        value: dict[str, Any] = {
            "agent": self.agent,
            "session": self.session,
            "runtime_success": self.runtime_success,
            "exit_code": self.exit_code,
            "timed_out": self.timed_out,
            "latency_seconds": self.latency_seconds,
            "ttft_seconds": self.ttft_seconds,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "comparable_tokens": self.input_tokens + self.output_tokens
            if self.usage_available
            else None,
            "total_tokens": self.input_tokens + self.output_tokens
            if self.usage_available
            else None,
            "usage_semantics": "visible-input-output-v1",
            "usage_available": self.usage_available,
            "raw_frames_complete": self.raw_frames_complete,
            "dropped_frame_count": self.dropped_frame_count,
            "dropped_frame_bytes": self.dropped_frame_bytes,
            "redacted_field_count": self.redacted_field_count,
            "redacted_field_bytes": self.redacted_field_bytes,
            "truncated_field_count": self.truncated_field_count,
            "truncated_field_bytes": self.truncated_field_bytes,
            "done_status": self.done_status,
            "error_code": self.error_code,
            "error_message": self.error_message,
            "stderr": self.stderr,
            "identity": asdict(self.identity),
            "cancellation": asdict(self.cancellation) if self.cancellation else None,
            "process_cleanup": asdict(self.process_cleanup),
            "cleanup": asdict(self.cleanup) if self.cleanup else None,
        }
        sanitized = sanitize(value, MAX_STDERR_BYTES)
        if not isinstance(sanitized, dict):
            raise TypeError("sanitized ctx metadata must remain an object")
        if include_frames:
            sanitized["frames"] = list(self.frames)
        return sanitized


def _content_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    text: list[str] = []
    for part in content:
        if isinstance(part, str):
            text.append(part)
        elif isinstance(part, dict):
            value = part.get("text", part.get("content"))
            if isinstance(value, str):
                text.append(value)
    return "".join(text)


def benchmark_session_name(prefix: str, agent_name: str) -> str:
    stem = re.sub(r"[^a-z0-9-]+", "-", f"{prefix}-{agent_name}".lower())
    stem = stem.strip("-")[:40] or "inspect"
    return f"{stem}-{uuid.uuid4().hex[:12]}"


def _bounded_text(value: str, limit: int = MAX_ERROR_BYTES) -> str:
    encoded = value.encode("utf-8", errors="replace")[:limit]
    return encoded.decode("utf-8", errors="replace")


def sanitize(value: object, limit: int = MAX_ERROR_BYTES) -> object:
    if isinstance(value, dict):
        return {
            str(key): "[REDACTED]" if _secret_key(str(key)) else sanitize(item, limit)
            for key, item in value.items()
        }
    if isinstance(value, tuple):
        return tuple(sanitize(item, limit) for item in value)
    if isinstance(value, list):
        return [sanitize(item, limit) for item in value]
    if isinstance(value, str):
        redacted = BEARER_PATTERN.sub("Bearer [REDACTED]", value)
        redacted = OPENAI_TOKEN_PATTERN.sub("[REDACTED_TOKEN]", redacted)
        redacted = JWT_PATTERN.sub("[REDACTED_JWT]", redacted)
        redacted = SECRET_VALUE_PATTERN.sub(r"\1\2[REDACTED]", redacted)
        return _bounded_text(redacted, limit)
    return value


def _redact(value: object, stats: dict[str, int] | None = None) -> object:
    counters = stats if stats is not None else {}
    if isinstance(value, dict):
        mapping: dict[str, object] = {}
        for key, item in value.items():
            name = str(key)
            if _secret_key(name):
                counters["fields"] = counters.get("fields", 0) + 1
                counters["bytes"] = counters.get("bytes", 0) + len(
                    str(item).encode("utf-8", errors="replace")
                )
                mapping[name] = "[REDACTED]"
            else:
                mapping[name] = _redact(item, counters)
        return mapping
    if isinstance(value, tuple):
        return tuple(_redact(item, counters) for item in value)
    if isinstance(value, list):
        return [_redact(item, counters) for item in value]
    if isinstance(value, str):
        text = BEARER_PATTERN.sub("Bearer [REDACTED]", value)
        text = OPENAI_TOKEN_PATTERN.sub("[REDACTED_TOKEN]", text)
        text = JWT_PATTERN.sub("[REDACTED_JWT]", text)
        text = SECRET_VALUE_PATTERN.sub(r"\1\2[REDACTED]", text)
        if text != value:
            counters["fields"] = counters.get("fields", 0) + 1
            counters["bytes"] = counters.get("bytes", 0) + len(
                value.encode("utf-8", errors="replace")
            )
        return text
    return value


def _frame_evidence(
    frame: dict[str, Any], observed_seconds: float
) -> tuple[dict[str, Any], int, int, int]:
    redaction: dict[str, int] = {}
    evidence = _redact(frame, redaction)
    if not isinstance(evidence, dict):
        raise ValueError("redacted protocol frame must remain an object")
    evidence["_observer_elapsed_seconds"] = observed_seconds
    evidence_bytes = len(json.dumps(evidence).encode("utf-8"))
    return (
        evidence,
        evidence_bytes,
        redaction.get("fields", 0),
        redaction.get("bytes", 0),
    )


def _secret_key(key: str) -> bool:
    normalized = key.lower().replace("-", "_")
    return normalized in SECRET_KEYS or any(
        normalized.endswith(f"_{name}") for name in SECRET_KEYS
    )


def _usage(value: object, field: str) -> int:
    if value is None:
        return 0
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"invalid {field}")
    return value


class SubprocessCommandRunner:
    def __init__(self) -> None:
        self.last_cleanup = _empty_process_cleanup()

    async def run(self, *command: str, timeout: float) -> tuple[int, str, str]:
        try:
            process = await asyncio.create_subprocess_exec(
                *command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                start_new_session=True,
            )
        except OSError as error:
            self.last_cleanup = _empty_process_cleanup()
            return -1, "", str(sanitize(str(error)))
        process_start_time = _process_group_members(process.pid).get(process.pid)
        try:
            stdout, stderr = await asyncio.wait_for(process.communicate(), timeout)
        except asyncio.TimeoutError:
            self.last_cleanup = await _cleanup_process_group(
                process, process_start_time
            )
            detail = (
                "command timed out"
                if process_cleanup_complete(self.last_cleanup)
                else "command timed out; process cleanup was not verified"
            )
            return -1, "", detail
        self.last_cleanup = await _cleanup_process_group(process, process_start_time)
        if not process_cleanup_complete(self.last_cleanup):
            return -1, "", "command process cleanup was not verified"
        return (
            process.returncode or 0,
            str(sanitize(stdout.decode("utf-8", errors="replace"), MAX_STDERR_BYTES)),
            str(sanitize(stderr.decode("utf-8", errors="replace"), MAX_STDERR_BYTES)),
        )


def process_cleanup_complete(
    value: ProcessCleanupReceipt | Mapping[str, object],
) -> bool:
    if isinstance(value, ProcessCleanupReceipt):
        spawned = value.spawned
        pgid = value.pgid
        start_time = value.leader_start_time
        verified = value.verified_empty
    else:
        spawned = bool(value.get("spawned"))
        pgid = value.get("pgid")
        start_time = value.get("leader_start_time")
        verified = bool(value.get("verified_empty"))
    if not spawned:
        return verified
    return (
        isinstance(pgid, int)
        and not isinstance(pgid, bool)
        and pgid > 1
        and isinstance(start_time, int)
        and not isinstance(start_time, bool)
        and start_time > 0
        and verified
    )


def record_process_cleanup_complete(record: Mapping[str, object]) -> bool:
    primary = record.get("process_cleanup")
    if not isinstance(primary, Mapping) or not process_cleanup_complete(primary):
        return False
    cancellation = record.get("cancellation")
    if cancellation is None:
        return True
    if not isinstance(cancellation, Mapping):
        return False
    helper = cancellation.get("helper_process_cleanup")
    return isinstance(helper, Mapping) and process_cleanup_complete(helper)


def _process_group_members(pgid: int) -> dict[int, int]:
    members: dict[int, int] = {}
    try:
        entries = os.scandir("/proc")
    except OSError:
        return members
    with entries:
        for entry in entries:
            if not entry.name.isdigit():
                continue
            descriptor = -1
            try:
                descriptor = os.open(
                    f"/proc/{entry.name}/stat",
                    os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW,
                )
                raw = os.read(descriptor, 4096)
                tail = raw.rsplit(b") ", 1)[1].split()
                if int(tail[2]) == pgid:
                    members[int(entry.name)] = int(tail[19])
            except (OSError, IndexError, ValueError):
                continue
            finally:
                if descriptor >= 0:
                    os.close(descriptor)
    return members


async def _wait_for_empty_group(pgid: int, timeout: float) -> dict[int, int]:
    deadline = time.monotonic() + timeout
    while True:
        members = _process_group_members(pgid)
        if not members or time.monotonic() >= deadline:
            return members
        await asyncio.sleep(0.02)


async def _cleanup_process_group(
    process: asyncio.subprocess.Process,
    leader_start_time: int | None = None,
) -> ProcessCleanupReceipt:
    pgid = process.pid
    initial = _process_group_members(pgid)
    leader_start = leader_start_time or initial.get(process.pid)
    current_leader_start = initial.get(process.pid)
    if (
        pgid <= 1
        or leader_start is None
        or (current_leader_start is not None and current_leader_start != leader_start)
    ):
        return ProcessCleanupReceipt(
            spawned=True,
            pgid=pgid,
            leader_start_time=leader_start,
            signal=None,
            term_sent=False,
            kill_sent=False,
            verified_empty=False,
            remaining_pids=tuple(sorted(initial)),
            detail="process group ownership could not be verified",
        )
    term_sent = False
    kill_sent = False
    detail: str | None = None
    signal_name: str | None = None
    if initial or process.returncode is None:
        try:
            os.killpg(pgid, signal.SIGTERM)
            term_sent = True
            signal_name = "TERM"
        except ProcessLookupError:
            pass
        except OSError as error:
            detail = str(sanitize(str(error)))
    if process.returncode is None:
        with suppress(asyncio.TimeoutError):
            await asyncio.wait_for(process.wait(), timeout=0.5)
    remaining = await _wait_for_empty_group(pgid, 0.5)
    if remaining:
        try:
            os.killpg(pgid, signal.SIGKILL)
            kill_sent = True
            signal_name = "KILL"
        except ProcessLookupError:
            pass
        except OSError as error:
            detail = str(sanitize(str(error)))
        if process.returncode is None:
            with suppress(asyncio.TimeoutError):
                await asyncio.wait_for(process.wait(), timeout=1.0)
        remaining = await _wait_for_empty_group(pgid, 1.0)
    verified_empty = (
        leader_start is not None
        and pgid > 1
        and not remaining
        and process.returncode is not None
    )
    if not verified_empty and detail is None:
        detail = "process group or leader remains after bounded cleanup"
    return ProcessCleanupReceipt(
        spawned=True,
        pgid=pgid,
        leader_start_time=leader_start,
        signal=signal_name,
        term_sent=term_sent,
        kill_sent=kill_sent,
        verified_empty=verified_empty,
        remaining_pids=tuple(sorted(remaining)),
        detail=detail,
    )


def _empty_process_cleanup(pgid: int = -1) -> ProcessCleanupReceipt:
    return ProcessCleanupReceipt(
        spawned=False,
        pgid=pgid,
        leader_start_time=None,
        signal=None,
        term_sent=False,
        kill_sent=False,
        verified_empty=True,
        remaining_pids=(),
        detail=None,
    )


async def run_ctx_agent(
    *,
    ctx_path: str,
    agent_name: str,
    session: str,
    prompt: str,
    timeout: float,
    retain_frames: bool = False,
    command_runner: CommandRunner | None = None,
) -> CtxRunResult:
    """Run one ctx request with bounded protocol parsing and exact cancellation."""

    started = time.perf_counter()
    client_id = f"benchmark-{uuid.uuid4().hex}"
    identity = RunIdentity(client_id=client_id, canonical_run_id=None, session=session)
    cancellation: CancellationReceipt | None = None
    try:
        process = await asyncio.create_subprocess_exec(
            ctx_path,
            "agent",
            "send",
            agent_name,
            "--session",
            session,
            "--raw",
            prompt,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            start_new_session=True,
        )
    except OSError as error:
        latency = time.perf_counter() - started
        return CtxRunResult(
            agent=agent_name,
            session=session,
            completion="",
            frames=(),
            runtime_success=False,
            exit_code=-1,
            timed_out=False,
            latency_seconds=latency,
            ttft_seconds=None,
            input_tokens=0,
            output_tokens=0,
            usage_available=False,
            raw_frames_complete=False,
            dropped_frame_count=0,
            dropped_frame_bytes=0,
            redacted_field_count=0,
            redacted_field_bytes=0,
            truncated_field_count=0,
            truncated_field_bytes=0,
            done_status=None,
            error_code="spawn_error",
            error_message=str(sanitize(str(error))),
            stderr="",
            identity=identity,
            cancellation=None,
            process_cleanup=_empty_process_cleanup(),
            cleanup=None,
        )

    process_start_time = _process_group_members(process.pid).get(process.pid)
    stdout = cast(asyncio.StreamReader, process.stdout)
    stderr = cast(asyncio.StreamReader, process.stderr)
    stderr_task = asyncio.create_task(stderr.read(MAX_STDERR_BYTES + 1))
    frames: list[dict[str, Any]] = []
    deltas: list[str] = []
    assistant_messages: list[str] = []
    ttft: float | None = None
    input_tokens = 0
    output_tokens = 0
    usage_available = False
    done_status: str | None = None
    error_code: str | None = None
    error_message: str | None = None
    last_recoverable_error: tuple[str, str] | None = None
    canonical_run_id: str | None = None
    stdout_bytes = 0
    retained_bytes = 0
    terminal_seen = False
    frame_count = 0
    dropped_frame_count = 0
    dropped_frame_bytes = 0
    redacted_field_count = 0
    redacted_field_bytes = 0

    async def collect() -> int:
        nonlocal ttft, input_tokens, output_tokens, usage_available, done_status
        nonlocal error_code, error_message, canonical_run_id, stdout_bytes
        nonlocal retained_bytes, terminal_seen, frame_count, last_recoverable_error
        nonlocal dropped_frame_count, dropped_frame_bytes
        nonlocal redacted_field_count, redacted_field_bytes
        while True:
            raw_line = await stdout.readline()
            if not raw_line:
                break
            stdout_bytes += len(raw_line)
            if len(raw_line) > MAX_LINE_BYTES:
                raise ValueError("JSONL line exceeds byte limit")
            if stdout_bytes > MAX_STDOUT_BYTES:
                raise ValueError("stdout exceeds byte limit")
            frame_count += 1
            if frame_count > MAX_FRAMES:
                raise ValueError("frame count exceeds limit")
            observed_seconds = time.perf_counter() - started
            try:
                frame = json.loads(raw_line.decode("utf-8"))
            except UnicodeDecodeError as error:
                raise ValueError("invalid UTF-8 frame") from error
            except json.JSONDecodeError as error:
                raise ValueError("invalid JSON frame") from error
            if not isinstance(frame, dict):
                raise ValueError("JSON frame must be an object")
            if terminal_seen:
                dropped_frame_count += 1
                dropped_frame_bytes += len(raw_line)
                continue
            run = frame.get("run")
            if isinstance(run, str) and run:
                if canonical_run_id is not None and canonical_run_id != run:
                    raise ValueError("conflicting canonical run ids")
                canonical_run_id = run
            evidence, evidence_bytes, fields, field_bytes = _frame_evidence(
                frame, observed_seconds
            )
            redacted_field_count += fields
            redacted_field_bytes += field_bytes
            if (
                retain_frames
                and retained_bytes + evidence_bytes <= MAX_RETAINED_FRAME_BYTES
            ):
                frames.append(evidence)
                retained_bytes += evidence_bytes
            elif retain_frames:
                dropped_frame_count += 1
                dropped_frame_bytes += evidence_bytes
            elif frame.get("type") in {
                "start",
                "done",
                "error",
                "usage",
            }:
                frames.append(evidence)
            kind = frame.get("type")
            if kind == "delta" and isinstance(frame.get("text"), str):
                text = frame["text"]
                if text.strip() and ttft is None:
                    ttft = time.perf_counter() - started
                deltas.append(text)
            elif kind == "message" and frame.get("role") == "assistant":
                text = _content_text(frame.get("content"))
                if text.strip() and ttft is None:
                    ttft = time.perf_counter() - started
                if text:
                    assistant_messages.append(text)
            elif kind == "usage":
                if "input_tokens" not in frame or "output_tokens" not in frame:
                    usage_available = False
                    raise ValueError("usage frame is missing required token fields")
                try:
                    next_input = _usage(frame["input_tokens"], "input_tokens")
                    next_output = _usage(frame["output_tokens"], "output_tokens")
                except ValueError:
                    usage_available = False
                    raise
                input_tokens += next_input
                output_tokens += next_output
                usage_available = True
            elif kind == "done" and isinstance(frame.get("status"), str):
                done_status = frame["status"]
                terminal_seen = True
            elif kind == "error":
                diagnostic = (
                    str(sanitize(str(frame.get("code") or "agent_error"))),
                    str(sanitize(str(frame.get("message") or "ctx agent failed"))),
                )
                recoverable = frame.get("recoverable", False)
                if not isinstance(recoverable, bool):
                    raise ValueError("error recoverable flag must be boolean")
                if recoverable:
                    last_recoverable_error = diagnostic
                else:
                    error_code, error_message = diagnostic
        return await process.wait()

    timed_out = False
    local_signal: str | None = None
    process_cleanup: ProcessCleanupReceipt | None = None
    runner = command_runner or SubprocessCommandRunner()

    async def request_server_cancel() -> None:
        nonlocal cancellation
        if canonical_run_id is None:
            return
        outcome = (
            await asyncio.gather(
                runner.run(
                    ctx_path,
                    "agent",
                    "cancel",
                    agent_name,
                    "--session",
                    session,
                    "--raw",
                    canonical_run_id,
                    timeout=min(timeout, 5.0),
                ),
                return_exceptions=True,
            )
        )[0]
        helper_cleanup = getattr(runner, "last_cleanup", None)
        if not isinstance(helper_cleanup, ProcessCleanupReceipt):
            helper_cleanup = None
        if isinstance(outcome, BaseException):
            cancellation = CancellationReceipt(
                canonical_run_id,
                session,
                -1,
                done_status,
                None,
                str(sanitize(str(outcome))) or None,
                helper_cleanup,
            )
            return
        cancel_code, _stdout, cancel_stderr = outcome
        cancellation = CancellationReceipt(
            canonical_run_id,
            session,
            cancel_code,
            done_status,
            None,
            str(sanitize(cancel_stderr)) or None,
            helper_cleanup,
        )

    async def cancel_interrupted_run() -> None:
        await request_server_cancel()
        receipt = await _cleanup_process_group(process, process_start_time)
        if not process_cleanup_complete(receipt):
            raise RuntimeError("interrupted ctx process cleanup was not verified")

    collect_task = asyncio.create_task(collect())
    handled = False
    try:
        completed, _pending = await asyncio.wait({collect_task}, timeout=timeout)
        if not completed:
            timed_out = True
            error_code = "ETIMEDOUT"
            error_message = f"ctx agent timed out after {timeout:g}s"
            await request_server_cancel()
            collect_task.cancel()
            await asyncio.gather(collect_task, return_exceptions=True)
            process_cleanup = await _cleanup_process_group(process, process_start_time)
            local_signal = process_cleanup.signal
            exit_code = process.returncode if process.returncode is not None else -1
            handled = True
        elif collect_task.cancelled():
            raise asyncio.CancelledError
        else:
            task_error = collect_task.exception()
            if task_error is None:
                exit_code = collect_task.result()
                handled = True
            elif isinstance(task_error, ValueError):
                error_code = "protocol_error"
                error_message = str(sanitize(str(task_error)))
                process_cleanup = await _cleanup_process_group(
                    process, process_start_time
                )
                local_signal = process_cleanup.signal
                exit_code = process.returncode if process.returncode is not None else -1
                handled = True
            else:
                raise task_error
    finally:
        if not handled:
            collect_task.cancel()
            await asyncio.gather(collect_task, return_exceptions=True)
            await cancel_interrupted_run()

    if process_cleanup is None:
        process_cleanup = await _cleanup_process_group(process, process_start_time)
        local_signal = process_cleanup.signal
    stderr_bytes = await stderr_task
    if len(stderr_bytes) > MAX_STDERR_BYTES and error_code is None:
        error_code = "protocol_error"
        error_message = "stderr exceeds byte limit"
    stderr = stderr_bytes[:MAX_STDERR_BYTES].decode("utf-8", errors="replace").strip()
    stderr = str(sanitize(stderr, MAX_STDERR_BYTES))
    latency = time.perf_counter() - started
    completion = assistant_messages[-1] if assistant_messages else "".join(deltas)
    raw_frames_complete = bool(
        retain_frames and dropped_frame_count == 0 and len(frames) == frame_count
    )
    runtime_success = (
        not timed_out
        and exit_code == 0
        and error_code is None
        and done_status in {"ok", "success"}
        and process_cleanup_complete(process_cleanup)
    )
    if not runtime_success and error_code is None:
        if not process_cleanup_complete(process_cleanup):
            error_code = "cleanup_error"
            error_message = "ctx process group cleanup was not verified"
        elif stderr:
            error_code = f"exit_{exit_code}" if exit_code else "protocol_error"
            error_message = stderr
        elif last_recoverable_error is not None:
            error_code, error_message = last_recoverable_error
        else:
            error_code = f"exit_{exit_code}" if exit_code else "protocol_error"
            error_message = str(
                sanitize(f"ctx agent ended with status {done_status!r}")
            )
    identity = RunIdentity(client_id, canonical_run_id, session)
    if cancellation is not None and local_signal is not None:
        cancellation = CancellationReceipt(
            cancellation.requested_run,
            cancellation.session,
            cancellation.server_exit_code,
            cancellation.terminal_status,
            local_signal,
            cancellation.detail,
            cancellation.helper_process_cleanup,
        )
    return CtxRunResult(
        agent=agent_name,
        session=session,
        completion=completion,
        frames=tuple(frames),
        runtime_success=runtime_success,
        exit_code=exit_code,
        timed_out=timed_out,
        latency_seconds=latency,
        ttft_seconds=ttft,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        usage_available=usage_available,
        raw_frames_complete=raw_frames_complete,
        dropped_frame_count=dropped_frame_count,
        dropped_frame_bytes=dropped_frame_bytes,
        redacted_field_count=redacted_field_count,
        redacted_field_bytes=redacted_field_bytes,
        truncated_field_count=0,
        truncated_field_bytes=0,
        done_status=done_status,
        error_code=error_code,
        error_message=error_message,
        stderr=stderr,
        identity=identity,
        cancellation=cancellation,
        process_cleanup=process_cleanup,
        cleanup=None,
    )


@agent
def cli_bridge_agent(
    command: str = "agent_benchmark/examples/openai_cli_agent.py",
    extra_args: str = "",
    env_name: str = "OPENAI_BASE_URL",
) -> Agent:
    """Bridge a CLI agent that accepts --prompt into Inspect."""

    async def execute(state: AgentState) -> AgentState:
        async with sandbox_agent_bridge(state) as bridge:
            prompt = user_prompt(state.messages)
            cmd = [command]
            if extra_args:
                cmd += extra_args.split()
            cmd += ["--prompt", prompt.text]
            result = await sandbox().exec(
                cmd=cmd,
                env={env_name: f"http://localhost:{bridge.port}/v1"},
            )
            if not result.success:
                raise RuntimeError(f"CLI agent failed: {result.stderr}")
            return bridge.state

    return execute


@agent
def in_process_openai_agent() -> Agent:
    """Bridge an in-process OpenAI-compatible Python agent."""

    async def execute(state: AgentState) -> AgentState:
        try:
            from openai import AsyncOpenAI
        except ImportError as error:
            raise RuntimeError(
                "in-process OpenAI bridge requires the 'openai' benchmark extra"
            ) from error
        async with agent_bridge(state) as bridge:
            client = AsyncOpenAI()
            await client.chat.completions.create(
                model="inspect",
                messages=messages_to_openai(state.messages),
            )
            return bridge.state

    return execute


@agent
def ctx_cli_agent(
    ctx_path: str = "/usr/bin/ctx",
    agent_name: str = "coder",
    session_prefix: str = "inspect",
    timeout: float = 180.0,
    retain_frames: bool = False,
) -> Agent:
    """Evaluate an installed CortexFS agent through its real JSONL socket path."""

    async def execute(state: AgentState) -> AgentState:
        prompt = user_prompt(state.messages).text
        session = benchmark_session_name(session_prefix, agent_name)
        result = await run_ctx_agent(
            ctx_path=ctx_path,
            agent_name=agent_name,
            session=session,
            prompt=prompt,
            timeout=timeout,
            retain_frames=retain_frames,
        )
        model = f"ctx/{agent_name}"
        output = ModelOutput.from_content(
            model=model,
            content=result.completion,
            error=None if result.runtime_success else result.error_message,
        )
        output.time = result.latency_seconds
        output.usage = ModelUsage(
            input_tokens=result.input_tokens,
            output_tokens=result.output_tokens,
            total_tokens=result.input_tokens + result.output_tokens,
        )
        output.metadata = {"ctx": result.metadata()}
        state.output = output
        state.messages.append(output.message)
        transcript().info({"ctx_agent_benchmark": result.metadata(False)})
        return state

    return execute
