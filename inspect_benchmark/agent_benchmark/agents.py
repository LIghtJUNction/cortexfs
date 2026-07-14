import asyncio
import json
import os
import re
import signal
import time
import uuid
from dataclasses import asdict, dataclass
from typing import Any, Protocol

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
    async def run(self, *command: str, timeout: float) -> tuple[int, str, str]: ...


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
    done_status: str | None
    error_code: str | None
    error_message: str | None
    stderr: str
    identity: RunIdentity
    cancellation: CancellationReceipt | None
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
            "done_status": self.done_status,
            "error_code": self.error_code,
            "error_message": self.error_message,
            "stderr": self.stderr,
            "identity": asdict(self.identity),
            "cancellation": asdict(self.cancellation) if self.cancellation else None,
            "cleanup": asdict(self.cleanup) if self.cleanup else None,
        }
        if include_frames:
            value["frames"] = list(self.frames)
        sanitized = sanitize(value, MAX_STDERR_BYTES)
        assert isinstance(sanitized, dict)
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
        redacted = SECRET_VALUE_PATTERN.sub(r"\1\2[REDACTED]", redacted)
        return _bounded_text(redacted, limit)
    return value


def _redact(value: object) -> object:
    return sanitize(value)


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
    async def run(self, *command: str, timeout: float) -> tuple[int, str, str]:
        process = await asyncio.create_subprocess_exec(
            *command,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            start_new_session=True,
        )
        try:
            stdout, stderr = await asyncio.wait_for(process.communicate(), timeout)
        except asyncio.TimeoutError:
            await _terminate_process(process)
            return -1, "", "command timed out"
        return (
            process.returncode or 0,
            str(sanitize(stdout.decode("utf-8", errors="replace"), MAX_STDERR_BYTES)),
            str(sanitize(stderr.decode("utf-8", errors="replace"), MAX_STDERR_BYTES)),
        )


