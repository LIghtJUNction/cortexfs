# pyright: reportArgumentType=false, reportAttributeAccessIssue=false
from __future__ import annotations

import asyncio
import importlib
import json
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, patch


def _stub_inspect() -> None:
    inspect = types.ModuleType("inspect_ai")
    agent = types.ModuleType("inspect_ai.agent")
    log = types.ModuleType("inspect_ai.log")
    model = types.ModuleType("inspect_ai.model")
    util = types.ModuleType("inspect_ai.util")

    def identity(function=None, **_kw):
        return function if function else identity

    agent.Agent = object
    agent.AgentState = object
    agent.agent = identity
    agent.agent_bridge = identity
    agent.sandbox_agent_bridge = identity
    log.transcript = lambda: types.SimpleNamespace(info=lambda _value: None)
    model.ModelOutput = object
    model.ModelUsage = object
    model.messages_to_openai = lambda value: value
    model.user_prompt = lambda value: value
    util.sandbox = lambda: None
    sys.modules.update(
        {
            "inspect_ai": inspect,
            "inspect_ai.agent": agent,
            "inspect_ai.log": log,
            "inspect_ai.model": model,
            "inspect_ai.util": util,
        }
    )


_stub_inspect()
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
agents = importlib.import_module("agent_benchmark.agents")
system = importlib.import_module("agent_benchmark.system")


class FakeStream:
    def __init__(self, lines: list[bytes], hang: bool = False) -> None:
        self.lines = lines
        self.hang = hang

    async def readline(self) -> bytes:
        if self.lines:
            return self.lines.pop(0)
        if self.hang:
            await asyncio.Future()
        return b""

    async def read(self, limit: int = -1) -> bytes:
        return b"".join(self.lines)[:limit]


class FakeProcess:
    def __init__(
        self, lines: list[bytes], hang: bool = False, stderr: bytes = b""
    ) -> None:
        self.stdout = FakeStream(lines, hang)
        self.stderr = FakeStream([stderr])
        self.pid = 4242
        self.returncode: int | None = None

    async def wait(self) -> int:
        if self.returncode is None:
            self.returncode = 0
        return self.returncode

    async def communicate(self) -> tuple[bytes, bytes]:
        return b"", b""


class FakeRunner:
    def __init__(self, code: int = 0) -> None:
        self.code = code
        self.commands: list[tuple[str, ...]] = []

    async def run(self, *command: str, timeout: float) -> tuple[int, str, str]:
        self.commands.append(command)
        return self.code, "", "cancel failed" if self.code else ""


def frame(value: object) -> bytes:
    return json.dumps(value).encode() + b"\n"


