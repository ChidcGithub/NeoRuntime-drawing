"""Single-process benchmark sampler; never launches a GUI or discovers models.

Requires psutil only for execution, not --help. Memory is bytes, not MiB.
Run each configuration in a fresh process; polling can miss transient peaks.
Windows peak_wset/peak_pagefile are OS high-water marks observed while alive.
No process-memory limit, target-hardware simulation or whole-system RAM claim.
"""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time


def positive_seconds(value):
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise argparse.ArgumentTypeError("must be finite and > 0")
    return value


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--exe", required=True, type=Path,
                        help="explicit path to neural_benchmark.exe only")
    result.add_argument("--model-dir", required=True, type=Path)
    result.add_argument("--profile", required=True, choices=("baseline", "low-memory"))
    result.add_argument("--rounds", type=int, choices=range(1, 1001), default=3,
                        metavar="1..1000")
    result.add_argument("--ink-repeat", type=int, choices=range(1, 9), default=1)
    result.add_argument("--timeout", type=positive_seconds, default=240.0,
                        help="wall-clock seconds including startup; default 240")
    result.add_argument("--poll-ms", type=positive_seconds, default=10.0)
    result.add_argument("--cpu", type=int, choices=(2,),
                        help="Windows only: inherit affinity to two allowed logical CPUs; NOT i5 emulation")
    result.add_argument("--intra-threads", choices=("auto", "1", "2", "3", "4"))
    for name in ("prepacking", "memory-pattern", "cpu-arena", "spinning"):
        result.add_argument("--" + name, choices=("true", "false"))
    result.add_argument("--optimization-level", choices=("0", "1", "2", "3"))
    result.add_argument("--max-tokens", type=int, choices=range(1, 257), metavar="1..256")
    result.add_argument("--time-budget-secs", type=int, choices=range(1, 61), metavar="1..60")
    return result


def command_for(args):
    exe = args.exe.resolve(strict=True)
    model_dir = args.model_dir.resolve(strict=True)
    if exe.name.lower() != "neural_benchmark.exe" or not exe.is_file():
        raise ValueError("only an explicit neural_benchmark.exe file is allowed")
    if not model_dir.is_dir():
        raise ValueError("--model-dir must be an existing directory")
    command = [str(exe), "--model-dir", str(model_dir), "--profile", args.profile,
               "--rounds", str(args.rounds), "--ink-repeat", str(args.ink_repeat)]
    for name in ("intra_threads", "prepacking", "memory_pattern", "cpu_arena", "spinning",
                 "optimization_level", "max_tokens", "time_budget_secs"):
        value = getattr(args, name)
        if value is not None:
            command.extend(["--" + name.replace("_", "-"), str(value)])
    return command


def update_peaks(peaks, memory):
    for field in ("rss", "private", "peak_wset", "peak_pagefile"):
        value = getattr(memory, field, None)
        if value is not None:
            peaks[field] = max(peaks[field] or 0, value)


def kill_owned_tree(process, psutil):
    """Only the Popen identity and its descendants; no global name/PID search.

    psutil Process.kill checks PID reuse. The benchmark does not spawn children;
    descendant enumeration is best effort, not a general sandbox/job object.
    """
    failures = []
    try:
        children = process.children(recursive=True)
    except psutil.NoSuchProcess:
        children = []
    except psutil.Error as error:
        children = []
        failures.append(str(error))
    for owned in [*reversed(children), process]:
        try:
            owned.kill()
        except psutil.NoSuchProcess:
            pass
        except psutil.Error as error:
            failures.append(str(error))
    _, alive = psutil.wait_procs([*children, process], timeout=5)
    if alive:
        failures.append("owned processes still alive: " + str([p.pid for p in alive]))
    return failures


def sample(args, psutil):
    command = command_for(args)
    if args.cpu and os.name != "nt":
        raise ValueError("--cpu is supported only on Windows")
    report = {
        "command": command,
        "scope": "spawned benchmark single process only; excludes runner/children/OS/GUI",
        "target_hardware_validated": False,
        "hardware_note": "local host, not target i5/3GB validation; affinity is NOT CPU emulation",
        "hardware": {"platform": platform.platform(), "processor": platform.processor(),
                     "logical_cpus": psutil.cpu_count(), "physical_cpus": psutil.cpu_count(logical=False),
                     "host_ram_bytes": psutil.virtual_memory().total},
        "timeout_seconds": args.timeout, "poll_interval_ms": args.poll_ms,
        "affinity_requested_logical_cpus": args.cpu, "affinity_applied": None,
        "memory_bytes": {key: None for key in ("rss", "private", "peak_wset", "peak_pagefile")},
        "memory_note": "max observed; null means unavailable; polling may miss transients/exit peaks; Windows peak fields are OS high-water marks",
        "samples": 0, "timed_out": False, "cleanup_errors": [],
    }
    process = None
    started = time.monotonic()
    # File-backed streams avoid pipe deadlock without allocating all child output
    # while sampling. JSON is assembled only after the measured process exits.
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            parent = psutil.Process()
            original_affinity = None
            try:
                if args.cpu:
                    original_affinity = parent.cpu_affinity()
                    if len(original_affinity) < 2:
                        raise ValueError("fewer than two allowed logical CPUs")
                    selected = original_affinity[:2]
                    # Windows child inherits affinity at creation, before model load.
                    parent.cpu_affinity(selected)
                    report["affinity_applied"] = selected
                process = psutil.Popen(command, stdin=subprocess.DEVNULL,
                                       stdout=stdout, stderr=stderr, shell=False)
            finally:
                if original_affinity is not None:
                    parent.cpu_affinity(original_affinity)
            report["pid"] = process.pid
            while True:
                try:
                    update_peaks(report["memory_bytes"], process.memory_info())
                    report["samples"] += 1
                except psutil.NoSuchProcess:
                    pass
                if process.poll() is not None:
                    break
                remaining = args.timeout - (time.monotonic() - started)
                if remaining <= 0:
                    report["timed_out"] = True
                    report["cleanup_errors"] = kill_owned_tree(process, psutil)
                    break
                time.sleep(min(args.poll_ms / 1000.0, remaining))
        except (Exception, KeyboardInterrupt) as error:
            report["error"] = str(error) or type(error).__name__
            if process is not None:
                report["cleanup_errors"] = kill_owned_tree(process, psutil)
        report["elapsed_seconds"] = time.monotonic() - started
        report["returncode"] = process.poll() if process is not None else None
        stdout.seek(0)
        stderr.seek(0)
        events, non_json = [], []
        for line in stdout.read().decode("utf-8", errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except ValueError:
                non_json.append(line)
        report["benchmark_events"] = events
        report["stdout_non_json"] = non_json
        report["stderr"] = stderr.read().decode("utf-8", errors="replace")
    report["ok"] = (report["returncode"] == 0 and not report["timed_out"]
                    and "error" not in report and not report["cleanup_errors"])
    return report


def main():
    args = parser().parse_args()
    try:
        # --help and importing this module work even with no site packages.
        import psutil
        report = sample(args, psutil)
    except Exception as error:
        report = {"ok": False, "error": str(error)}
    print(json.dumps(report, ensure_ascii=True, allow_nan=False))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
