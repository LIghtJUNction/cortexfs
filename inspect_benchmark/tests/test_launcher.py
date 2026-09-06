from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class LauncherTests(unittest.TestCase):
    def test_ctx_default_and_explicit_agents_preserve_inspect_options(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            launcher = root / "run_benchmark.sh"
            shutil.copyfile(
                Path(__file__).resolve().parents[1] / "run_benchmark.sh", launcher
            )
            binaries = root / ".venv" / "bin"
            binaries.mkdir(parents=True)
            (binaries / "python").symlink_to(sys.executable)
            inspect = binaries / "inspect"
            inspect.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            inspect.chmod(0o755)
            for options, agent, forwarded in (
                ([], "executor", []),
                (["--limit", "2"], "executor", ["--limit", "2"]),
                (["architect", "--limit", "2"], "architect", ["--limit", "2"]),
            ):
                with self.subTest(options=options):
                    result = subprocess.run(
                        ["bash", str(launcher), "ctx", *options],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        check=True,
                    )
                    self.assertEqual(
                        result.stdout.splitlines(),
                        [
                            "eval",
                            "agent_benchmark/tasks.py@agent_benchmark",
                            "--model",
                            "mockllm/model",
                            "--solver",
                            "agent_benchmark/ctx_cli_agent",
                            "-S",
                            f"agent_name={agent}",
                            *forwarded,
                        ],
                    )


if __name__ == "__main__":
    unittest.main()
