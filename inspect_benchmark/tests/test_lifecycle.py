from __future__ import annotations

import asyncio
import importlib
import json
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
        ):
            self.assertEqual((await self.run_agent([bad])).error_code, "protocol_error")

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

    async def test_late_frames_after_terminal_are_ignored(self) -> None:
        result = await self.run_agent(
            [
                frame({"type": "start", "run": "r"}),
                frame({"type": "done", "status": "ok", "run": "r"}),
                frame({"type": "error", "code": "LATE", "run": "r"}),
            ]
        )
        self.assertIsNone(result.error_code)

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
                done_status="ok",
                error_code=None,
                error_message=None,
                stderr="",
                identity=agents.RunIdentity("client", "run", values["session"]),
                cancellation=None,
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
            )
        self.assertEqual(observed, ["architect", "coder", "reviewer", "worker"])


class LifecycleSystemTests(unittest.TestCase):
    def command_fixture(
        self, failure: str | None = None, status_output: str = "idle\nmodel=main\n"
    ):
        def command(values: list[str], _timeout: float):
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
        with patch.object(
            system, "_command", side_effect=self.command_fixture("ping agent/coder")
        ):
            with self.assertRaisesRegex(ValueError, "ping"):
                system._preflight("ctx", ["architect", "coder"], 1)

    def test_preflight_rejects_dirty_status_doctor_and_mount(self) -> None:
        for failure, message in (
            ("ctx status", "status"),
            ("ctx doctor", "doctor"),
            ("findmnt", "FUSE"),
        ):
            with self.subTest(failure=failure):
                with patch.object(
                    system, "_command", side_effect=self.command_fixture(failure)
                ):
                    with self.assertRaisesRegex(ValueError, message):
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
            ):
                with self.assertRaisesRegex(ValueError, "must begin with idle"):
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
            self.assertIn("current", current.detail)
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
