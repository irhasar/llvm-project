#!/usr/bin/env python3
"""Measure wall time, CPU time, peak RSS and artifact size for a command.

Use the same command shape for both backends: a clean compile command for
compile-time comparisons, or a benchmark executable for runtime comparisons.
The script measures one child process at a time so that ``wait4`` resource
accounting belongs to that command.

Examples:
  python3 llvm/utils/rust-migration/bench_metrics.py \
    --label cpp-compile --repeat 5 --artifact /tmp/ebcdic-cpp.o -- \
    g++ -O3 -c ... -o /tmp/ebcdic-cpp.o

  python3 llvm/utils/rust-migration/bench_metrics.py \
    --label rust-runtime --repeat 5 -- ./build/bin/llvm-rust-convertebcdic-differential
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import statistics
import subprocess
import sys
import time
from pathlib import Path


def path_size(path: Path) -> int:
    if not path.exists():
        return 0
    if path.is_file():
        return path.stat().st_size
    return sum(child.stat().st_size for child in path.rglob("*") if child.is_file())


def run_once(command: list[str], cwd: str | None) -> dict[str, float | int]:
    started = time.perf_counter()
    proc = subprocess.Popen(
        command,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    _, status, usage = os.wait4(proc.pid, 0)
    elapsed = time.perf_counter() - started
    proc.returncode = os.waitstatus_to_exitcode(status)
    if proc.returncode != 0:
        raise RuntimeError(f"command exited with {proc.returncode}: {command}")
    # Linux reports KiB; macOS reports bytes. Normalize to KiB.
    maxrss = usage.ru_maxrss
    if sys.platform == "darwin":
        maxrss //= 1024
    return {
        "wall_s": elapsed,
        "user_s": usage.ru_utime,
        "sys_s": usage.ru_stime,
        "maxrss_kib": maxrss,
    }


def median(values: list[float]) -> float:
    return statistics.median(values) if values else 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", default=None, help="row label")
    parser.add_argument("--repeat", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=0)
    parser.add_argument("--cwd", default=None)
    parser.add_argument(
        "--artifact",
        action="append",
        default=[],
        help="file or directory whose size is reported; repeatable",
    )
    parser.add_argument(
        "--prepare",
        default=None,
        help="command run before each measured run, e.g. 'cargo clean'",
    )
    parser.add_argument("--json", action="store_true", help="emit JSON instead of Markdown")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()

    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        parser.error("missing command after --")
    if args.repeat < 1:
        parser.error("--repeat must be positive")
    if args.warmup < 0:
        parser.error("--warmup must be non-negative")

    prepare = shlex.split(args.prepare) if args.prepare else None
    label = args.label or os.path.basename(command[0])
    runs: list[dict[str, float | int]] = []
    try:
        for _ in range(args.warmup):
            if prepare:
                run_once(prepare, args.cwd)
            run_once(command, args.cwd)
        for _ in range(args.repeat):
            if prepare:
                run_once(prepare, args.cwd)
            runs.append(run_once(command, args.cwd))
    except (OSError, RuntimeError) as error:
        print(f"{label}: {error}", file=sys.stderr)
        return 1

    walls = [float(run["wall_s"]) for run in runs]
    users = [float(run["user_s"]) for run in runs]
    systems = [float(run["sys_s"]) for run in runs]
    rss = [float(run["maxrss_kib"]) for run in runs]
    wall = median(walls)
    user = median(users)
    system = median(systems)
    artifact_bytes = sum(path_size(Path(path)) for path in args.artifact)
    result = {
        "label": label,
        "runs": len(runs),
        "wall_ms_median": wall * 1000.0,
        "user_ms_median": user * 1000.0,
        "sys_ms_median": system * 1000.0,
        "cpu_percent_median": (user + system) / wall * 100.0 if wall else 0.0,
        "maxrss_mib_max": max(rss) / 1024.0 if rss else 0.0,
        "artifact_bytes": artifact_bytes,
        "wall_ms_min": min(walls) * 1000.0 if walls else 0.0,
        "wall_ms_max": max(walls) * 1000.0 if walls else 0.0,
    }

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(
            f"| {label} | {result['runs']} | {result['wall_ms_median']:.2f} "
            f"({result['wall_ms_min']:.2f}-{result['wall_ms_max']:.2f}) | "
            f"{result['user_ms_median']:.2f} | {result['sys_ms_median']:.2f} | "
            f"{result['cpu_percent_median']:.1f}% | {result['maxrss_mib_max']:.2f} | "
            f"{artifact_bytes} |"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
