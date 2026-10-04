"""Runner unit tests: no psutil installation, executable launch or model required."""
import contextlib
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import neural_benchmark_runner as runner


class RunnerTests(unittest.TestCase):
    def args(self, root, *extra):
        return runner.parser().parse_args([
            "--exe", str(root / "neural_benchmark.exe"), "--model-dir", str(root),
            "--profile", "low-memory", *extra,
        ])

    def test_explicit_paths_defaults_and_options(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "neural_benchmark.exe").touch()
            args = self.args(root, "--ink-repeat", "8", "--prepacking", "false", "--cpu", "2")
            command = runner.command_for(args)
            self.assertEqual(args.timeout, 240)
            self.assertEqual(command[0], str((root / "neural_benchmark.exe").resolve()))
            self.assertEqual(command[-2:], ["--prepacking", "false"])
            self.assertEqual(command[command.index("--ink-repeat") + 1], "8")
            args.exe = root / "blackboard.exe"
            args.exe.touch()
            with self.assertRaises(ValueError):
                runner.command_for(args)

    def test_invalid_arguments_without_psutil(self):
        for extra in [("--ink-repeat", "0"), ("--ink-repeat", "9"),
                      ("--timeout", "0"), ("--timeout", "nan"),
                      ("--timeout", "inf"), ("--cpu", "4"),
                      ("--process-memory-limit", "512")]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                self.args(Path("explicit"), *extra)

    def test_peaks_and_unavailable_fields(self):
        peaks = dict.fromkeys(("rss", "private", "peak_wset", "peak_pagefile"))
        runner.update_peaks(peaks, SimpleNamespace(rss=10))
        self.assertEqual(peaks, dict(rss=10, private=None, peak_wset=None, peak_pagefile=None))
        runner.update_peaks(peaks, SimpleNamespace(rss=5, private=20, peak_wset=30, peak_pagefile=40))
        self.assertEqual(peaks, dict(rss=10, private=20, peak_wset=30, peak_pagefile=40))

    def test_timeout_kills_only_owned_tree_and_captures_json(self):
        killed = []

        class Process:
            pid = 123
            returncode = None

            def children(self, recursive):
                self.assert_recursive = recursive
                return [child]

            def kill(self):
                killed.append(self.pid)
                self.returncode = -1

            def poll(self):
                return self.returncode

            def memory_info(self):
                return SimpleNamespace(rss=100, private=80, peak_wset=120, peak_pagefile=90)

        process = Process()
        child = Process()
        child.pid = 124

        def popen(command, **kwargs):
            self.assertFalse(kwargs["shell"])
            kwargs["stdout"].write(b'{"event":"start"}\n')
            return process

        fake = SimpleNamespace(
            cpu_count=lambda **kw: 4,
            virtual_memory=lambda: SimpleNamespace(total=8 * 1024**3),
            Process=lambda: None, Popen=popen,
            NoSuchProcess=ProcessLookupError, Error=OSError,
            wait_procs=lambda processes, timeout: (processes, []),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "neural_benchmark.exe").touch()
            with patch.object(runner.time, "monotonic", side_effect=[0, 241, 242]):
                report = runner.sample(self.args(root), fake)
        self.assertFalse(report["ok"])
        self.assertTrue(report["timed_out"])
        self.assertEqual(killed, [124, 123])
        self.assertEqual(report["memory_bytes"]["rss"], 100)
        self.assertEqual(report["benchmark_events"], [{"event": "start"}])
        self.assertTrue(process.assert_recursive)


if __name__ == "__main__":
    unittest.main()
