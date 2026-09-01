# pyright: reportMissingImports=false
from __future__ import annotations

import asyncio
import hashlib
import json
import os
import stat
import tempfile
import time
from contextlib import suppress
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, cast

from inspect_ai.agent import Agent, AgentState, agent
from inspect_ai.log import transcript
from inspect_ai.model import ModelOutput, ModelUsage, user_prompt

from .agents import (
    MAX_FRAMES,
    MAX_LINE_BYTES,
    MAX_RETAINED_FRAME_BYTES,
    MAX_STDERR_BYTES,
    MAX_STDOUT_BYTES,
    ProcessCleanupReceipt,
    _cleanup_process_group,
    _content_text,
    _empty_process_cleanup,
    _frame_evidence,
    _process_group_members,
    _usage,
    process_cleanup_complete,
    sanitize,
)
from .system import OwnedUserUnit

RSS_SAMPLE_SECONDS = 0.02
RSS_SAMPLE_MILLISECONDS = 20
PI_ENVIRONMENT_KEYS = (
    "HOME",
    "PATH",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "PI_CODING_AGENT_DIR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
)


@dataclass(frozen=True)
class PiUsage:
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    reasoning_tokens: int = 0
    total_tokens: int = 0
    provider_catalog_cost_usd: float | None = None


@dataclass(frozen=True)
class PiRunResult:
    completion: str
    runtime_success: bool
    exit_code: int
    timed_out: bool
    settled: bool
    settled_after_final: bool
    latency_seconds: float
    ttft_seconds: float | None
    usage: PiUsage
    usage_available: bool
    stop_reason: str | None
    error_code: str | None
    error_message: str | None
    stderr: str
    requested_provider: str
    requested_model: str
    response_provider: str | None
    response_model: str | None
    peak_rss_bytes: int | None
    peak_memory_bytes: int | None
    memory_complete: bool
    memory: dict[str, object]
    raw_frames_complete: bool
    dropped_frame_count: int
    dropped_frame_bytes: int
    redacted_field_count: int
    redacted_field_bytes: int
    truncated_field_count: int
    truncated_field_bytes: int
    process_cleanup: ProcessCleanupReceipt
    frames: tuple[dict[str, Any], ...]

    def metadata(self, include_frames: bool = True) -> dict[str, Any]:
        value: dict[str, Any] = {
            "runtime_success": self.runtime_success,
            "exit_code": self.exit_code,
            "timed_out": self.timed_out,
            "settled": self.settled,
            "settled_after_final": self.settled_after_final,
            "latency_seconds": self.latency_seconds,
            "ttft_seconds": self.ttft_seconds,
            "usage": asdict(self.usage),
            "usage_available": self.usage_available,
            "stop_reason": self.stop_reason,
            "error_code": self.error_code,
            "error_message": self.error_message,
            "stderr": self.stderr,
            "requested_provider": self.requested_provider,
            "requested_model": self.requested_model,
            "response_provider": self.response_provider,
            "response_model": self.response_model,
            "peak_rss_bytes": self.peak_rss_bytes,
            "peak_memory_bytes": self.peak_memory_bytes,
            "memory_scope": "owned-user-systemd-cgroup-v2",
            "memory_complete": self.memory_complete,
            "memory": self.memory,
            "raw_frames_complete": self.raw_frames_complete,
            "dropped_frame_count": self.dropped_frame_count,
            "dropped_frame_bytes": self.dropped_frame_bytes,
            "redacted_field_count": self.redacted_field_count,
            "redacted_field_bytes": self.redacted_field_bytes,
            "truncated_field_count": self.truncated_field_count,
            "truncated_field_bytes": self.truncated_field_bytes,
            "process_cleanup": asdict(self.process_cleanup),
            "rss_sample_interval_ms": RSS_SAMPLE_MILLISECONDS,
            "attributable_subscription_cost_usd": None,
        }
        sanitized = sanitize(value, MAX_STDERR_BYTES)
        if not isinstance(sanitized, dict):
            raise TypeError("sanitized Pi metadata must remain an object")
        if include_frames:
            sanitized["frames"] = list(self.frames)
        return sanitized


def _integer(value: object, field: str) -> int:
    if value is None:
        return 0
    return _usage(value, field)


