#!/usr/bin/env python3
"""Verify the compiler-rt artefacts rustc needs were actually produced."""

from __future__ import annotations

import argparse
import fnmatch
import sys
from pathlib import Path

# Runtimes used by rustc: compiler-builtins/CRT, the sanitizer runtimes, and
# the profiling/fuzzing/ORC runtimes that rustc's toolchain ships.
HOST_PATTERNS = [
    "libclang_rt.builtins-*",
    "libclang_rt.asan-*",
    "libclang_rt.asan_cxx-*",
    "libclang_rt.ubsan_standalone-*",
    "libclang_rt.ubsan_minimal-*",
    "libclang_rt.tsan-*",
    "libclang_rt.msan-*",
    "libclang_rt.dfsan-*",
    "libclang_rt.lsan-*",
    "libclang_rt.profile-*",
    "libclang_rt.fuzzer-*",
    "libclang_rt.orc-*",
    "crtbegin*.o",
    "crtend*.o",
]

BUILTINS_PATTERNS = [
    "libclang_rt.builtins-*",
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build-dir", required=True, type=Path)
    parser.add_argument(
        "--builtins-only",
        action="store_true",
        help="only require the builtins archive (cross-target builds)",
    )
    args = parser.parse_args()

    files = [path.name for path in args.build_dir.rglob("*") if path.is_file()]
    patterns = BUILTINS_PATTERNS if args.builtins_only else HOST_PATTERNS
    missing = [
        pattern
        for pattern in patterns
        if not any(fnmatch.fnmatch(name, pattern) for name in files)
    ]

    for pattern in patterns:
        matches = sorted(name for name in files if fnmatch.fnmatch(name, pattern))
        print(f"{pattern}: {matches[:3]}")

    if missing:
        print(f"missing compiler-rt artefacts: {missing}", file=sys.stderr)
        return 1
    print("compiler-rt artefacts present.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
