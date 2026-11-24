#!/usr/bin/env python3
"""Audit that this repository contains exactly the LLVM pieces rustc uses.

The checks below encode the component set that rustc 1.99.0 drives through
src/bootstrap (llvm/, lld/, compiler-rt/, libunwind/, libc/ and
third-party/siphash), plus the target list and LLVM tools rustc asks for.
Anything from the wider LLVM monorepo is reported as unexpected.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# rustc 1.99.0, src/bootstrap/src/core/build_steps/llvm.rs.
EXPECTED_TARGETS = {
    "AArch64",
    "AMDGPU",
    "ARM",
    "BPF",
    "Hexagon",
    "LoongArch",
    "MSP430",
    "Mips",
    "NVPTX",
    "PowerPC",
    "RISCV",
    "Sparc",
    "SystemZ",
    "WebAssembly",
    "X86",
    "AVR",
    "M68k",
    "CSKY",
    "Xtensa",
}

# rustc 1.99.0, src/bootstrap/src/core/build_steps/llvm.rs (LLVM_TOOLS) plus
# llvm-config and lto, which the in-tree LLVM build always produces.
EXPECTED_TOOLS = {
    "llc",
    "lto",
    "llvm-ar",
    "llvm-as",
    "llvm-config",
    "llvm-cov",
    "llvm-dis",
    "llvm-link",
    "llvm-nm",
    "llvm-objcopy",
    "llvm-objdump",
    "llvm-profdata",
    "llvm-readobj",
    "llvm-shlib",
    "llvm-size",
    "llubi",
    "opt",
}

EXPECTED_TOP_LEVEL = {
    ".github",
    ".gitattributes",
    ".gitignore",
    "LICENSE.TXT",
    "README.md",
    "cmake",
    "compiler-rt",
    "libc",
    "libunwind",
    "lld",
    "llvm",
    "third-party",
}

EXPECTED_LLVM_MISSING_DIRS = {
    "benchmarks",
    "bindings",
    "docs",
    "examples",
    "test",
    "unittests",
}

# Utilities that pull in tools rustc does not build.
EXPECTED_LLVM_MISSING_UTILS = {
    "llvm-locstats",
}

EXPECTED_LLD_SUBDIRS = {
    "COFF",
    "Common",
    "ELF",
    "MachO",
    "MinGW",
    "cmake",
    "include",
    "tools",
    "wasm",
}

EXPECTED_COMPILER_RT_SUBDIRS = {"cmake", "include", "lib", "tools"}
EXPECTED_LIBUNWIND_SUBDIRS = {"cmake", "include", "src"}
EXPECTED_LIBC_SUBDIRS = {"hdr", "include", "shared", "src"}

REQUIRED_FILES = [
    "cmake/Modules/FindLibcCommonUtils.cmake",
    "compiler-rt/CMakeLists.txt",
    "compiler-rt/lib/builtins/CMakeLists.txt",
    "compiler-rt/lib/builtins/crtbegin.c",
    "compiler-rt/lib/builtins/crtend.c",
    "libc/shared/math.h",
    "libc/shared/math_check_exceptions.h",
    "libunwind/CMakeLists.txt",
    "libunwind/src/UnwindLevel1.c",
    "libunwind/src/libunwind.cpp",
    "lld/CMakeLists.txt",
    "llvm/CMakeLists.txt",
    "llvm/include/llvm/Analysis/Lint.h",
    "llvm/include/llvm/LTO/LTO.h",
    "llvm/include/llvm/Passes/PassBuilder.h",
    "llvm/include/llvm/Plugins/PassPlugin.h",
    "llvm/include/llvm/Support/FileSystem.h",
    "llvm/include/llvm/DTLTO/DTLTO.h",
    "llvm/include/llvm/Target/TargetMachine.h",
    "llvm/include/llvm/WindowsDriver/MSVCPaths.h",
    "llvm/lib/DTLTO/CMakeLists.txt",
    "llvm/lib/DTLTO/DTLTO.cpp",
    "llvm/lib/WindowsDriver/CMakeLists.txt",
    "llvm/lib/WindowsDriver/MSVCPaths.cpp",
    "llvm/utils/FileCheck/CMakeLists.txt",
    "llvm/utils/TableGen/CMakeLists.txt",
    "llvm/utils/not/CMakeLists.txt",
    "third-party/siphash/include/siphash/SipHash.h",
]

REQUIRED_WORKFLOWS = [
    ".github/workflows/rust-premerge.yaml",
    ".github/workflows/rustc-integration.yml",
]

failures: list[str] = []


def fail(message: str) -> None:
    failures.append(message)


def tracked_files() -> set[str]:
    result = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-files", "-z"],
        check=True,
        stdout=subprocess.PIPE,
    )
    return {entry for entry in result.stdout.decode().split("\0") if entry}


def subdirs(path: Path) -> set[str]:
    return {entry.name for entry in path.iterdir() if entry.is_dir()}


def cmake_target_set(text: str, variable: str) -> set[str]:
    match = re.search(
        rf"set\(\s*{re.escape(variable)}\s*\n(.*?)\n\s*\)", text, re.DOTALL
    )
    if match is None:
        fail(f"could not find {variable} in llvm/CMakeLists.txt")
        return set()
    return {
        line.strip()
        for line in match.group(1).splitlines()
        if re.fullmatch(r"\s*[A-Za-z0-9]+\s*", line)
    }


def check_tracked_top_level(files: set[str]) -> None:
    top = {path.split("/", 1)[0] for path in files}
    for extra in sorted(top - EXPECTED_TOP_LEVEL):
        fail(f"unexpected top-level path: {extra}")
    for missing in sorted(EXPECTED_TOP_LEVEL - top):
        if missing.startswith("."):
            continue
        fail(f"missing top-level path: {missing}")


def check_required_files() -> None:
    for relative in REQUIRED_FILES:
        if not (REPO_ROOT / relative).is_file():
            fail(f"missing rustc-required file: {relative}")
    for relative in REQUIRED_WORKFLOWS:
        if not (REPO_ROOT / relative).is_file():
            fail(f"missing CI workflow: {relative}")


def check_llvm_layout(files: set[str]) -> None:
    for removed in EXPECTED_LLVM_MISSING_DIRS:
        if any(path.startswith(f"llvm/{removed}/") for path in files):
            fail(f"llvm/{removed} must not be present (not needed by rustc)")

    for required in ("llvm/projects/CMakeLists.txt", "llvm/runtimes/CMakeLists.txt"):
        if required not in files:
            fail(f"missing LLVM build glue: {required}")

    for removed in EXPECTED_LLVM_MISSING_UTILS:
        if any(path.startswith(f"llvm/utils/{removed}/") for path in files):
            fail(f"llvm/utils/{removed} must not be present (pulls in unused tools)")

    target_dir = REPO_ROOT / "llvm/lib/Target"
    if not target_dir.is_dir():
        fail("missing llvm/lib/Target")
    else:
        actual_targets = subdirs(target_dir)
        for extra in sorted(actual_targets - EXPECTED_TARGETS):
            fail(f"LLVM backend not used by rustc: lib/Target/{extra}")
        for missing in sorted(EXPECTED_TARGETS - actual_targets):
            fail(f"LLVM backend needed by rustc is missing: lib/Target/{missing}")

    cmake_text = (REPO_ROOT / "llvm/CMakeLists.txt").read_text()
    declared = cmake_target_set(cmake_text, "LLVM_ALL_TARGETS") | cmake_target_set(
        cmake_text, "LLVM_ALL_EXPERIMENTAL_TARGETS"
    )
    for extra in sorted(declared - EXPECTED_TARGETS):
        fail(f"LLVM_ALL_TARGETS declares a backend rustc does not use: {extra}")
    for missing in sorted(EXPECTED_TARGETS - declared):
        fail(f"LLVM_ALL_TARGETS is missing a backend rustc uses: {missing}")

    tools_dir = REPO_ROOT / "llvm/tools"
    if not tools_dir.is_dir():
        fail("missing llvm/tools")
    else:
        actual_tools = subdirs(tools_dir)
        for extra in sorted(actual_tools - EXPECTED_TOOLS):
            fail(f"LLVM tool not used by rustc: tools/{extra}")
        for missing in sorted(EXPECTED_TOOLS - actual_tools):
            fail(f"LLVM tool needed by rustc is missing: tools/{missing}")


def check_subproject_layout() -> None:
    lld = REPO_ROOT / "lld"
    if not lld.is_dir():
        fail("missing lld/")
    else:
        actual = subdirs(lld)
        for extra in sorted(actual - EXPECTED_LLD_SUBDIRS):
            fail(f"lld subdirectory not needed by rustc: {extra}")
        for missing in sorted(EXPECTED_LLD_SUBDIRS - actual):
            fail(f"lld subdirectory missing: {missing}")

    compiler_rt = REPO_ROOT / "compiler-rt"
    if not compiler_rt.is_dir():
        fail("missing compiler-rt/")
    else:
        actual = subdirs(compiler_rt)
        for extra in sorted(actual - EXPECTED_COMPILER_RT_SUBDIRS):
            fail(f"compiler-rt subdirectory not needed by rustc: {extra}")
        for missing in sorted(EXPECTED_COMPILER_RT_SUBDIRS - actual):
            fail(f"compiler-rt subdirectory missing: {missing}")

    libunwind = REPO_ROOT / "libunwind"
    if not libunwind.is_dir():
        fail("missing libunwind/")
    else:
        actual = subdirs(libunwind)
        for extra in sorted(actual - EXPECTED_LIBUNWIND_SUBDIRS):
            fail(f"libunwind subdirectory not needed by rustc: {extra}")
        for missing in sorted(EXPECTED_LIBUNWIND_SUBDIRS - actual):
            fail(f"libunwind subdirectory missing: {missing}")

    libc = REPO_ROOT / "libc"
    if not libc.is_dir():
        fail("missing libc/ headers used by APFloat")
    else:
        actual = subdirs(libc)
        for extra in sorted(actual - EXPECTED_LIBC_SUBDIRS):
            fail(f"libc subdirectory not needed by APFloat: {extra}")
        for missing in sorted(EXPECTED_LIBC_SUBDIRS - actual):
            fail(f"libc subdirectory missing: {missing}")

    third_party = REPO_ROOT / "third-party"
    if not third_party.is_dir():
        fail("missing third-party/")
    else:
        actual = subdirs(third_party)
        expected = {"siphash"}
        for extra in sorted(actual - expected):
            fail(f"third-party subdirectory not used by LLVM: {extra}")
        for missing in sorted(expected - actual):
            fail(f"third-party subdirectory missing: {missing}")


def main() -> int:
    files = tracked_files()
    check_tracked_top_level(files)
    check_required_files()
    check_llvm_layout(files)
    check_subproject_layout()

    if failures:
        print("Pruning audit failed:\n", file=sys.stderr)
        for message in failures:
            print(f"  - {message}", file=sys.stderr)
        return 1

    print(
        "Pruning audit passed: llvm, lld, compiler-rt, libunwind, libc and "
        "third-party/siphash match the rustc 1.99.0 requirement set."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
