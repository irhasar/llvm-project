#!/usr/bin/env python3
"""Compare Rust bitcode tools with the C++ LLVM oracle on a synthetic corpus."""

import argparse
import os
import statistics
import subprocess
import tempfile
import time


def run(cmd):
    start = time.perf_counter()
    process = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _, status, usage = os.wait4(process.pid, 0)
    if status != 0:
        raise SystemExit(f"command failed: {' '.join(cmd)}")
    return time.perf_counter() - start, usage.ru_maxrss


def measure(name, cmd, runs, warmup):
    for _ in range(warmup):
        run(cmd)
    samples = [run(cmd) for _ in range(runs)]
    times = [sample[0] for sample in samples]
    rss = max(sample[1] for sample in samples)
    print(
        f"{name:34s} median={statistics.median(times) * 1000:7.1f}ms "
        f"min={min(times) * 1000:7.1f}ms max={max(times) * 1000:7.1f}ms "
        f"peak_rss={rss / 1024:6.1f}MiB"
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--llvm-bin", default="/usr/lib/llvm-19/bin")
    parser.add_argument("--rust-bin", default="/tmp/llvm-rust-target/release")
    parser.add_argument("--functions", type=int, default=1000)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=1)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        source = os.path.join(tmp, "corpus.ll")
        cpp_bc = os.path.join(tmp, "cpp.bc")
        rust_bc = os.path.join(tmp, "rust.bc")
        with open(source, "w", encoding="utf-8") as out:
            out.write("declare void @use(ptr align 8, i32 noundef)\n")
            out.write("@slot = global i32 0\n\n")
            for index in range(args.functions):
                out.write(f"define i32 @f{index}(i32 %a, i32 %b) {{\nentry:\n")
                out.write("  %v0 = add i32 %a, %b\n")
                for step in range(1, 10):
                    out.write(f"  %v{step} = mul i32 %v{step - 1}, {step + 1}\n")
                out.write("  call void @use(ptr align 8 @slot, i32 noundef %v9)\n")
                out.write("  ret i32 %v9\n}\n\n")

        prefix = ["taskset", "-c", "0"]
        cpp_as = [*prefix, os.path.join(args.llvm_bin, "llvm-as"), source, "-o", cpp_bc]
        rust_as = [*prefix, os.path.join(args.rust_bin, "llvm-as-rust"), source, "-o", rust_bc]
        measure("llvm-as (C++)", cpp_as, args.runs, args.warmup)
        measure("llvm-as-rust", rust_as, args.runs, args.warmup)
        # Both disassemblers consume the same C++-produced bitcode so the
        # comparison isolates decoder work from writer record layout.
        cpp_dis = [*prefix, os.path.join(args.llvm_bin, "llvm-dis"), cpp_bc, "-o", os.devnull]
        rust_dis = [*prefix, os.path.join(args.rust_bin, "llvm-dis-rust"), cpp_bc, "-o", os.devnull]
        measure("llvm-dis (C++) on C++ bitcode", cpp_dis, args.runs, args.warmup)
        measure("llvm-dis-rust on C++ bitcode", rust_dis, args.runs, args.warmup)
        print(f"input: {os.path.getsize(source)} bytes, {args.functions} functions")


if __name__ == "__main__":
    main()