async def _terminate_process(process: asyncio.subprocess.Process) -> str | None:
    if process.returncode is not None:
        return None
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return None
    try:
        await asyncio.wait_for(process.wait(), timeout=2.0)
    except asyncio.TimeoutError:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            return "TERM"
        await process.wait()
        return "KILL"
    return "TERM"


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
            done_status=None,
            error_code="spawn_error",
            error_message=str(sanitize(str(error))),
            stderr="",
            identity=identity,
            cancellation=None,
            cleanup=None,
        )

    assert process.stdout is not None
    assert process.stderr is not None
    stderr_task = asyncio.create_task(process.stderr.read(MAX_STDERR_BYTES + 1))
    frames: list[dict[str, Any]] = []
    deltas: list[str] = []
    assistant_messages: list[str] = []
    ttft: float | None = None
    input_tokens = 0
    output_tokens = 0
    done_status: str | None = None
    error_code: str | None = None
    error_message: str | None = None
    canonical_run_id: str | None = None
    stdout_bytes = 0
    retained_bytes = 0
    terminal_seen = False
    frame_count = 0

    async def collect() -> int:
        nonlocal ttft, input_tokens, output_tokens, done_status
        nonlocal error_code, error_message, canonical_run_id, stdout_bytes
        nonlocal retained_bytes, terminal_seen, frame_count
        while True:
            raw_line = await process.stdout.readline()
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
            try:
                frame = json.loads(raw_line.decode("utf-8"))
            except UnicodeDecodeError as error:
                raise ValueError("invalid UTF-8 frame") from error
            except json.JSONDecodeError as error:
                raise ValueError("invalid JSON frame") from error
            if not isinstance(frame, dict):
                raise ValueError("JSON frame must be an object")
            if terminal_seen:
                continue
            run = frame.get("run")
            if isinstance(run, str) and run:
                if canonical_run_id is not None and canonical_run_id != run:
                    raise ValueError("conflicting canonical run ids")
                canonical_run_id = run
            evidence = _redact(frame)
            evidence_bytes = len(json.dumps(evidence).encode("utf-8"))
            if (
                retain_frames
                and retained_bytes + evidence_bytes <= MAX_RETAINED_FRAME_BYTES
            ):
                frames.append(evidence)  # type: ignore[arg-type]
                retained_bytes += evidence_bytes
            elif not retain_frames and frame.get("type") in {
                "start",
                "done",
                "error",
                "usage",
            }:
                frames.append(evidence)  # type: ignore[arg-type]
            kind = frame.get("type")
            if kind == "delta" and isinstance(frame.get("text"), str):
                text = frame["text"]
                if text and ttft is None:
                    ttft = time.perf_counter() - started
                deltas.append(text)
            elif kind == "message" and frame.get("role") == "assistant":
                text = _content_text(frame.get("content"))
                if text and ttft is None:
                    ttft = time.perf_counter() - started
                if text:
                    assistant_messages.append(text)
            elif kind == "usage":
                input_tokens += _usage(frame.get("input_tokens"), "input_tokens")
                output_tokens += _usage(frame.get("output_tokens"), "output_tokens")
            elif kind == "done" and isinstance(frame.get("status"), str):
                done_status = frame["status"]
                terminal_seen = True
            elif kind == "error":
                error_code = str(sanitize(str(frame.get("code") or "agent_error")))
                error_message = str(
                    sanitize(str(frame.get("message") or "ctx agent failed"))
                )
        return await process.wait()

    timed_out = False
    local_signal: str | None = None
    runner = command_runner or SubprocessCommandRunner()
    try:
        exit_code = await asyncio.wait_for(collect(), timeout=timeout)
    except asyncio.TimeoutError:
        timed_out = True
        error_code = "ETIMEDOUT"
        error_message = f"ctx agent timed out after {timeout:g}s"
        if canonical_run_id is not None:
            cancel_code, _stdout, cancel_stderr = await runner.run(
                ctx_path,
                "agent",
                "cancel",
                agent_name,
                "--session",
                session,
                "--raw",
                canonical_run_id,
                timeout=min(timeout, 5.0),
            )
            cancellation = CancellationReceipt(
                requested_run=canonical_run_id,
                session=session,
                server_exit_code=cancel_code,
                terminal_status=done_status,
                local_signal=None,
                detail=str(sanitize(cancel_stderr)) or None,
            )
        local_signal = await _terminate_process(process)
        exit_code = process.returncode if process.returncode is not None else -1
    except ValueError as error:
        error_code = "protocol_error"
        error_message = str(sanitize(str(error)))
        local_signal = await _terminate_process(process)
        exit_code = process.returncode if process.returncode is not None else -1
    except (asyncio.CancelledError, KeyboardInterrupt):
        if canonical_run_id is not None:
            cancel_code, _stdout, cancel_stderr = await runner.run(
                ctx_path,
                "agent",
                "cancel",
                agent_name,
                "--session",
                session,
                "--raw",
                canonical_run_id,
                timeout=min(timeout, 5.0),
            )
            cancellation = CancellationReceipt(
                canonical_run_id,
                session,
                cancel_code,
                done_status,
                None,
                str(sanitize(cancel_stderr)) or None,
            )
        await _terminate_process(process)
        raise

    stderr_bytes = await stderr_task
    if len(stderr_bytes) > MAX_STDERR_BYTES and error_code is None:
        error_code = "protocol_error"
        error_message = "stderr exceeds byte limit"
    stderr = stderr_bytes[:MAX_STDERR_BYTES].decode("utf-8", errors="replace").strip()
    stderr = str(sanitize(stderr, MAX_STDERR_BYTES))
    latency = time.perf_counter() - started
    completion = assistant_messages[-1] if assistant_messages else "".join(deltas)
    runtime_success = (
        not timed_out
        and exit_code == 0
        and error_code is None
        and done_status in {"ok", "success"}
    )
    if not runtime_success and error_code is None:
        error_code = f"exit_{exit_code}" if exit_code else "protocol_error"
        error_message = str(
            sanitize(stderr or f"ctx agent ended with status {done_status!r}")
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
        done_status=done_status,
        error_code=error_code,
        error_message=error_message,
        stderr=stderr,
        identity=identity,
        cancellation=cancellation,
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