class LifecycleTests(unittest.IsolatedAsyncioTestCase):
    async def run_real_agent(
        self,
        frames: list[dict[str, object]],
        *,
        exit_code: int = 0,
        stderr: str = "",
        delay: float = 0,
        timeout: float = 1.0,
    ):
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "ctx-fixture"
            executable.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys, time\n"
                f"frames = {frames!r}\n"
                "for value in frames:\n"
                "    print(json.dumps(value), flush=True)\n"
                f"sys.stderr.write({stderr!r})\n"
                f"time.sleep({delay!r})\n"
                f"raise SystemExit({exit_code!r})\n",
                encoding="utf-8",
            )
            executable.chmod(0o755)
            return await agents.run_ctx_agent(
                ctx_path=os.fspath(executable),
                agent_name="coder",
                session="benchmark-owned",
                prompt="x",
                timeout=timeout,
            )

    async def run_agent(
        self,
        lines: list[bytes],
        *,
        hang: bool = False,
        runner: FakeRunner | None = None,
        timeout: float = 1.0,
        stderr: bytes = b"",
        retain: bool = False,
    ):
        process = FakeProcess(lines, hang, stderr)
        with (
            patch.object(
                agents.asyncio,
                "create_subprocess_exec",
                AsyncMock(return_value=process),
            ),
            patch.object(
                agents.os,
                "killpg",
                lambda _pid, _signal: setattr(process, "returncode", -15),
            ),
            patch.object(
                agents,
                "_process_group_members",
                side_effect=lambda _pgid: (
                    {process.pid: 1} if process.returncode is None else {}
                ),
            ),
        ):
            return await agents.run_ctx_agent(
                ctx_path="ctx",
                agent_name="coder",
                session="benchmark-owned",
                prompt="x",
                timeout=timeout,
                retain_frames=retain,
                command_runner=runner or FakeRunner(),
            )

    async def test_canonical_id_is_separate_and_receipts_serialize(self) -> None:
        result = await self.run_agent(
            [
                frame({"type": "start", "run": "run-1"}),
                frame({"type": "done", "status": "ok", "run": "run-1"}),
            ]
        )
        self.assertNotEqual(result.identity.client_id, "run-1")
        self.assertEqual(result.identity.canonical_run_id, "run-1")
        json.dumps(result.metadata())

    async def test_conflicting_run_and_invalid_protocol(self) -> None:
        result = await self.run_agent(
            [
                frame({"type": "start", "run": "one"}),
                frame({"type": "usage", "run": "two"}),
            ]
        )
        self.assertEqual(result.error_code, "protocol_error")
        for bad in (
            b"\xff\n",
            b"not-json\n",
            frame([]),
            frame({"type": "usage", "input_tokens": "1"}),
            frame({"type": "usage", "input_tokens": 1}),
            frame({"type": "usage", "output_tokens": 1}),
        ):
            self.assertEqual((await self.run_agent([bad])).error_code, "protocol_error")

        partial = await self.run_agent(
            [
                frame({"type": "start", "run": "r"}),
                frame({"type": "usage", "input_tokens": 2, "output_tokens": 3}),
                frame({"type": "usage", "input_tokens": 1}),
            ]
        )
        self.assertEqual(partial.error_code, "protocol_error")
        self.assertFalse(partial.usage_available)

    async def test_bounds_and_redaction(self) -> None:
        oversized = b"{" + b"x" * agents.MAX_LINE_BYTES + b"}\n"
        self.assertEqual(
            (await self.run_agent([oversized])).error_code, "protocol_error"
        )
        secret = {
            "type": "start",
            "run": "r",
            "authorization": "Bearer abc",
            "nested": {"token": "x"},
        }
        result = await self.run_agent(
            [frame(secret), frame({"type": "done", "status": "ok", "run": "r"})],
            retain=True,
        )
        rendered = json.dumps(result.frames)
        self.assertNotIn("abc", rendered)
        self.assertNotIn('"x"', rendered)

    async def test_timeout_after_run_cancels_exact_id_first(self) -> None:
        runner = FakeRunner()
        result = await self.run_agent(
            [frame({"type": "start", "run": "run-7"})],
            hang=True,
            runner=runner,
            timeout=0.01,
        )
        self.assertTrue(result.timed_out)
        self.assertEqual(runner.commands[0][-1], "run-7")
        self.assertEqual(result.cancellation.local_signal, "TERM")
        self.assertFalse(agents.record_process_cleanup_complete(result.metadata()))

    async def test_command_runner_retains_verified_helper_cleanup_receipt(self) -> None:
        runner = agents.SubprocessCommandRunner()
        code, stdout, stderr = await runner.run(
            sys.executable,
            "-c",
            "print('ok')",
            timeout=1,
        )
        self.assertEqual((code, stdout.strip(), stderr), (0, "ok", ""))
        self.assertTrue(agents.process_cleanup_complete(runner.last_cleanup))

    async def test_process_group_cleanup_rejects_reused_leader_identity(self) -> None:
        process = FakeProcess([], hang=True)
        kill = patch.object(agents.os, "killpg")
        with (
            patch.object(
                agents,
                "_process_group_members",
                return_value={process.pid: 2},
            ),
            kill as kill_mock,
        ):
            receipt = await agents._cleanup_process_group(process, 1)
        self.assertFalse(receipt.verified_empty)
        self.assertIn("ownership", receipt.detail)
        kill_mock.assert_not_called()

    async def test_process_group_cleanup_kills_a_surviving_grandchild(self) -> None:
        code = """
import signal, subprocess, sys, time
child = subprocess.Popen([
    sys.executable,
    "-c",
    "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); print('ready', flush=True); time.sleep(30)",
], stdout=subprocess.PIPE, text=True)
child.stdout.readline()
print(child.pid, flush=True)
signal.signal(signal.SIGTERM, lambda *_args: sys.exit(0))
time.sleep(30)
"""
        process = await asyncio.create_subprocess_exec(
            sys.executable,
            "-c",
            code,
            stdout=asyncio.subprocess.PIPE,
            start_new_session=True,
        )
        assert process.stdout is not None
        child_pid = int((await process.stdout.readline()).decode().strip())
        receipt = await agents._cleanup_process_group(process)
        self.assertTrue(receipt.term_sent)
        self.assertTrue(receipt.kill_sent)
        self.assertTrue(receipt.verified_empty)
        self.assertNotIn(child_pid, receipt.remaining_pids)

    async def test_cancel_runner_exception_cannot_skip_local_termination(self) -> None:
        runner = types.SimpleNamespace(
            run=AsyncMock(side_effect=OSError("cancel transport failed"))
        )
        result = await self.run_agent(
            [frame({"type": "start", "run": "run-8"})],
            hang=True,
            runner=runner,
            timeout=0.01,
        )
        self.assertTrue(result.timed_out)
        self.assertEqual(result.cancellation.server_exit_code, -1)
        self.assertEqual(result.cancellation.local_signal, "TERM")
        self.assertIn("cancel transport failed", result.cancellation.detail)

    async def test_timeout_before_run_never_guesses_and_cancel_failure_is_recorded(
        self,
    ) -> None:
        runner = FakeRunner(1)
        await self.run_agent([], hang=True, runner=runner, timeout=0.01)
        self.assertEqual(runner.commands, [])
        failed = await self.run_agent(
            [frame({"type": "start", "run": "r"})],
            hang=True,
            runner=runner,
            timeout=0.01,
        )
        self.assertEqual(failed.cancellation.server_exit_code, 1)

    async def test_protocol_frame_completeness_tracks_retention_and_drops(self) -> None:
        complete = await self.run_agent(
            [
                frame({"type": "start", "run": "r"}),
                frame({"type": "done", "status": "ok", "run": "r"}),
            ],
            retain=True,
        )
        self.assertTrue(complete.raw_frames_complete)
        self.assertEqual(complete.dropped_frame_count, 0)
        self.assertTrue(
            all("_observer_elapsed_seconds" in frame for frame in complete.frames)
        )

        long_text = "x" * 10_000
        preserved = await self.run_agent(
            [
                frame({"type": "start", "run": "r", "authorization": "secret"}),
                frame({"type": "delta", "run": "r", "text": long_text}),
                frame({"type": "done", "status": "ok", "run": "r"}),
            ],
            retain=True,
        )
        delta = next(item for item in preserved.frames if item["type"] == "delta")
        self.assertEqual(delta["text"], long_text)
        self.assertEqual(preserved.truncated_field_count, 0)
        self.assertEqual(preserved.redacted_field_count, 1)
        self.assertEqual(
            next(item for item in preserved.frames if item["type"] == "start")[
                "authorization"
            ],
            "[REDACTED]",
        )

        incomplete = await self.run_agent(
            [
                frame({"type": "start", "run": "r"}),
                frame({"type": "done", "status": "ok", "run": "r"}),
                frame({"type": "error", "code": "LATE", "run": "r"}),
            ],
            retain=True,
        )
        self.assertFalse(incomplete.raw_frames_complete)
        self.assertEqual(incomplete.dropped_frame_count, 1)

    async def test_late_frames_after_terminal_are_ignored(self) -> None:
        result = await self.run_agent(
            [
                frame({"type": "start", "run": "r"}),
                frame({"type": "done", "status": "ok", "run": "r"}),
                frame({"type": "error", "code": "LATE", "run": "r"}),
            ]
        )
        self.assertIsNone(result.error_code)

    async def test_real_collector_recoverable_errors_follow_terminal_outcome(
        self,
    ) -> None:
        recoverable = {
            "type": "error",
            "run": "r",
            "code": "ERETRY",
            "message": "first candidate failed",
            "recoverable": True,
        }
        later = {
            **recoverable,
            "code": "EFALLBACK",
            "message": "last candidate failed",
        }
        successful = await self.run_real_agent(
            [
                {"type": "start", "run": "r"},
                recoverable,
                later,
                {"type": "message", "run": "r", "role": "assistant", "content": "ok"},
                {"type": "done", "run": "r", "status": "success"},
            ]
        )
        self.assertTrue(successful.runtime_success)
        self.assertIsNone(successful.error_code)
        self.assertEqual(
            [item["code"] for item in successful.frames if item["type"] == "error"],
            ["ERETRY", "EFALLBACK"],
        )

        failed = await self.run_real_agent(
            [recoverable, later, {"type": "done", "run": "r", "status": "error"}]
        )
        self.assertFalse(failed.runtime_success)
        self.assertEqual(failed.error_code, "EFALLBACK")
        self.assertEqual(failed.error_message, "last candidate failed")

    async def test_real_collector_preserves_authoritative_failures(self) -> None:
        recoverable = {
            "type": "error",
            "run": "r",
            "code": "ERETRY",
            "message": "candidate failed",
            "recoverable": True,
        }
        fatal = {
            "type": "error",
            "run": "r",
            "code": "EFATAL",
            "message": "fatal failure",
        }
        nonrecoverable = await self.run_real_agent(
            [recoverable, fatal, {"type": "done", "run": "r", "status": "error"}]
        )
        self.assertEqual(nonrecoverable.error_code, "EFATAL")

        stderr = await self.run_real_agent([recoverable], exit_code=2, stderr="boom")
        self.assertEqual(stderr.error_code, "exit_2")
        self.assertEqual(stderr.error_message, "boom")

        exited = await self.run_real_agent([recoverable], exit_code=2)
        self.assertEqual(exited.error_code, "ERETRY")
        self.assertEqual(exited.error_message, "candidate failed")

        timed_out = await self.run_real_agent([recoverable], delay=0.2, timeout=0.01)
        self.assertTrue(timed_out.timed_out)
        self.assertEqual(timed_out.error_code, "ETIMEDOUT")

    async def test_outer_cancellation_cancels_server(self) -> None:
        runner = FakeRunner()
        process = FakeProcess([frame({"type": "start", "run": "outer"})], True)
        with (
            patch.object(
                agents.asyncio,
                "create_subprocess_exec",
                AsyncMock(return_value=process),
            ),
            patch.object(
                agents.os,
                "killpg",
                lambda _pid, _signal: setattr(process, "returncode", -15),
            ),
            patch.object(
                agents,
                "_process_group_members",
                side_effect=lambda _pgid: (
                    {process.pid: 1} if process.returncode is None else {}
                ),
            ),
        ):
            task = asyncio.create_task(
                agents.run_ctx_agent(
                    ctx_path="ctx",
                    agent_name="coder",
                    session="benchmark-owned",
                    prompt="x",
                    timeout=10,
                    command_runner=runner,
                )
            )
            await asyncio.sleep(0)
            await asyncio.sleep(0)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
        self.assertEqual(runner.commands[0][-1], "outer")

    async def test_sample_roles_remain_sequential(self) -> None:
        observed: list[str] = []

        def baseline(_agent_name: str, session: str):
            return agents.SessionBaseline(session, True, "default", ("default",))

        async def run(**values):
            observed.append(values["agent_name"])
            return agents.CtxRunResult(
                agent=values["agent_name"],
                session=values["session"],
                completion="answer",
                frames=(),
                runtime_success=True,
                exit_code=0,
                timed_out=False,
                latency_seconds=0.1,
                ttft_seconds=0.05,
                input_tokens=1,
                output_tokens=1,
                usage_available=True,
                raw_frames_complete=False,
                dropped_frame_count=0,
                dropped_frame_bytes=0,
                redacted_field_count=0,
                redacted_field_bytes=0,
                truncated_field_count=0,
                truncated_field_bytes=0,
                done_status="ok",
                error_code=None,
                error_message=None,
                stderr="",
                identity=agents.RunIdentity("client", "run", values["session"]),
                cancellation=None,
                process_cleanup=agents._empty_process_cleanup(),
                cleanup=None,
            )

        def archive(_ctx: str, _agent: str, baseline, _timeout: float):
            return agents.SessionReceipt(
                baseline.session,
                baseline,
                True,
                "done",
                True,
                True,
                current_after="default",
                candidates=(baseline.session,),
            )

        with (
            patch.object(system, "_session_baseline", side_effect=baseline),
            patch.object(system, "run_ctx_agent", side_effect=run),
            patch.object(system, "_archive_session", side_effect=archive),
        ):
            await system._samples(
                ctx_path="ctx",
                agents=["architect", "coder", "reviewer", "worker"],
                dataset=[{"id": "one", "prompt": "q", "target": "answer"}],
                repeat=1,
                timeout=1,
                measure_memory=False,
            )
        self.assertEqual(observed, ["architect", "coder", "reviewer", "worker"])

    async def test_generic_observation_error_still_archives_the_session(self) -> None:
        baseline = agents.SessionBaseline("owned", True, "default", ())
        receipt = agents.SessionReceipt(
            "owned",
            baseline,
            True,
            None,
            True,
            True,
            detail=None,
        )
        archive = patch.object(system, "_archive_session", return_value=receipt)
        with (
            patch.object(system, "_session_baseline", return_value=baseline),
            patch.object(
                system,
                "run_ctx_agent",
                AsyncMock(side_effect=ValueError("collector failed")),
            ),
            archive as archive_mock,
            self.assertRaisesRegex(ValueError, "collector failed"),
        ):
            await system._ctx_observation(
                ctx_path="ctx",
                agent_name="coder",
                sample={"id": "one", "prompt": "q", "target": "answer"},
                sample_index=0,
                epoch=0,
                timeout=1,
                retain_frames=False,
                measure_memory=False,
            )
        archive_mock.assert_called_once()

    async def test_interrupted_observation_surfaces_failed_session_cleanup(
        self,
    ) -> None:
        baseline = agents.SessionBaseline("owned", True, "default", ())
        receipt = agents.SessionReceipt(
            "owned",
            baseline,
            True,
            None,
            True,
            False,
            detail="archive failed",
        )
        with (
            patch.object(system, "_session_baseline", return_value=baseline),
            patch.object(
                system,
                "run_ctx_agent",
                AsyncMock(side_effect=asyncio.CancelledError),
            ),
            patch.object(system, "_archive_session", return_value=receipt),
            self.assertRaisesRegex(RuntimeError, "archive failed"),
        ):
            await system._ctx_observation(
                ctx_path="ctx",
                agent_name="coder",
                sample={"id": "one", "prompt": "q", "target": "answer"},
                sample_index=0,
                epoch=0,
                timeout=1,
                retain_frames=False,
                measure_memory=False,
            )

    async def test_runtime_process_observer_hashes_executing_process(self) -> None:
        completed = asyncio.create_task(asyncio.sleep(0))
        await completed
        with (
            patch.object(system, "_unit_cgroup_pids", return_value=(os.getpid(),)),
            patch.object(
                system,
                "_unit_runtime_snapshot",
                return_value={"complete": True, "invocation_id": "run-1"},
            ),
        ):
            evidence = await system._observe_unit_processes(
                "cortexfs-agent@coder.service", completed
            )
        self.assertTrue(evidence["complete"])
        observation = evidence["observations"][0]
        self.assertEqual(observation["pid"], os.getpid())
        self.assertEqual(len(observation["executable"]["sha256"]), 64)


