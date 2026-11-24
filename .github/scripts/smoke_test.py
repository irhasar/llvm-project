#!/usr/bin/env python3
"""Exercise the LLVM, LLD and support tools rustc consumes.

The checks are deliberately end-to-end: every target backend rustc enables is
asked to produce assembly, the main object formats are asked to produce
machine code, and the tools in rustc's LLVM_TOOLS list are run on real files.
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

DEFAULT_TARGETS = [
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
]

# (LLVM target name, triple used for llc). Every backend gets an assembly test.
ASSEMBLY_TRIPLES = {
    "AArch64": "aarch64-unknown-linux-gnu",
    "AMDGPU": "amdgcn-amd-amdhsa",
    "ARM": "armv7-unknown-linux-gnueabihf",
    "BPF": "bpfel-unknown-unknown",
    "Hexagon": "hexagon-unknown-elf",
    "LoongArch": "loongarch64-unknown-linux-gnu",
    "MSP430": "msp430-unknown-unknown",
    "Mips": "mips64el-unknown-linux-gnuabi64",
    "NVPTX": "nvptx64-nvidia-cuda",
    "PowerPC": "powerpc64le-unknown-linux-gnu",
    "RISCV": "riscv64-unknown-linux-gnu",
    "Sparc": "sparcv9-unknown-linux-gnu",
    "SystemZ": "s390x-unknown-linux-gnu",
    "WebAssembly": "wasm32-unknown-unknown",
    "X86": "x86_64-unknown-linux-gnu",
    "AVR": "avr-unknown-gnu-atmega328",
    "M68k": "m68k-unknown-linux-gnu",
    "CSKY": "csky-unknown-linux-gnuabiv2",
    "Xtensa": "xtensa-unknown-elf",
}

# Backends for which we also check MC/object emission on the host.
OBJECT_TRIPLES = {
    "AArch64": "aarch64-unknown-linux-gnu",
    "ARM": "armv7-unknown-linux-gnueabihf",
    "BPF": "bpfel-unknown-unknown",
    "LoongArch": "loongarch64-unknown-linux-gnu",
    "Mips": "mips64el-unknown-linux-gnuabi64",
    "NVPTX": "nvptx64-nvidia-cuda",
    "PowerPC": "powerpc64le-unknown-linux-gnu",
    "RISCV": "riscv64-unknown-linux-gnu",
    "SystemZ": "s390x-unknown-linux-gnu",
    "WebAssembly": "wasm32-unknown-unknown",
    "X86": "x86_64-unknown-linux-gnu",
    "AMDGPU": "amdgcn-amd-amdhsa",
}

IR = """\
define i32 @add(i32 %a, i32 %b) {
entry:
  %sum = add i32 %a, %b
  %scaled = mul i32 %sum, 2
  ret i32 %scaled
}

define i32 @load_store(ptr %p) {
entry:
  %v = load i32, ptr %p, align 4
  store i32 %v, ptr %p, align 4
  ret i32 %v
}
"""

OPT_IR = """\
define i32 @sum_to_n(i32 %n) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %acc.next = add i32 %acc, %i
  %next = add i32 %i, 1
  %done = icmp eq i32 %next, %n
  br i1 %done, label %exit, label %loop

