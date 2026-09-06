"""Regression tests for the evidence runner; these do not evaluate CortexFS."""

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import run


SUCCESS = (
    "running 1 test\n"
    "test sample::contract ... ok\n\n"
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
)
SUITE = {"id": "sample", "title": "Sample", "cargo_args": ["-p", "sample"],
         "required_tests": ["sample::contract"]}


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def invoke(self, text=SUCCESS, exit_code=0, timeout=5):
        command = [sys.executable, "-c", f"print({text!r}); raise SystemExit({exit_code})"]
        return run.execute(command, self.root / "cargo.log", timeout)

    def test_collects_evidence_from_multiple_test_binaries(self):
        result = self.invoke(SUCCESS + SUCCESS.replace("sample::contract", "other::test"))
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["summary_count"], 2)
        self.assertEqual(result["counts"]["passed"], 2)
        self.assertEqual(run.assess(SUITE, result)["status"], "passed")

    def test_exit_failure_cannot_be_overruled_by_success_text(self):
        result = self.invoke(exit_code=1)
        self.assertEqual(run.assess(SUITE, result)["status"], "failed")

    def test_zero_matching_tests_is_a_failure(self):
        result = self.invoke("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out;")
        self.assertEqual(result["status"], "failed")

    def test_no_summary_is_a_failure_even_with_zero_exit(self):
        self.assertEqual(self.invoke("finished compiling")["status"], "failed")

    def test_missing_or_ignored_required_tests_fail(self):
        for status in ("ignored", "FAILED", "unknown"):
            with self.subTest(status=status):
                result = {"status": "passed", "tests": {"sample::contract": [status]}}
                self.assertEqual(run.assess(SUITE, result)["status"], "failed")
        self.assertEqual(run.assess(SUITE, {"status": "passed", "tests": {}})["status"], "failed")
        self.assertEqual(run.assess(SUITE, None)["status"], "not_run")

    def test_duplicate_test_names_do_not_hide_failure(self):
        result = {"status": "passed", "tests": {"sample::contract": ["ok", "FAILED"]}}
        self.assertEqual(run.assess(SUITE, result)["status"], "failed")

    def test_spawn_failure_retains_a_failure_record(self):
        result = run.execute([str(self.root / "missing")], self.root / "missing.log", 1)
        self.assertEqual((result["status"], result["outcome"]), ("failed", "error"))
        self.assertIsNotNone(result["error"])

    def test_progress_keeps_complete_raw_log_and_success_evidence(self):
        code = f"import sys,time; sys.stdout.write({SUCCESS!r}); sys.stdout.flush(); time.sleep(0.12)"
        log = self.root / "progress.log"
        console = io.StringIO()
        with patch.object(run, "PROGRESS_INTERVAL", 0.02), redirect_stdout(console):
            result = run.execute([sys.executable, "-c", code], log, 5)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(log.read_text(), SUCCESS)
        self.assertIn("progress: running for ", console.getvalue())
        self.assertIn(f"log {len(SUCCESS.encode())} bytes", console.getvalue())

    def test_timeout_cleans_descendants_when_parent_exits_on_term(self):
        child = "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(30)"
        code = (
            "import subprocess,sys,time; "
            f"p=subprocess.Popen([sys.executable,'-c',{child!r}]); "
            "print(p.pid,flush=True); time.sleep(30)"
        )
        log = self.root / "timeout.log"
        console = io.StringIO()
        with patch.object(run, "PROGRESS_INTERVAL", 0.05), redirect_stdout(console):
            result = run.execute([sys.executable, "-c", code], log, 0.5)
        self.assertIn("timeout: running for ", console.getvalue())
        self.assertEqual((result["status"], result["outcome"]), ("failed", "timeout"))
        pid = log.read_text().strip()
        state = Path(f"/proc/{pid}/stat")
        # Killed descendants may briefly remain zombies until their init reaps them.
        if state.exists():
            for _ in range(100):
                if not state.exists() or state.read_text().split(") ", 1)[1][0] == "Z":
                    break
                time.sleep(0.01)
            self.assertTrue(not state.exists() or state.read_text().split(") ", 1)[1][0] == "Z")

    def test_workspace_mode_keeps_full_ci_gate_and_writes_reports(self):
        output = self.root / "results"
        invocation = self.invoke()
        with patch.object(run, "load_suites", return_value=[SUITE]), \
             patch.object(run, "identify", return_value="fixture"), \
             patch.object(run, "execute", return_value=invocation) as execute, \
             redirect_stdout(io.StringIO()):
            self.assertEqual(run.main(["--workspace", "--output", str(output)]), 0)
        command = execute.call_args.args[0]
        self.assertEqual(command[1:], ["cargo", "test", "--locked", "--workspace", "--all-targets",
                                      "--all-features", "--", "--test-threads=1", "--format=pretty", "--color=never"])
        report = json.loads((output / "report.json").read_text())
        self.assertEqual(report["schema"], "cortexfs.harness-evaluation/v1")
        self.assertEqual(report["suites"][0]["status"], "passed")
        self.assertIn("1/1", (output / "report.md").read_text())

    def test_timeout_report_is_written_and_later_suites_are_not_run(self):
        output = self.root / "results"
        invocation = self.invoke(exit_code=1)
        invocation["outcome"] = "timeout"
        with patch.object(run, "load_suites", return_value=[SUITE, {**SUITE, "id": "later"}]), \
             patch.object(run, "identify", return_value="fixture"), \
             patch.object(run, "execute", return_value=invocation) as execute, \
             redirect_stdout(io.StringIO()):
            self.assertEqual(run.main(["--output", str(output)]), 1)
        self.assertEqual(execute.call_count, 1)
        report = json.loads((output / "report.json").read_text())
        self.assertEqual([suite["status"] for suite in report["suites"]], ["failed", "not_run"])

    def test_existing_output_directory_is_never_reused(self):
        with redirect_stdout(io.StringIO()), patch("sys.stderr", new=io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                run.main(["--output", str(self.root)])
        self.assertEqual(error.exception.code, 2)

    def test_manifest_contracts_are_unique_and_reference_real_test_sources(self):
        suites = run.load_suites()
        names = [name for suite in suites for name in suite["required_tests"]]
        self.assertEqual(len(names), len(set(names)))
        for suite in suites:
            fixture_text = "\n".join((run.ROOT / file).read_text() for file in suite["fixtures"])
            for name in suite["required_tests"]:
                self.assertIn("fn " + name.split("::")[-1] + "(", fixture_text)


if __name__ == "__main__":
    unittest.main()
