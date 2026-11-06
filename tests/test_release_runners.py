"""Check cargo-dist's release matrix using the version installed in PATH."""

import json
from pathlib import Path
import subprocess
import unittest


class ReleaseRunnerTest(unittest.TestCase):
    def test_macos_targets_use_supported_native_runners(self):
        root = Path(__file__).resolve().parent.parent
        result = subprocess.run(
            ["dist", "plan", "--output-format=json"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        )
        matrix = json.loads(result.stdout)["ci"]["github"]["artifacts_matrix"]["include"]

        for target, supported_runners in (
            ("aarch64-apple-darwin", {"macos-15", "macos-26"}),
            ("x86_64-apple-darwin", {"macos-15-intel", "macos-26-intel"}),
        ):
            with self.subTest(target=target):
                jobs = [job for job in matrix if target in job["targets"]]
                self.assertEqual(len(jobs), 1)
                self.assertEqual(jobs[0]["host"], target)
                self.assertIn(jobs[0]["runner"], supported_runners)


if __name__ == "__main__":
    unittest.main()
