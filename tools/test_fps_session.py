"""Check the reported median and the shareable 12+ fps figure on synthetic logs."""
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class SessionReportTests(unittest.TestCase):
    def test_reported_statistics(self):
        cases = [
            ([10, 20], 15, 50),
            ([2.6, 2.7], 2.65, 0),
            ([12, 15, 18], 15, 100),
            ([11, 12, 15, 18, 11, 11], 11.5, 50),
            ([9], 9, 0),
        ]
        script = Path(__file__).with_name("fps-session.py")
        for rates, median, percent in cases:
            with self.subTest(rates=rates), tempfile.TemporaryDirectory() as directory:
                log = Path(directory) / "session.log"
                report = Path(directory) / "report.json"
                start = datetime(2026, 11, 26, tzinfo=timezone.utc)
                lines = []
                for second, fps in enumerate(rates):
                    timestamp = (start + timedelta(seconds=second)).isoformat()
                    lines.append(f"{timestamp} INFO {fps} fps, {1000 / fps:.3f} ms/frame, 1000 draws/frame\n")
                log.write_text("".join(lines), encoding="utf8")
                output = subprocess.check_output(
                    [sys.executable, str(script), str(log), "--out", str(report)], text=True
                )
                self.assertEqual(json.loads(report.read_text(encoding="utf8"))["median"], median)
                self.assertIn(f"time at 12+ fps: {percent} %", output)


if __name__ == "__main__":
    unittest.main()