def _required_integer(usage: dict[str, Any], field: str) -> int:
    if field not in usage:
        raise ValueError(f"missing Pi usage field {field}")
    return _usage(usage[field], field)


def _catalog_cost(usage: dict[str, Any]) -> float | None:
    cost = usage.get("cost")
    if not isinstance(cost, dict):
        return None
    total = cost.get("total")
    if isinstance(total, (int, float)) and not isinstance(total, bool) and total >= 0:
        return total * 1.0
    return None


def _usage_from_message(message: dict[str, Any]) -> PiUsage | None:
    usage = message.get("usage")
    if not isinstance(usage, dict):
        return None
    details = usage.get("outputDetails")
    reasoning = details.get("reasoning") if isinstance(details, dict) else None
    input_tokens = _required_integer(usage, "input")
    output_tokens = _required_integer(usage, "output")
    total_tokens = _required_integer(usage, "totalTokens")
    if total_tokens < input_tokens + output_tokens:
        raise ValueError("Pi totalTokens is smaller than input plus output")
    return PiUsage(
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        cache_read_tokens=_integer(usage.get("cacheRead"), "cacheRead"),
        cache_write_tokens=_integer(usage.get("cacheWrite"), "cacheWrite"),
        reasoning_tokens=_integer(usage.get("reasoning", reasoning), "reasoning"),
        total_tokens=total_tokens,
        provider_catalog_cost_usd=_catalog_cost(usage),
    )


def _sum_usage(left: PiUsage, right: PiUsage) -> PiUsage:
    costs = [
        value
        for value in (
            left.provider_catalog_cost_usd,
            right.provider_catalog_cost_usd,
        )
        if value is not None
    ]
    return PiUsage(
        input_tokens=left.input_tokens + right.input_tokens,
        output_tokens=left.output_tokens + right.output_tokens,
        cache_read_tokens=left.cache_read_tokens + right.cache_read_tokens,
        cache_write_tokens=left.cache_write_tokens + right.cache_write_tokens,
        reasoning_tokens=left.reasoning_tokens + right.reasoning_tokens,
        total_tokens=left.total_tokens + right.total_tokens,
        provider_catalog_cost_usd=sum(costs) if costs else None,
    )


def _proc_start_time(pid: int) -> str | None:
    try:
        value = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    except (OSError, UnicodeError):
        return None
    end = value.rfind(")")
    fields = value[end + 2 :].split() if end >= 0 else []
    return fields[19] if len(fields) > 19 else None


def _proc_rss(pid: int) -> int:
    try:
        lines = Path(f"/proc/{pid}/status").read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError):
        return 0
    for line in lines:
        if line.startswith("VmRSS:"):
            fields = line.split()
            if len(fields) < 2:
                return 0
            try:
                return int(fields[1]) * 1024
            except ValueError:
                return 0
    return 0


def _children(pid: int) -> tuple[int, ...]:
    try:
        value = Path(f"/proc/{pid}/task/{pid}/children").read_text(encoding="ascii")
        return tuple(int(item) for item in value.split())
    except (OSError, UnicodeError, ValueError):
        return ()


def _process_tree_rss(pid: int, start_time: str) -> int | None:
    if _proc_start_time(pid) != start_time:
        return None
    total = 0
    pending = [pid]
    visited: set[int] = set()
    while pending:
        current = pending.pop()
        if current in visited:
            continue
        visited.add(current)
        total += _proc_rss(current)
        pending.extend(_children(current))
    return total


async def _sample_peak_rss(pid: int, done: asyncio.Event) -> int | None:
    start_time = _proc_start_time(pid)
    if start_time is None:
        return None
    peak = 0
    while not done.is_set():
        sample = _process_tree_rss(pid, start_time)
        if sample is not None:
            peak = max(peak, sample)
        with suppress(asyncio.TimeoutError):
            await asyncio.wait_for(done.wait(), RSS_SAMPLE_SECONDS)
    sample = _process_tree_rss(pid, start_time)
    if sample is not None:
        peak = max(peak, sample)
    return peak or None


