"""Focused regression coverage for timeout thread evidence."""

import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import run


class ThreadEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def test_process_snapshot_includes_thread_state(self):
        snapshot = run.process_snapshot(__import__("os").getpid())
        self.assertTrue(snapshot["threads"])
        current = next(thread for thread in snapshot["threads"] if thread["tid"] == snapshot["pid"])
        self.assertIn(current["state"], {"R", "S"})
        self.assertTrue(current["comm"])
        self.assertIn("wchan", current)
        self.assertIn("syscall", current)

    def test_timeout_evidence_captures_waiting_worker_thread(self):
        code = """
import threading
import time

ready = threading.Event()
stop = threading.Event()

def worker():
    ready.set()
    stop.wait()

threading.Thread(target=worker, name="fixture-worker").start()
ready.wait()
print("ready", flush=True)
time.sleep(30)
"""
        log = self.root / "threads.log"
        with patch.object(run, "PROGRESS_INTERVAL", 0.05):
            result = run.execute([sys.executable, "-c", code], log, 0.5)
        self.assertEqual((result["status"], result["outcome"]), ("failed", "timeout"))
        evidence = json.loads(log.with_suffix(".timeout.json").read_text())
        self.assertEqual(evidence["schema"], "cortexfs.harness-timeout/v1")
        self.assertEqual(len(evidence["processes"]), 1)
        threads = evidence["processes"][0]["threads"]
        self.assertGreaterEqual(len(threads), 2)
        self.assertEqual([thread["tid"] for thread in threads], sorted(thread["tid"] for thread in threads))
        self.assertTrue(all("state" in thread and "wchan" in thread for thread in threads))


if __name__ == "__main__":
    unittest.main()