exit:
  ret i32 %acc.next
}
"""


class Harness:
    def __init__(self, build_dir: Path) -> None:
        self.build_dir = build_dir
        self.bin = build_dir / "bin"
        self.failures: list[str] = []
        self.tmp = Path(tempfile.mkdtemp(prefix="llvm-smoke-"))

    def tool(self, name: str) -> Path:
        candidates = [self.bin / name]
        if os.name == "nt":
            candidates.insert(0, self.bin / f"{name}.exe")
        for candidate in candidates:
            if candidate.exists():
                return candidate
        raise FileNotFoundError(f"missing build tool: {self.bin / name}")

    def run(
        self,
        args: list[object],
        *,
        input_data: bytes | None = None,
        expected: int = 0,
        cwd: Path | None = None,
    ) -> subprocess.CompletedProcess[bytes]:
        command = [str(arg) for arg in args]
        result = subprocess.run(
            command,
            input=input_data,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            cwd=cwd or self.tmp,
            timeout=900,
        )
        if result.returncode != expected:
            output = result.stdout.decode(errors="replace")
            raise AssertionError(
                f"command failed ({result.returncode} != {expected}): "
                f"{shlex.join(command)}\n{output[-4000:]}"
            )
        return result

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            raise AssertionError(message)


def write(path: Path, contents: str) -> Path:
    path.write_text(contents)
    return path


def smoke_llvm(h: Harness, expected_targets: set[str]) -> None:
    llvm_config = h.tool("llvm-config")

    version = h.run([llvm_config, "--version"]).stdout.decode().strip()
    print(f"llvm-config --version: {version}")
    h.check(bool(version), "llvm-config --version returned nothing")

    targets = set(
        h.run([llvm_config, "--targets-built"]).stdout.decode().split()
    )
    print(f"targets built: {' '.join(sorted(targets))}")
    h.check(
        targets == expected_targets,
        f"targets-built mismatch: missing={sorted(expected_targets - targets)} "
        f"extra={sorted(targets - expected_targets)}",
    )

    link_shared = h.run([llvm_config, "--link-shared"]).stdout.decode().strip()
    print(f"llvm-config --link-shared: {link_shared or '(no)'}")

    ir = write(h.tmp / "sample.ll", IR)
    opt_ir = write(h.tmp / "opt.ll", OPT_IR)

    llc = h.tool("llc")
    for target in sorted(expected_targets):
        triple = ASSEMBLY_TRIPLES[target]
        output = h.tmp / f"{target}.s"
        h.run([llc, "-mtriple", triple, "-filetype=asm", "-o", output, ir])
        h.check(output.stat().st_size > 0, f"{target}: empty assembly output")

    for target, triple in sorted(OBJECT_TRIPLES.items()):
        if target not in expected_targets:
            continue
        output = h.tmp / f"{target}.o"
        h.run([llc, "-mtriple", triple, "-filetype=obj", "-o", output, ir])
        h.check(output.stat().st_size > 0, f"{target}: empty object output")

    opt = h.tool("opt")
    optimized = h.run([opt, "-passes=default<O2>", "-S", opt_ir]).stdout.decode()
    h.check("define i32 @sum_to_n" in optimized, "opt dropped the test function")

    llvm_as = h.tool("llvm-as")
    llvm_dis = h.tool("llvm-dis")
    bitcode = h.tmp / "sample.bc"
    h.run([llvm_as, "-o", bitcode, ir])
    disassembled = h.run([llvm_dis, "-o", "-", bitcode]).stdout.decode()
    h.check("define i32 @add" in disassembled, "llvm-as/llvm-dis round trip failed")

    second = write(
        h.tmp / "second.ll",
        "define i32 @other(i32 %x) {\nentry:\n  ret i32 %x\n}\n",
    )
    second_bc = h.tmp / "second.bc"
    h.run([llvm_as, "-o", second_bc, second])
    linked = h.tmp / "linked.bc"
    h.run([h.tool("llvm-link"), "-o", linked, bitcode, second_bc])
    linked_ir = h.run([llvm_dis, "-o", "-", linked]).stdout.decode()
    h.check("@add" in linked_ir and "@other" in linked_ir, "llvm-link lost a module")

    sample_object = h.tmp / "X86.o"
    archive = h.tmp / "libsample.a"
    llvm_ar = h.tool("llvm-ar")
    h.run([llvm_ar, "rcs", archive, sample_object])
    archive_members = h.run([llvm_ar, "t", archive]).stdout.decode()
    h.check("X86.o" in archive_members, "llvm-ar did not archive the object")
    h.run([llvm_ar, "x", archive, "X86.o"])

    copied = h.tmp / "copied.o"
    h.run([h.tool("llvm-objcopy"), sample_object, copied])
    h.check(copied.stat().st_size > 0, "llvm-objcopy produced an empty file")
    stripped = h.tmp / "stripped.o"
    h.run([h.tool("llvm-strip"), "-o", stripped, copied])
    h.check(stripped.stat().st_size > 0, "llvm-strip produced an empty file")

    nm_output = h.run([h.tool("llvm-nm"), sample_object]).stdout.decode()
    h.check("add" in nm_output, "llvm-nm did not list the test symbol")
    size_output = h.run([h.tool("llvm-size"), sample_object]).stdout.decode()
    h.check("text" in size_output, "llvm-size did not report sections")
    objdump_output = h.run([h.tool("llvm-objdump"), "-d", sample_object]).stdout.decode()
    h.check("add" in objdump_output or "ret" in objdump_output, "llvm-objdump output looks wrong")
    h.run([h.tool("llvm-readobj"), "--file-headers", sample_object])
    h.run([h.tool("llvm-cov"), "--version"])
    h.run([h.tool("llvm-profdata"), "--version"])

    check_file = write(h.tmp / "sample.check", "CHECK: define i32 @add\n")
    h.run([h.tool("FileCheck"), check_file, "--input-file", ir])
    h.run([h.tool("not"), sys.executable, "-c", "import sys; sys.exit(1)"])
    h.run(
        [h.tool("not"), sys.executable, "-c", "import sys; sys.exit(0)"],
        expected=1,
    )


def smoke_lld(h: Harness, test_link: bool) -> None:
    for flavor in ("ld.lld", "ld64.lld", "lld-link", "wasm-ld"):
        tool = h.tool(flavor)
        result = h.run([tool, "--version"]).stdout.decode()
        h.check("LLD" in result or "lld" in result, f"{flavor} did not report its version")

    if not test_link:
        return

    cc = os.environ.get("CC", "cc")
    hello = write(h.tmp / "hello.c", "int main(void) { return 0; }\n")
    executable = h.tmp / "hello"
    env = os.environ.copy()
    env["PATH"] = f"{h.bin}{os.pathsep}{env['PATH']}"
    result = subprocess.run(
        [cc, f"-B{h.bin}", "-fuse-ld=lld", "-o", str(executable), str(hello)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
        cwd=h.tmp,
        timeout=300,
    )
    h.check(
        result.returncode == 0,
        f"clang + ld.lld link failed:\n{result.stdout.decode(errors='replace')[-4000:]}",
    )
    h.run([executable])

    object_file = h.tmp / "hello.o"
    h.run([cc, "-c", "-o", object_file, hello])
    relocatable = h.tmp / "hello.r.o"
    h.run([h.tool("ld.lld"), "-r", "-o", relocatable, object_file])
    h.run([h.tool("llvm-readobj"), "--file-headers", relocatable])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build-dir", required=True, type=Path)
    parser.add_argument(
        "--expected-targets",
        default=";".join(DEFAULT_TARGETS),
        help="semicolon or comma separated list of targets that must be built",
    )
    parser.add_argument(
        "--test-link",
        action="store_true",
        help="run a real clang + ld.lld link and execute the result",
    )
    args = parser.parse_args()

    expected_targets = {
        item.strip()
        for item in args.expected_targets.replace(",", ";").split(";")
        if item.strip()
    }

    harness = Harness(args.build_dir.resolve())
    smoke_llvm(harness, expected_targets)
    smoke_lld(harness, args.test_link)
    print("LLVM/LLD smoke tests passed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:  # noqa: BLE001 - CI needs a readable failure
        print(f"smoke test failed: {error}", file=sys.stderr)
        sys.exit(1)
