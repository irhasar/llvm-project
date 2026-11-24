#!/usr/bin/env python3
"""Compile rustc's LLVM wrapper against the LLVM built in this repository.

rustc_llvm compiles C++ against both the public LLVM headers and generated
headers (llvm-config.h, Intrinsics*.h). This check downloads the wrapper from
the rustc release this fork tracks and runs the compiler in syntax-only mode,
which catches missing headers and signature changes without building rustc.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path

WRAPPER_FILES = [
    "CoverageMappingWrapper.cpp",
    "LLVMWrapper.h",
    "Linker.cpp",
    "PassWrapper.cpp",
    "RustWrapper.cpp",
    "SuppressLLVMWarnings.h",
    "SymbolWrapper.cpp",
]


def run(command: list[str], *, cwd: Path | None = None) -> str:
    result = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        cwd=cwd,
        timeout=300,
    )
    output = result.stdout.decode(errors="replace")
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {shlex.join(command)}\n{output}"
        )
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build-dir", required=True, type=Path)
    parser.add_argument("--wrapper-dir", required=True, type=Path)
    parser.add_argument("--cxx", default=None)
    args = parser.parse_args()

    build_dir = args.build_dir.resolve()
    llvm_config = build_dir / "bin" / "llvm-config"
    if not llvm_config.exists():
        raise FileNotFoundError(llvm_config)

    cxx = args.cxx or os.environ.get("CXX") or "c++"
    cxxflags = shlex.split(run([str(llvm_config), "--cxxflags"]))
    # Keep only compiler flags so the list can be passed to cxx directly.
    cxxflags = [flag for flag in cxxflags if flag.startswith(("-", "/"))]

    wrapper_dir = args.wrapper_dir.resolve()
    sources = [wrapper_dir / name for name in WRAPPER_FILES if name.endswith(".cpp")]
    missing = [str(path) for path in sources if not path.exists()]
    if missing:
        raise FileNotFoundError(f"missing wrapper sources: {missing}")

    for source in sources:
        run(
            [cxx, *cxxflags, "-fsyntax-only", f"-I{wrapper_dir}", str(source)],
        )
        print(f"ok: {source.name}")

    print(f"rustc LLVM wrapper compiled against {llvm_config}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:  # noqa: BLE001 - CI needs a readable failure
        print(f"rustc wrapper check failed: {error}", file=sys.stderr)
        sys.exit(1)
