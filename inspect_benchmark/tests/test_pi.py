from __future__ import annotations

import hashlib
import json
import os
import stat
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

from agent_benchmark.pi import (
    PiJsonlRunner,
    _pi_settings_identity,
    _proc_start_time,
    _process_tree_rss,
)


class PiJsonlRunnerTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def runner(self, executable: Path, *, timeout: float = 1.0) -> PiJsonlRunner:
        return PiJsonlRunner(
            pi_path=str(executable),
            provider="openai-codex",
            model="fixture-model",
            thinking="medium",
            timeout=timeout,
            workspace=str(self.root),
            owned_memory=False,
        )

    def owned_backend(
        self,
        evidence: dict[str, object],
        *,
        preflight: bool = True,
        wait_for_stop: bool = False,
    ) -> type:
        class FakeOwnedUnit:
            def __init__(self, *, command: list[str], **_kwargs: object) -> None:
                self.command = command

            async def preflight(self) -> bool:
                return preflight

            async def observe(
                self, _wrapper: object, stop: object
            ) -> dict[str, object]:
                if wait_for_stop:
                    await stop.wait()  # type: ignore[attr-defined]
                return evidence

        return FakeOwnedUnit

    def executable(
        self,
        frames: list[dict[str, object]],
        *,
        delay: float = 0.0,
        exit_code: int = 0,
        stderr: str = "",
    ) -> Path:
        path = self.root / f"pi-{len(list(self.root.glob('pi-*')))}"
        source = textwrap.dedent(
            f"""\
            #!/usr/bin/env python3
            import json
            import sys
            import time

            time.sleep({delay!r})
            for frame in {frames!r}:
                print(json.dumps(frame), flush=True)
            sys.stderr.write({stderr!r})
            raise SystemExit({exit_code})
            """
        )
        path.write_text(source, encoding="utf-8")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    async def test_parses_authoritative_completion_usage_ttft_and_rss(self) -> None:
        frames = [
            {"type": "session", "api_key": "secret-value"},
            {
                "type": "message_update",
                "assistantMessageEvent": {"type": "thinking_delta", "delta": "x"},
            },
            {
                "type": "message_update",
                "assistantMessageEvent": {"type": "text_delta", "delta": "4"},
            },
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "4"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {
                        "input": 7,
                        "output": 1,
                        "cacheRead": 2,
                        "cacheWrite": 3,
                        "totalTokens": 13,
                        "outputDetails": {"reasoning": 0},
                        "cost": {"total": 0.001},
                    },
                },
            },
            {"type": "agent_settled"},
        ]
        result = await self.runner(self.executable(frames)).run("2+2")

        self.assertTrue(result.runtime_success)
        self.assertEqual(result.completion, "4")
        self.assertIsNotNone(result.ttft_seconds)
        self.assertEqual(result.usage.input_tokens, 7)
        self.assertEqual(result.usage.cache_read_tokens, 2)
        self.assertEqual(result.usage.provider_catalog_cost_usd, 0.001)
        self.assertGreater(result.peak_rss_bytes or 0, 0)
        self.assertFalse(result.raw_frames_complete)
        self.assertFalse(result.metadata()["memory_complete"])
        self.assertNotIn("secret-value", json.dumps(result.frames))
        self.assertIsNone(result.metadata()["attributable_subscription_cost_usd"])

    async def test_owned_unit_fake_marks_complete_memory(self) -> None:
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "ok"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {"input": 1, "output": 1, "totalTokens": 2},
                },
            },
            {"type": "agent_settled"},
        ]
        evidence = {
            "complete": True,
            "memory_peak_bytes": 4096,
            "memory_events": {"oom": 0, "oom_kill": 0},
            "cleanup": {"complete": True},
        }
        with patch("agent_benchmark.pi.OwnedUserUnit", self.owned_backend(evidence)):
            runner = PiJsonlRunner(
                pi_path=str(self.executable(frames)),
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=1,
                workspace=str(self.root),
            )
            result = await runner.run("prompt")
        self.assertTrue(result.memory_complete)
        self.assertEqual(result.peak_memory_bytes, 4096)
        self.assertEqual(result.metadata()["memory"], evidence)

    async def test_owned_unit_reuse_fails_closed(self) -> None:
        evidence = {"complete": False, "issue": "reuse"}
        with patch(
            "agent_benchmark.pi.OwnedUserUnit",
            self.owned_backend(evidence, preflight=False),
        ):
            runner = PiJsonlRunner(
                pi_path="unused",
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=1,
                workspace=str(self.root),
            )
            result = await runner.run("prompt")
        self.assertEqual(result.error_code, "memory_unavailable")
        self.assertFalse(result.memory_complete)

    async def test_owned_unit_foreign_pid_fails_closed(self) -> None:
        evidence = {
            "complete": False,
            "memory_peak_bytes": 4096,
            "foreign_pids": (999,),
            "cleanup": {"complete": True},
        }
        with patch("agent_benchmark.pi.OwnedUserUnit", self.owned_backend(evidence)):
            result = await PiJsonlRunner(
                pi_path=str(self.executable([])),
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=1,
                workspace=str(self.root),
            ).run("prompt")
        self.assertFalse(result.memory_complete)
        self.assertEqual(result.memory["foreign_pids"], (999,))

    async def test_owned_unit_timeout_stops_and_cleans(self) -> None:
        evidence = {
            "complete": True,
            "memory_peak_bytes": 4096,
            "memory_events": {"oom": 0, "oom_kill": 0},
            "cleanup": {"complete": True},
        }
        with patch(
            "agent_benchmark.pi.OwnedUserUnit",
            self.owned_backend(evidence, wait_for_stop=True),
        ):
            result = await PiJsonlRunner(
                pi_path=str(self.executable([], delay=5)),
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=0.05,
                workspace=str(self.root),
            ).run("prompt")
        self.assertTrue(result.timed_out)
        self.assertTrue(result.memory["cleanup"]["complete"])  # type: ignore[index]
        self.assertTrue(result.memory_complete)

    async def test_owned_unit_cleanup_failure_is_incomplete(self) -> None:
        evidence = {
            "complete": False,
            "memory_peak_bytes": 4096,
            "cleanup": {"complete": False},
        }
        with patch("agent_benchmark.pi.OwnedUserUnit", self.owned_backend(evidence)):
            result = await PiJsonlRunner(
                pi_path=str(self.executable([])),
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=1,
                workspace=str(self.root),
            ).run("prompt")
        self.assertFalse(result.memory_complete)

    async def test_owned_unit_missing_peak_is_incomplete(self) -> None:
        evidence = {
            "complete": False,
            "memory_peak_bytes": None,
            "memory_events": {"oom": 0, "oom_kill": 0},
            "cleanup": {"complete": True},
        }
        with patch("agent_benchmark.pi.OwnedUserUnit", self.owned_backend(evidence)):
            result = await PiJsonlRunner(
                pi_path=str(self.executable([])),
                provider="openai-codex",
                model="fixture-model",
                thinking="medium",
                timeout=1,
                workspace=str(self.root),
            ).run("prompt")
        self.assertFalse(result.memory_complete)
        self.assertIsNone(result.peak_memory_bytes)

    async def test_retain_frames_reports_complete_bounded_evidence(self) -> None:
        frames = [
            {
                "type": "message_update",
                "assistantMessageEvent": {"type": "text_delta", "delta": "answer"},
            },
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "answer"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {"input": 1, "output": 1, "totalTokens": 2},
                },
            },
            {"type": "agent_settled"},
        ]
        executable = self.executable(frames)
        runner = PiJsonlRunner(
            pi_path=str(executable),
            provider="openai-codex",
            model="fixture-model",
            thinking="medium",
            timeout=1,
            workspace=str(self.root),
            retain_frames=True,
            owned_memory=False,
        )
        result = await runner.run("prompt")
        self.assertTrue(result.raw_frames_complete)
        self.assertEqual(result.dropped_frame_count, 0)
        self.assertEqual(len(result.frames), len(frames))
        self.assertTrue(
            all("_observer_elapsed_seconds" in frame for frame in result.frames)
        )

    async def test_retained_frames_preserve_long_nonsecret_text_and_count_redaction(
        self,
    ) -> None:
        long_text = "x" * 10_000
        frames = [
            {
                "type": "message_update",
                "authorization": "secret",
                "assistantMessageEvent": {"type": "text_delta", "delta": long_text},
            },
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": long_text}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {"input": 1, "output": 1, "totalTokens": 2},
                },
            },
            {"type": "agent_settled"},
        ]
        runner = PiJsonlRunner(
            pi_path=str(self.executable(frames)),
            provider="openai-codex",
            model="fixture-model",
            thinking="medium",
            timeout=1,
            workspace=str(self.root),
            retain_frames=True,
            owned_memory=False,
        )
        result = await runner.run("prompt")
        event = result.frames[0]["assistantMessageEvent"]
        self.assertEqual(event["delta"], long_text)
        self.assertEqual(result.truncated_field_count, 0)
        self.assertEqual(result.redacted_field_count, 1)
        self.assertEqual(result.frames[0]["authorization"], "[REDACTED]")

    async def test_exit_zero_model_error_is_not_runtime_success(self) -> None:
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "partial"}],
                    "stopReason": "error",
                },
            },
            {"type": "agent_settled"},
        ]
        result = await self.runner(self.executable(frames)).run("prompt")

        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "model_error")
        self.assertEqual(result.exit_code, 0)

    async def test_missing_agent_settled_is_protocol_failure(self) -> None:
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "answer"}],
                    "stopReason": "stop",
                },
            }
        ]
        result = await self.runner(self.executable(frames)).run("prompt")

        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "missing_settled")

    async def test_old_error_answer_cannot_be_reused_by_empty_final_stop(self) -> None:
        usage = {"input": 1, "output": 1, "totalTokens": 2}
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "target"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "error",
                    "usage": usage,
                },
            },
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": usage,
                },
            },
            {"type": "agent_settled"},
        ]
        result = await self.runner(self.executable(frames)).run("prompt")

        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "missing_final_message")
        self.assertEqual(result.completion, "")

    async def test_settled_must_follow_the_final_message(self) -> None:
        frames = [
            {"type": "agent_settled"},
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "answer"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {"input": 1, "output": 1, "totalTokens": 2},
                },
            },
        ]
        result = await self.runner(self.executable(frames)).run("prompt")
        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "missing_settled")

    async def test_incomplete_usage_is_a_protocol_failure(self) -> None:
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "answer"}],
                    "provider": "openai-codex",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {},
                },
            },
            {"type": "agent_settled"},
        ]
        result = await self.runner(self.executable(frames)).run("prompt")
        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "protocol_error")

    async def test_response_provider_and_model_must_match_request(self) -> None:
        frames = [
            {
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "answer"}],
                    "provider": "other",
                    "model": "fixture-model",
                    "stopReason": "stop",
                    "usage": {"input": 1, "output": 1, "totalTokens": 2},
                },
            },
            {"type": "agent_settled"},
        ]
        result = await self.runner(self.executable(frames)).run("prompt")
        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "provider_mismatch")

    async def test_timeout_terminates_process_group(self) -> None:
        executable = self.executable([], delay=5.0)
        result = await self.runner(executable, timeout=0.05).run("prompt")

        self.assertTrue(result.timed_out)
        self.assertEqual(result.error_code, "ETIMEDOUT")
        self.assertNotEqual(result.exit_code, 0)

    async def test_invalid_json_is_protocol_failure(self) -> None:
        executable = self.root / "pi-invalid"
        executable.write_text("#!/bin/sh\nprintf 'not-json\\n'\n", encoding="utf-8")
        executable.chmod(0o700)

        result = await self.runner(executable).run("prompt")

        self.assertFalse(result.runtime_success)
        self.assertEqual(result.error_code, "protocol_error")

    def test_command_disables_ambient_resources_and_sessions(self) -> None:
        runner = PiJsonlRunner(
            pi_path="/opt/pi",
            provider="provider",
            model="model",
            thinking="high",
            timeout=1,
            system_prompt="system",
        )
        command = runner.command("prompt")

        self.assertEqual(command[:2], ["/opt/pi", "--mode"])
        for flag in (
            "--no-tools",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--no-approve",
            "--no-session",
        ):
            self.assertIn(flag, command)
        self.assertEqual(command[-2:], ["--", "prompt"])
        self.assertNotIn("system", runner.identity().values())
        with patch.dict(
            os.environ,
            {"HOME": "/home/fixture", "PATH": "/bin", "UNRELATED_SECRET": "x"},
            clear=True,
        ):
            keys = runner.identity()["environment_keys"]
        self.assertEqual(keys, ["HOME", "LANG", "NO_COLOR", "PATH", "PI_OFFLINE", "TZ"])

    def test_settings_identity_hashes_settings_without_touching_auth(self) -> None:
        settings = self.root / "settings.json"
        settings.write_text('{"theme":"fixture"}', encoding="utf-8")
        auth = self.root / "auth.json"
        auth.write_text("must-not-be-read", encoding="utf-8")
        auth.chmod(0)
        with patch.dict(
            os.environ, {"PI_CODING_AGENT_DIR": str(self.root)}, clear=True
        ):
            identity = _pi_settings_identity()
        self.assertTrue(identity["available"])
        self.assertTrue(identity["complete"])
        self.assertEqual(identity["state"], "present")
        self.assertEqual(
            identity["sha256"], hashlib.sha256(settings.read_bytes()).hexdigest()
        )
        self.assertNotIn("auth", json.dumps(identity))

        settings.unlink()
        settings.symlink_to(auth)
        with patch.dict(
            os.environ, {"PI_CODING_AGENT_DIR": str(self.root)}, clear=True
        ):
            invalid = _pi_settings_identity()
        self.assertFalse(invalid["available"])
        self.assertFalse(invalid["complete"])

        settings.unlink()
        with patch.dict(
            os.environ, {"PI_CODING_AGENT_DIR": str(self.root)}, clear=True
        ):
            absent = _pi_settings_identity()
        self.assertTrue(absent["complete"])
        self.assertEqual(absent["state"], "controlled-absence")

    def test_process_tree_rss_binds_root_start_time(self) -> None:
        start_time = _proc_start_time(os.getpid())
        self.assertIsNotNone(start_time)
        assert start_time is not None
        self.assertGreater(_process_tree_rss(os.getpid(), start_time) or 0, 0)
        self.assertIsNone(_process_tree_rss(os.getpid(), "wrong"))


if __name__ == "__main__":
    unittest.main()
