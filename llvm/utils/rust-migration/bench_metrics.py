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
    --label rust-runtime --repeat 5 -- \
    ./build/bin/llvm-rust-convertebcdic-differential \
    --no-checks --bench-backend=rust
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import shlex
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def path_size(path: Path) -> int:
    if not path.exists():
        return 0
    if path.is_file():
        return path.stat().st_size
    return sum(child.stat().st_size for child in path.rglob("*") if child.is_file())


def wait_status_code(status: int) -> int:
    if os.WIFEXITED(status):
        return os.WEXITSTATUS(status)
    if os.WIFSIGNALED(status):
        return -os.WTERMSIG(status)
    raise RuntimeError(f"unexpected wait status {status}")


def build_launcher(directory: Path) -> Path | None:
    """Build and probe the Linux launcher that isolates the benchmark.

    A Popen child inherits Python's resident pages before exec, and wait4 folds
    that pre-exec peak into ru_maxrss. The launcher forks a distinct child, so
    only the launcher's small footprint can inflate the reported peak. A
    ``noexec`` temporary mount lets the compile succeed but not the probe.
    """
    if sys.platform != "linux":
        return None
    source = Path(__file__).with_name("measure_exec.c")
    if not source.is_file():
        return None
    compiler = os.environ.get("CC", "cc")
    launcher = directory / "measure_exec"
    try:
        subprocess.run(
            [compiler, "-O2", "-o", str(launcher), str(source)],
            check=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        probe = subprocess.run(
            [str(launcher), "/bin/true"],
            check=False,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return launcher if probe.returncode == 0 else None


def run_once_direct(command: list[str], cwd: str | None) -> dict[str, float | int]:
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
    proc.returncode = wait_status_code(status)
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


def run_once_launcher(
    command: list[str], cwd: str | None, launcher: Path
) -> dict[str, float | int]:
    proc = subprocess.run(
        [str(launcher), *command],
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    fields = proc.stdout.decode().split()
    if len(fields) != 5:
        raise RuntimeError(
            f"launcher produced {len(fields)} fields, expected 5: {fields!r}"
        )
    returncode, wall, user, system, maxrss = (
        int(fields[0]),
        float(fields[1]),
        float(fields[2]),
        float(fields[3]),
        int(fields[4]),
    )
    if returncode != 0:
        raise RuntimeError(f"command exited with {returncode}: {command}")
    return {
        "wall_s": wall,
        "user_s": user,
        "sys_s": system,
        "maxrss_kib": maxrss,
    }


def median(values: list[float]) -> float:
    return statistics.median(values) if values else 0.0


def run_benchmark(
    args: argparse.Namespace,
    command: list[str],
    prepare: list[str] | None,
    label: str,
) -> dict[str, float | int]:
    with contextlib.ExitStack() as stack:
        launcher: Path | None = None
        if args.rss_backend != "wait4":
            if sys.platform != "linux":
                if args.rss_backend == "launcher":
                    raise RuntimeError("the launcher backend requires Linux")
                print(
                    "warning: launcher backend requires Linux; peak RSS uses "
                    "wait4 and includes Python's pre-exec footprint",
                    file=sys.stderr,
                )
            else:
                try:
                    temp_dir = stack.enter_context(
                        tempfile.TemporaryDirectory(prefix="llvm-rust-bench-")
                    )
                except OSError as error:
                    if args.rss_backend == "launcher":
                        raise RuntimeError(
                            f"cannot create launcher directory: {error}"
                        ) from error
                    print(
                        "warning: cannot create launcher directory; peak RSS "
                        "uses wait4 and includes Python's pre-exec footprint",
                        file=sys.stderr,
                    )
                else:
                    launcher = build_launcher(Path(temp_dir))
                    if launcher is None:
                        if args.rss_backend == "launcher":
                            raise RuntimeError(
                                "launcher could not be compiled or executed; "
                                "install a C compiler, use an exec-capable "
                                "temporary directory, or pass --rss-backend=wait4"
                            )
                        print(
                            "warning: launcher could not be compiled or "
                            "executed; peak RSS uses wait4 and includes Python's "
                            "pre-exec footprint",
                            file=sys.stderr,
                        )

        rss_backend = "launcher" if launcher is not None else "wait4"

        def run(step: list[str]) -> dict[str, float | int]:
            if launcher is not None:
                return run_once_launcher(step, args.cwd, launcher)
            return run_once_direct(step, args.cwd)

        runs: list[dict[str, float | int]] = []
        for _ in range(args.warmup):
            if prepare:
                run(prepare)
            run(command)
        for _ in range(args.repeat):
            if prepare:
                run(prepare)
            runs.append(run(command))

        walls = [float(run["wall_s"]) for run in runs]
        users = [float(run["user_s"]) for run in runs]
        systems = [float(run["sys_s"]) for run in runs]
        rss = [float(run["maxrss_kib"]) for run in runs]
        wall = median(walls)
        user = median(users)
        system = median(systems)
        cpu = [
            (u + s) / w * 100.0
            for w, u, s in zip(walls, users, systems)
            if w
        ]
        artifact_dir = Path(args.cwd) if args.cwd is not None else Path.cwd()
        artifact_bytes = sum(
            path_size(artifact_dir / path) for path in args.artifact
        )
        return {
            "label": label,
            "runs": len(runs),
            "rss_backend": rss_backend,
            "wall_ms_median": wall * 1000.0,
            "wall_ms_min": min(walls) * 1000.0 if walls else 0.0,
            "wall_ms_max": max(walls) * 1000.0 if walls else 0.0,
            "user_ms_median": user * 1000.0,
            "user_ms_min": min(users) * 1000.0 if users else 0.0,
            "user_ms_max": max(users) * 1000.0 if users else 0.0,
            "sys_ms_median": system * 1000.0,
            "sys_ms_min": min(systems) * 1000.0 if systems else 0.0,
            "sys_ms_max": max(systems) * 1000.0 if systems else 0.0,
            "cpu_percent_median": median(cpu),
            "maxrss_mib_median": median(rss) / 1024.0,
            "maxrss_mib_min": min(rss) / 1024.0 if rss else 0.0,
            "maxrss_mib_max": max(rss) / 1024.0 if rss else 0.0,
            "artifact_bytes": artifact_bytes,
        }


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
    parser.add_argument(
        "--rss-backend",
        choices=("auto", "launcher", "wait4"),
        default="auto",
        help="peak-RSS backend; launcher isolates Linux benchmarks from Python",
    )
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

    try:
        result = run_benchmark(args, command, prepare, label)
    except (OSError, RuntimeError) as error:
        print(f"{label}: {error}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(
            f"| {label} | {result['runs']} | {result['wall_ms_median']:.2f} "
            f"({result['wall_ms_min']:.2f}-{result['wall_ms_max']:.2f}) | "
            f"{result['user_ms_median']:.2f} "
            f"({result['user_ms_min']:.2f}-{result['user_ms_max']:.2f}) | "
            f"{result['sys_ms_median']:.2f} "
            f"({result['sys_ms_min']:.2f}-{result['sys_ms_max']:.2f}) | "
            f"{result['cpu_percent_median']:.1f}% | "
            f"{result['maxrss_mib_median']:.2f} "
            f"({result['maxrss_mib_min']:.2f}-{result['maxrss_mib_max']:.2f}) | "
            f"{result['artifact_bytes']} |"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
