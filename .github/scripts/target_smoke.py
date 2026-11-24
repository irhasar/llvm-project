#!/usr/bin/env python3
"""Exercise one LLVM backend without building every target."""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

# target: (llc triple, whether object emission is expected to work)
TARGETS = {
    "AArch64": ("aarch64-unknown-linux-gnu", True),
    "AMDGPU": ("amdgcn-amd-amdhsa", True),
    "ARM": ("armv7-unknown-linux-gnueabihf", True),
    "AVR": ("avr-unknown-gnu-atmega328", True),
    "BPF": ("bpfel-unknown-unknown", True),
    "CSKY": ("csky-unknown-linux-gnuabiv2", True),
    "Hexagon": ("hexagon-unknown-elf", True),
    "LoongArch": ("loongarch64-unknown-linux-gnu", True),
    "M68k": ("m68k-unknown-linux-gnu", True),
    "MSP430": ("msp430-unknown-unknown", True),
    "Mips": ("mips64el-unknown-linux-gnuabi64", True),
    "NVPTX": ("nvptx64-nvidia-cuda", False),
    "PowerPC": ("powerpc64le-unknown-linux-gnu", True),
    "RISCV": ("riscv64-unknown-linux-gnu", True),
    "Sparc": ("sparcv9-unknown-linux-gnu", True),
    "SystemZ": ("s390x-unknown-linux-gnu", True),
    "WebAssembly": ("wasm32-unknown-unknown", True),
    "X86": ("x86_64-unknown-linux-gnu", True),
    "Xtensa": ("xtensa-unknown-elf", True),
}

IR = """\
define i32 @add(i32 %a, i32 %b) {
entry:
  %sum = add i32 %a, %b
  %scaled = mul i32 %sum, 2
  ret i32 %scaled
}
"""


class Harness:
    def __init__(self, build_dir: Path) -> None:
        self.bin = build_dir / "bin"
        self.tmp = Path(tempfile.mkdtemp(prefix="llvm-target-smoke-"))

    def tool(self, name: str) -> Path:
        for candidate in (self.bin / name, self.bin / f"{name}.exe"):
            if candidate.exists():
                return candidate
        raise FileNotFoundError(f"missing build tool: {self.bin / name}")

    def run(self, args: list[object], *, expected: int = 0) -> subprocess.CompletedProcess[bytes]:
        command = [str(arg) for arg in args]
        result = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            cwd=self.tmp,
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--build-dir", required=True, type=Path)
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    args = parser.parse_args()

    triple, object_supported = TARGETS[args.target]
    harness = Harness(args.build_dir.resolve())
    llc = harness.tool("llc")
    llvm_config = harness.tool("llvm-config")

    built = set(harness.run([llvm_config, "--targets-built"]).stdout.decode().split())
    print(f"targets built: {' '.join(sorted(built))}")
    harness.check(args.target in built, f"{args.target} is not in llvm-config --targets-built")

    ir = harness.tmp / "sample.ll"
    ir.write_text(IR)

    asm = harness.tmp / f"{args.target}.s"
    harness.run([llc, "-mtriple", triple, "-filetype=asm", "-o", asm, ir])
    harness.check(asm.stat().st_size > 0, f"{args.target}: empty assembly output")
    harness.check("add" in asm.read_text(errors="replace"), f"{args.target}: assembly lost @add")

    if object_supported:
        obj = harness.tmp / f"{args.target}.o"
        harness.run([llc, "-mtriple", triple, "-filetype=obj", "-o", obj, ir])
        harness.check(obj.stat().st_size > 0, f"{args.target}: empty object output")
        harness.run([harness.tool("llvm-readobj"), "--file-headers", obj])
        harness.run([harness.tool("llvm-objdump"), "-d", obj])

    print(f"{args.target} target smoke test passed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:  # noqa: BLE001 - CI needs a readable failure
        print(f"target smoke test failed: {error}", file=sys.stderr)
        sys.exit(1)