def _pi_settings_identity() -> dict[str, object]:
    agent_dir = os.environ.get("PI_CODING_AGENT_DIR")
    if not agent_dir:
        home = os.environ.get("HOME")
        agent_dir = str(Path(home) / ".pi" / "agent") if home else None
    if agent_dir is None:
        return {
            "available": False,
            "complete": False,
            "state": "unresolved",
            "sha256": None,
            "bytes": None,
        }
    path = Path(agent_dir) / "settings.json"
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except FileNotFoundError:
        return {
            "available": False,
            "complete": True,
            "state": "controlled-absence",
            "sha256": None,
            "bytes": 0,
        }
    except OSError as error:
        return {
            "available": False,
            "complete": False,
            "state": f"error-{error.errno}",
            "sha256": None,
            "bytes": None,
        }
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 1024 * 1024:
            return {
                "available": False,
                "complete": False,
                "state": "invalid-file",
                "sha256": None,
                "bytes": metadata.st_size,
            }
        content = os.read(descriptor, metadata.st_size + 1)
    finally:
        os.close(descriptor)
    complete = len(content) == metadata.st_size
    return {
        "available": complete,
        "complete": complete,
        "state": "present" if complete else "short-read",
        "sha256": hashlib.sha256(content).hexdigest() if complete else None,
        "bytes": len(content),
    }


def _pi_environment() -> dict[str, str]:
    environment = {
        key: value for key in PI_ENVIRONMENT_KEYS if (value := os.environ.get(key))
    }
    environment.update(
        {"PI_OFFLINE": "1", "TZ": "UTC", "LANG": "C.UTF-8", "NO_COLOR": "1"}
    )
    return environment


