"""Focused regression tests for harness timeout evidence."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

import run


class TimeoutEvidenceTests(unittest.TestCase):
    def test_timeout_records_inflight_test_and_process_group(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            log = root / "workspace.log"
            code = (
                "import sys,time; "
                "sys.stdout.write('test sample::stuck ... '); sys.stdout.flush(); "
                "time.sleep(30)"
            )
            result = run.execute([sys.executable, "-c", code], log, 2)

            self.assertEqual(result["outcome"], "timeout")
            evidence = json.loads((root / result["timeout_evidence"]).read_text())
            self.assertEqual(evidence["active_test"], "sample::stuck")
            self.assertTrue(evidence["processes"])
            self.assertEqual(evidence["process_group"], evidence["processes"][0]["pid"])
            self.assertIn("wchan", evidence["processes"][0])


if __name__ == "__main__":
    unittest.main()