class LifecycleSystemTests(unittest.TestCase):
    def test_benchmark_source_identity_covers_declared_modules(self) -> None:
        identity = system._benchmark_source_identity(["agents.py", "system.py"])
        self.assertEqual(len(identity["sha256"]), 64)
        self.assertEqual(set(identity["files"]), {"agents.py", "system.py"})

    def test_hashed_command_rejects_truncation_and_preserves_small_stdout(self) -> None:
        small = system._hashed_command(
            [sys.executable, "-c", "print('search')"],
            1,
            retain_stdout=True,
        )
        self.assertTrue(small["complete"])
        self.assertEqual(small["stdout"], "search\n")
        self.assertEqual(len(small["sha256"]), 64)

        oversized = system._hashed_command(
            [
                sys.executable,
                "-c",
                f"print('x' * {system.COMMAND_VALIDATION_STDOUT_LIMIT + 1})",
            ],
            1,
        )
        self.assertFalse(oversized["complete"])
        self.assertEqual(oversized["stdout"], "")

    def test_runtime_snapshot_rejects_inactive_or_pidless_units(self) -> None:
        with patch.object(
            system,
            "_systemctl_fields",
            return_value=(
                True,
                {"MainPID": "0", "InvocationID": "", "ActiveState": "inactive"},
                "hash",
            ),
        ):
            snapshot = system._unit_runtime_snapshot("cortexfs-agent@coder.service", 1)
        self.assertFalse(snapshot["complete"])

    def test_unit_configuration_hashes_fragment_dropin_exec_and_environment(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fragment = root / "agent.service"
            drop_in = root / "override.conf"
            executable = root / "runtime"
            fragment.write_text("[Service]\n", encoding="utf-8")
            drop_in.write_text("[Service]\n", encoding="utf-8")
            executable.write_bytes(b"runtime")
            executable.chmod(0o755)
            stdout = "\n".join(
                (
                    f"FragmentPath={fragment}",
                    f"DropInPaths={drop_in}",
                    f"ExecStart={{ path={executable} ; argv[]={executable} ; }}",
                    "Environment=CTX_MODE=test API_KEY=must-not-leak",
                    "EnvironmentFiles=",
                    "User=cortexfs",
                    "Group=cortexfs",
                )
            )
            with patch.object(
                system,
                "_hashed_command",
                return_value={
                    "success": True,
                    "complete": True,
                    "stdout": stdout,
                },
            ):
                identity = system._unit_configuration_identity(
                    "cortexfs-agent@coder.service", 1
                )
        self.assertTrue(identity["complete"])
        self.assertEqual(len(identity["configuration_sha256"]), 64)
        self.assertEqual(len(identity["files"]["executable"]["sha256"]), 64)
        self.assertNotIn("must-not-leak", repr(identity))

    def test_executable_identity_hashes_the_exact_binary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "ctx"
            executable.write_bytes(b"ctx-binary")
            executable.chmod(0o755)
            with patch.object(
                system,
                "_command",
                return_value={"success": True, "stdout": "ctx 1.2.3\n"},
            ):
                identity = system._executable_identity(str(executable), 1)
        self.assertEqual(identity["bytes"], 10)
        self.assertEqual(len(identity["sha256"]), 64)
        self.assertEqual(identity["version"], "ctx 1.2.3")

    def command_fixture(
        self, failure: str | None = None, status_output: str = "idle\nmodel=main\n"
    ):
        def command(values: list[str], _timeout: float, **_kwargs: object):
            key = " ".join(values)
            if values[1:] == ["status"]:
                stdout = "ctx\n    State: running\n   Status: ready\n  Mounted: yes\n"
            elif values[1:] == ["doctor"]:
                stdout = "ok root /ctx\nok status\nok agent/coder\n"
            elif values[0] == "findmnt":
                stdout = "/ctx cortexfs fuse rw,default_permissions,allow_other\n"
            elif " ping agent/" in f" {key}":
                stdout = '{"type":"pong"}\n'
            elif " agent status " in f" {key} ":
                stdout = status_output
            else:
                raise AssertionError(values)
            success = failure is None or failure not in key
            if not success:
                stdout = "bad\n"
            return {
                "command": values,
                "success": success,
                "exit_code": 0 if success else 1,
                "latency_seconds": 0.01,
                "stdout": stdout,
                "stderr": "",
            }

        return command

    def test_preflight_literal_outputs_and_role_ping_failure(self) -> None:
        with patch.object(system, "_command", side_effect=self.command_fixture()):
            health = system._preflight("ctx", ["architect", "coder"], 1)
        self.assertEqual(list(health["pings"]), ["architect", "coder"])
        with (
            patch.object(
                system, "_command", side_effect=self.command_fixture("ping agent/coder")
            ),
            self.assertRaisesRegex(ValueError, "ping"),
        ):
            system._preflight("ctx", ["architect", "coder"], 1)

    def test_preflight_rejects_dirty_status_doctor_and_mount(self) -> None:
        for failure, message in (
            ("ctx status", "status"),
            ("ctx doctor", "doctor"),
            ("findmnt", "FUSE"),
        ):
            with (
                self.subTest(failure=failure),
                patch.object(
                    system, "_command", side_effect=self.command_fixture(failure)
                ),
                self.assertRaisesRegex(ValueError, message),
            ):
                system._preflight("ctx", ["coder"], 1)

    def test_preflight_requires_exact_idle_first_status_line(self) -> None:
        with patch.object(
            system,
            "_command",
            side_effect=self.command_fixture(status_output="idle\nmodel=main\n"),
        ):
            system._preflight("ctx", ["coder"], 1)
        for status_output in (
            "mystery\nmodel=main\n",
            "",
            "idle active\nmodel=main\n",
            "active\nmodel=main\n",
        ):
            with (
                self.subTest(status_output=status_output),
                patch.object(
                    system,
                    "_command",
                    side_effect=self.command_fixture(status_output=status_output),
                ),
                self.assertRaisesRegex(ValueError, "must begin with idle"),
            ):
                system._preflight("ctx", ["coder"], 1)

    def session_tree(self, root: Path, current: str = "default") -> Path:
        session = root / "home" / "1000" / "agent" / "coder" / "session"
        (session / "index").mkdir(parents=True)
        (session / "index" / "current").write_text(current + "\n")
        (session / "index" / "list").write_text("default\n")
        return session

    def test_session_baseline_collision_current_active_refs_and_exact_archive(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            session_root = self.session_tree(root)
            baseline = system._session_baseline("coder", "owned", root=root, uid=1000)
            self.assertTrue(baseline.absent)
            owned = session_root / "owned"
            owned.mkdir()
            (owned / "state").write_text("active\n")
            active = system._archive_session(
                "ctx", "coder", baseline, 1, root=root, uid=1000
            )
            self.assertFalse(active.archive_allowed)
            (owned / "state").write_text("done\n")
            (session_root / "index" / "current").write_text("owned\n")
            current = system._archive_session(
                "ctx", "coder", baseline, 1, root=root, uid=1000
            )
            self.assertIn("select", current.detail)
            (session_root / "index" / "current").write_text("default\n")
            (owned / "context" / "child" / "one").mkdir(parents=True)
            referenced = system._archive_session(
                "ctx", "coder", baseline, 1, root=root, uid=1000
            )
            self.assertIn("references", referenced.detail)
            (owned / "context" / "child" / "one").rmdir()
            for preview in (
                "ctx: no matching agent sessions\n",
                "ctx: dry-run; pass --yes to archive\narchive wrong\n",
                "ctx: dry-run; pass --yes to archive\narchive owned\narchive wrong\n",
            ):
                with (
                    self.subTest(preview=preview),
                    patch.object(
                        system,
                        "_command",
                        return_value={"success": True, "stdout": preview, "stderr": ""},
                    ),
                ):
                    refused = system._archive_session(
                        "ctx", "coder", baseline, 1, root=root, uid=1000
                    )
                    self.assertFalse(refused.archive_allowed)
            with patch.object(
                system,
                "_command",
                side_effect=[
                    {
                        "success": True,
                        "stdout": "ctx: dry-run; pass --yes to archive\narchive owned\n",
                        "stderr": "",
                    },
                    {"success": True, "stdout": "archived owned\n", "stderr": ""},
                ],
            ):
                archived = system._archive_session(
                    "ctx", "coder", baseline, 1, root=root, uid=1000
                )
            self.assertTrue(archived.archived)
            with patch.object(
                system,
                "_command",
                side_effect=[
                    {
                        "success": True,
                        "stdout": "ctx: dry-run; pass --yes to archive\narchive owned\n",
                        "stderr": "",
                    },
                    {"success": True, "stdout": "archived wrong\n", "stderr": ""},
                ],
            ):
                mismatch = system._archive_session(
                    "ctx", "coder", baseline, 1, root=root, uid=1000
                )
            self.assertFalse(mismatch.archived)
            collision = system._session_baseline("coder", "owned", root=root, uid=1000)
            self.assertFalse(collision.absent)

    def test_current_owned_session_is_restored_then_archived(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            session_root = self.session_tree(root)
            (session_root / "default").mkdir()
            baseline = system._session_baseline("coder", "owned", root=root, uid=1000)
            owned = session_root / "owned"
            owned.mkdir()
            (owned / "state").write_text("done\n")
            (session_root / "index" / "current").write_text("owned\n")

            def command(args, _timeout):
                if "select" in args:
                    (session_root / "index" / "current").write_text("default\n")
                    return {"success": True, "stdout": "", "stderr": ""}
                if "--dry-run" in args:
                    return {
                        "success": True,
                        "stdout": "ctx: dry-run; pass --yes to archive\narchive owned\n",
                        "stderr": "",
                    }
                return {"success": True, "stdout": "archived owned\n", "stderr": ""}

            with patch.object(system, "_command", side_effect=command) as invoked:
                receipt = system._archive_session(
                    "ctx", "coder", baseline, 1, root=root, uid=1000
                )
            self.assertTrue(receipt.archived)
            self.assertEqual(receipt.current_after, "default")
            self.assertEqual(invoked.call_count, 3)

    def test_current_restore_cas_and_baseline_failures_retain_session(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            session_root = self.session_tree(root)
            (session_root / "default").mkdir()
            baseline = system._session_baseline("coder", "owned", root=root, uid=1000)
            owned = session_root / "owned"
            owned.mkdir()
            (owned / "state").write_text("done\n")
            (session_root / "index" / "current").write_text("owned\n")
            for stderr in ("EAGAIN current mismatch", "ENOENT baseline missing"):
                with (
                    self.subTest(stderr=stderr),
                    patch.object(
                        system,
                        "_command",
                        return_value={"success": False, "stdout": "", "stderr": stderr},
                    ) as invoked,
                ):
                    receipt = system._archive_session(
                        "ctx", "coder", baseline, 1, root=root, uid=1000
                    )
                self.assertFalse(receipt.archive_allowed)
                self.assertIn(stderr.split()[0], receipt.detail)
                self.assertEqual(invoked.call_count, 1)
                self.assertTrue(owned.is_dir())

            missing_baseline = agents.SessionBaseline(
                "owned", True, "missing", ("default",)
            )
            with patch.object(system, "_command") as invoked:
                receipt = system._archive_session(
                    "ctx", "coder", missing_baseline, 1, root=root, uid=1000
                )
            self.assertIn("baseline", receipt.detail)
            invoked.assert_not_called()

    def test_gc_preview_zero_multiple_wrong_and_sanitization(self) -> None:
        self.assertEqual(
            system._gc_candidates(
                "ctx: dry-run; pass --yes to archive\narchive owned\n"
            ),
            ("owned",),
        )
        self.assertEqual(system._gc_candidates("ctx: no matching agent sessions\n"), ())
        self.assertEqual(
            system._gc_candidates(
                "ctx: dry-run; pass --yes to archive\narchive one\narchive two\n"
            ),
            ("one", "two"),
        )
        sanitized = agents.sanitize(
            {
                "provider_api_key": "value",
                "nested": {"token": "nested-value"},
                "detail": (
                    "Authorization: abc api_key=def provider_secret: ghi "
                    "password = jkl cookie: mno Bearer xyz "
                    "tokenization: enabled cookiejar=value " + "x" * 5000
                ),
            }
        )
        rendered = json.dumps(sanitized)
        for secret in ("abc", "def", "ghi", "jkl", "mno", "xyz", "nested-value"):
            self.assertNotIn(secret, rendered)
        self.assertIn("tokenization: enabled", rendered)
        self.assertIn("cookiejar=value", rendered)
        self.assertLessEqual(len(sanitized["detail"].encode()), agents.MAX_ERROR_BYTES)

    def test_cleanup_failure_is_nonzero_and_preflight_aborts_main_early(self) -> None:
        with self.assertRaisesRegex(SystemExit, "cleanup failures"):
            system._require_cleanup([{"cleanup_success": False}])
        dataset = (
            Path(__file__).resolve().parents[1]
            / "agent_benchmark/data/agent_benchmark.jsonl"
        )
        argv = ["ctx-agent-benchmark", "--dataset", str(dataset)]
        with (
            patch.object(sys, "argv", argv),
            patch.object(system, "_preflight", side_effect=ValueError("bad preflight")),
            patch.object(system.OutputDirectory, "create") as output,
            patch.object(system.asyncio, "run") as sampling,
            self.assertRaises(SystemExit),
        ):
            system.main()
        output.assert_not_called()
        sampling.assert_not_called()


if __name__ == "__main__":
    unittest.main(verbosity=2)