class PiJsonlRunner:
    """Run one isolated Pi JSONL request and retain bounded benchmark evidence."""

    def __init__(
        self,
        *,
        pi_path: str,
        provider: str,
        model: str,
        thinking: str,
        timeout: float,
        system_prompt: str | None = None,
        workspace: str | None = None,
        retain_frames: bool = False,
        owned_memory: bool = True,
    ) -> None:
        if not provider or not model or not thinking or timeout <= 0:
            raise ValueError(
                "provider, model, thinking, and positive timeout are required"
            )
        self.pi_path = pi_path
        self.provider = provider
        self.model = model
        self.thinking = thinking
        self.timeout = timeout
        self.system_prompt = system_prompt
        self.workspace = workspace
        self.retain_frames = retain_frames
        self.owned_memory = owned_memory

    def command(self, prompt: str) -> list[str]:
        command = [
            self.pi_path,
            "--mode",
            "json",
            "--provider",
            self.provider,
            "--model",
            self.model,
            "--thinking",
            self.thinking,
        ]
        if self.system_prompt is not None:
            command.extend(("--system-prompt", self.system_prompt))
        command.extend(
            (
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--no-approve",
                "--no-session",
                "--",
                prompt,
            )
        )
        return command

    def identity(self) -> dict[str, object]:
        prompt = self.system_prompt
        environment = _pi_environment()
        environment_hash = hashlib.sha256(
            json.dumps(environment, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        return {
            "pi_path": str(Path(self.pi_path).expanduser().absolute()),
            "provider": self.provider,
            "model": self.model,
            "thinking": self.thinking,
            "system_prompt": "pi-default" if prompt is None else "explicit",
            "system_prompt_sha256": None
            if prompt is None
            else hashlib.sha256(prompt.encode()).hexdigest(),
            "tools": "disabled",
            "resources": "disabled",
            "session": "disabled",
            "workspace": "per-request-empty-temporary"
            if self.workspace is None
            else "caller-supplied",
            "process_boundary": "cold-cli-per-request",
            "memory_method": "owned-user-systemd-cgroup-v2"
            if self.owned_memory
            else "disabled-incomplete",
            "environment_keys": sorted(environment),
            "environment_sha256": environment_hash,
            "settings": _pi_settings_identity(),
        }

    async def run(self, prompt: str) -> PiRunResult:
        temporary = None
        cwd = self.workspace
        if cwd is None:
            temporary = tempfile.TemporaryDirectory(prefix="pi-benchmark-")
            cwd = temporary.name
        try:
            return await self._run(prompt, cwd)
        finally:
            if temporary is not None:
                temporary.cleanup()

    async def _run(self, prompt: str, cwd: str) -> PiRunResult:
        started = time.perf_counter()
        environment = _pi_environment()
        owned = (
            OwnedUserUnit(
                cwd=cwd,
                environment=environment,
                command=self.command(prompt),
                timeout=self.timeout,
            )
            if self.owned_memory
            else None
        )
        if owned is not None and not await owned.preflight():
            return self._failure(
                started,
                "memory_unavailable",
                "owned user unit is unavailable or already exists",
                {"complete": False, "issue": "owned unit preflight failed"},
            )
        launch_command = owned.command if owned is not None else self.command(prompt)
        try:
            process = await asyncio.create_subprocess_exec(
                *launch_command,
                cwd=cwd,
                env=os.environ.copy() if owned is not None else environment,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                start_new_session=True,
            )
        except OSError as error:
            return self._failure(started, "spawn_error", str(error))

        unit_stop = asyncio.Event()
        memory_task = (
            asyncio.create_task(owned.observe(process, unit_stop))
            if owned is not None
            else None
        )
        process_start_time = _process_group_members(process.pid).get(process.pid)
        stdout = cast(asyncio.StreamReader, process.stdout)
        stderr_stream = cast(asyncio.StreamReader, process.stderr)
        done = asyncio.Event()
        rss_task = asyncio.create_task(_sample_peak_rss(process.pid, done))

        async def read_stderr() -> tuple[bytes, bool]:
            retained = bytearray()
            exceeded = False
            while chunk := await stderr_stream.read(8192):
                room = MAX_STDERR_BYTES - len(retained)
                if room > 0:
                    retained.extend(chunk[:room])
                exceeded = exceeded or len(chunk) > room
            return bytes(retained), exceeded

        stderr_task = asyncio.create_task(read_stderr())
        frames: list[dict[str, Any]] = []
        completion = ""
        final_message_seen = False
        final_message_index: int | None = None
        settled_index: int | None = None
        assistant_message_count = 0
        usage_message_count = 0
        usage = PiUsage()
        usage_available = False
        stop_reason: str | None = None
        response_provider: str | None = None
        response_model: str | None = None
        ttft: float | None = None
        settled = False
        stdout_bytes = 0
        retained_bytes = 0
        frame_count = 0
        dropped_frame_count = 0
        dropped_frame_bytes = 0
        redacted_field_count = 0
        redacted_field_bytes = 0
        error_code: str | None = None
        error_message: str | None = None

        async def collect() -> int:
            nonlocal completion, final_message_seen, final_message_index, settled_index
            nonlocal \
                assistant_message_count, \
                usage_message_count, \
                usage, \
                usage_available
            nonlocal stop_reason, response_provider, response_model, ttft, settled
            nonlocal stdout_bytes, retained_bytes, frame_count
            nonlocal dropped_frame_count, dropped_frame_bytes
            nonlocal redacted_field_count, redacted_field_bytes
            while True:
                raw_line = await stdout.readline()
                if not raw_line:
                    break
                stdout_bytes += len(raw_line)
                frame_count += 1
                if len(raw_line) > MAX_LINE_BYTES or stdout_bytes > MAX_STDOUT_BYTES:
                    raise ValueError("Pi JSONL output exceeds byte limit")
                if frame_count > MAX_FRAMES:
                    raise ValueError("Pi JSONL frame count exceeds limit")
                observed_seconds = time.perf_counter() - started
                try:
                    frame = json.loads(raw_line.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise ValueError("invalid Pi JSONL frame") from error
                if not isinstance(frame, dict) or not isinstance(
                    frame.get("type"), str
                ):
                    raise ValueError("Pi JSONL frame must be a typed object")
                evidence, evidence_bytes, fields, field_bytes = _frame_evidence(
                    frame, observed_seconds
                )
                redacted_field_count += fields
                redacted_field_bytes += field_bytes
                kind = frame["type"]
                if (
                    self.retain_frames
                    and retained_bytes + evidence_bytes <= MAX_RETAINED_FRAME_BYTES
                ):
                    frames.append(evidence)
                    retained_bytes += evidence_bytes
                elif self.retain_frames:
                    dropped_frame_count += 1
                    dropped_frame_bytes += evidence_bytes
                elif kind in {
                    "session",
                    "message_end",
                    "agent_settled",
                    "auto_retry_start",
                    "auto_retry_end",
                }:
                    frames.append(evidence)
                if kind == "message_update":
                    event = frame.get("assistantMessageEvent")
                    if isinstance(event, dict) and event.get("type") == "text_delta":
                        delta = event.get("delta")
                        if isinstance(delta, str) and delta.strip() and ttft is None:
                            ttft = time.perf_counter() - started
                elif kind == "message_end":
                    message = frame.get("message")
                    if (
                        not isinstance(message, dict)
                        or message.get("role") != "assistant"
                    ):
                        continue
                    assistant_message_count += 1
                    final_message_seen = True
                    final_message_index = frame_count
                    completion = _content_text(message.get("content"))
                    current = _usage_from_message(message)
                    if current is not None:
                        usage = _sum_usage(usage, current)
                        usage_message_count += 1
                    usage_available = usage_message_count == assistant_message_count
                    reason = message.get("stopReason")
                    stop_reason = reason if isinstance(reason, str) else None
                    provider = message.get("provider")
                    model = message.get("model")
                    response_provider = provider if isinstance(provider, str) else None
                    response_model = model if isinstance(model, str) else None
                elif kind == "agent_settled":
                    settled = True
                    settled_index = frame_count
            return await process.wait()

        timed_out = False
        process_cleanup: ProcessCleanupReceipt | None = None
        try:
            exit_code = await asyncio.wait_for(collect(), self.timeout)
        except asyncio.TimeoutError:
            timed_out = True
            error_code = "ETIMEDOUT"
            error_message = f"Pi timed out after {self.timeout:g}s"
            unit_stop.set()
            exit_code = process.returncode if process.returncode is not None else -1
        except ValueError as error:
            error_code = "protocol_error"
            error_message = str(error)
            unit_stop.set()
            exit_code = process.returncode if process.returncode is not None else -1
        except (asyncio.CancelledError, KeyboardInterrupt) as error:
            unit_stop.set()
            memory = (
                await memory_task
                if memory_task is not None
                else {"complete": False, "issue": "owned memory disabled"}
            )
            interrupted_cleanup = await _cleanup_process_group(
                process, process_start_time
            )
            memory_cleanup = memory.get("cleanup")
            if not process_cleanup_complete(interrupted_cleanup) or not (
                isinstance(memory_cleanup, dict) and memory_cleanup.get("complete")
            ):
                raise RuntimeError(
                    "interrupted Pi process cleanup was not verified"
                ) from error
            raise
        finally:
            done.set()

        memory = (
            await memory_task
            if memory_task is not None
            else {"complete": False, "issue": "owned memory disabled"}
        )
        if process_cleanup is None:
            process_cleanup = await _cleanup_process_group(process, process_start_time)
        peak_rss = await rss_task
        memory_peak = memory.get("memory_peak_bytes")
        if not isinstance(memory_peak, int) or isinstance(memory_peak, bool):
            memory_peak = None
        stderr_bytes, stderr_exceeded = await stderr_task
        if stderr_exceeded and error_code is None:
            error_code = "protocol_error"
            error_message = "Pi stderr exceeds byte limit"
        stderr = (
            stderr_bytes[:MAX_STDERR_BYTES].decode("utf-8", errors="replace").strip()
        )
        stderr = str(sanitize(stderr, MAX_STDERR_BYTES))
        settled_after_final = bool(
            settled_index is not None
            and final_message_index is not None
            and settled_index > final_message_index
        )
        raw_frames_complete = bool(
            self.retain_frames
            and dropped_frame_count == 0
            and len(frames) == frame_count
        )
        if error_code is None and not process_cleanup_complete(process_cleanup):
            error_code = "cleanup_error"
            error_message = "Pi process group cleanup was not verified"
        elif error_code is None and exit_code != 0:
            error_code = f"exit_{exit_code}"
            error_message = stderr or f"Pi exited with {exit_code}"
        elif error_code is None and not settled_after_final:
            error_code = "missing_settled"
            error_message = "Pi did not settle after the final assistant message"
        elif error_code is None and (not final_message_seen or not completion.strip()):
            error_code = "missing_final_message"
            error_message = "Pi produced no non-empty final assistant message"
        elif error_code is None and stop_reason != "stop":
            error_code = "model_error" if stop_reason == "error" else "stop_reason"
            error_message = f"Pi assistant stopReason={stop_reason!r}"
        elif error_code is None and response_provider != self.provider:
            error_code = "provider_mismatch"
            error_message = f"Pi response provider {response_provider!r} does not match {self.provider!r}"
        elif error_code is None and response_model != self.model:
            error_code = "model_mismatch"
            error_message = (
                f"Pi response model {response_model!r} does not match {self.model!r}"
            )
        runtime_success = not timed_out and error_code is None
        return PiRunResult(
            completion=completion,
            runtime_success=runtime_success,
            exit_code=exit_code,
            timed_out=timed_out,
            settled=settled,
            settled_after_final=settled_after_final,
            latency_seconds=time.perf_counter() - started,
            ttft_seconds=ttft,
            usage=usage,
            usage_available=usage_available,
            stop_reason=stop_reason,
            error_code=error_code,
            error_message=str(sanitize(error_message)) if error_message else None,
            stderr=stderr,
            requested_provider=self.provider,
            requested_model=self.model,
            response_provider=response_provider,
            response_model=response_model,
            peak_rss_bytes=peak_rss,
            peak_memory_bytes=memory_peak,
            memory_complete=bool(
                memory.get("complete") and process_cleanup_complete(process_cleanup)
            ),
            memory=memory,
            raw_frames_complete=raw_frames_complete,
            dropped_frame_count=dropped_frame_count,
            dropped_frame_bytes=dropped_frame_bytes,
            redacted_field_count=redacted_field_count,
            redacted_field_bytes=redacted_field_bytes,
            truncated_field_count=0,
            truncated_field_bytes=0,
            process_cleanup=process_cleanup,
            frames=tuple(frames),
        )

    def _failure(
        self,
        started: float,
        code: str,
        message: str,
        memory: dict[str, object] | None = None,
    ) -> PiRunResult:
        return PiRunResult(
            completion="",
            runtime_success=False,
            exit_code=-1,
            timed_out=False,
            settled=False,
            settled_after_final=False,
            latency_seconds=time.perf_counter() - started,
            ttft_seconds=None,
            usage=PiUsage(),
            usage_available=False,
            stop_reason=None,
            error_code=code,
            error_message=str(sanitize(message)),
            stderr="",
            requested_provider=self.provider,
            requested_model=self.model,
            response_provider=None,
            response_model=None,
            peak_rss_bytes=None,
            peak_memory_bytes=None,
            memory_complete=False,
            memory=memory or {"complete": False, "issue": "process did not start"},
            raw_frames_complete=False,
            dropped_frame_count=0,
            dropped_frame_bytes=0,
            redacted_field_count=0,
            redacted_field_bytes=0,
            truncated_field_count=0,
            truncated_field_bytes=0,
            process_cleanup=_empty_process_cleanup(),
            frames=(),
        )


@agent
def pi_cli_agent(
    pi_path: str = "pi",
    provider: str = "openai-codex",
    model_id: str = "",
    thinking: str = "medium",
    timeout: float = 180.0,
    system_prompt: str | None = None,
    retain_frames: bool = False,
) -> Agent:
    """Evaluate Pi through its non-interactive JSONL protocol."""

    if not model_id:
        raise ValueError("pi_cli_agent requires an explicit model_id")
    runner = PiJsonlRunner(
        pi_path=pi_path,
        provider=provider,
        model=model_id,
        thinking=thinking,
        timeout=timeout,
        system_prompt=system_prompt,
        retain_frames=retain_frames,
    )

    async def execute(state: AgentState) -> AgentState:
        result = await runner.run(user_prompt(state.messages).text)
        output = ModelOutput.from_content(
            model=f"pi/{provider}/{model_id}",
            content=result.completion,
            error=None if result.runtime_success else result.error_message,
        )
        output.time = result.latency_seconds
        output.usage = ModelUsage(
            input_tokens=result.usage.input_tokens,
            output_tokens=result.usage.output_tokens,
            total_tokens=result.usage.total_tokens,
        )
        output.metadata = {"pi": result.metadata()}
        state.output = output
        state.messages.append(output.message)
        transcript().info({"pi_agent_benchmark": result.metadata(False)})
        return state

    return execute
