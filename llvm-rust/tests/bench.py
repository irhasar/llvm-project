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
    parser.add_argument(
        "--eh",
        action="store_true",
        help="generate invoke/landingpad/resume functions instead of the mixed corpus",
    )
    parser.add_argument(
        "--wineh",
        action="store_true",
        help="generate catchswitch/catchpad/cleanuppad functions instead of the mixed corpus",
    )
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        source = os.path.join(tmp, "corpus.ll")
        cpp_bc = os.path.join(tmp, "cpp.bc")
        rust_bc = os.path.join(tmp, "rust.bc")
        with open(source, "w", encoding="utf-8") as out:
            out.write("declare void @use(ptr align 8, i32 noundef)\n")
            out.write("@slot = global i32 0\n")
            out.write("@dispatch = global ptr blockaddress(@f0, %done)\n")
            out.write("%pair = type { i32, [2 x i8] }\n\n")
            for index in range(args.functions):
                out.write(
                    f"define i32 @f{index}(i32 %a, i32 %b, %pair %p, <4 x i32> %vec, ptr %ap) {{\nentry:\n"
                )
                out.write("  %v0 = add i32 %a, %b\n")
                for step in range(1, 10):
                    out.write(f"  %v{step} = mul i32 %v{step - 1}, {step + 1}\n")
                out.write("  %e = extractvalue %pair %p, 1, 0\n")
                out.write("  %x = sext i8 %e to i32\n")
                out.write("  %q = insertvalue %pair %p, i8 %e, 1, 1\n")
                out.write("  %el = extractelement <4 x i32> %vec, i32 0\n")
                out.write("  %n = insertelement <4 x i32> %vec, i32 %el, i32 0\n")
                out.write("  %r = extractelement <4 x i32> %n, i32 0\n")
                out.write("  %vsum = add <4 x i32> %vec, %n\n")
                out.write(
                    "  %vsh = shufflevector <4 x i32> %vec, <4 x i32> %n, "
                    "<4 x i32> <i32 0, i32 5, i32 2, i32 7>\n"
                )
                out.write("  %vcmp = icmp slt <4 x i32> %vsh, %vsum\n")
                out.write(
                    "  %vsel = select <4 x i1> %vcmp, <4 x i32> %vsh, <4 x i32> %vsum\n"
                )
                out.write("  %vr = extractelement <4 x i32> %vsel, i32 1\n")
                out.write("  %sum = add i32 %v9, %x\n")
                out.write("  %sum2 = add i32 %sum, %r\n")
                out.write("  %sum3 = add i32 %sum2, %vr\n")
                out.write("  %av = load atomic i32, ptr @slot seq_cst, align 4\n")
                out.write("  %sum4 = add i32 %sum3, %av\n")
                out.write("  %va = va_arg ptr %ap, i32\n")
                out.write("  %sum5 = add i32 %sum4, %va\n")
                out.write("  store atomic i32 %sum5, ptr @slot release, align 4\n")
                out.write("  call void @use(ptr align 8 @slot, i32 noundef %sum5)\n")
                out.write("  br label %dispatch\n\n")
                out.write("dispatch:\n")
                out.write(
                    f"  %target = phi ptr [ blockaddress(@f{index}, %done), %entry ]\n"
                )
                out.write("  indirectbr ptr %target, [label %done]\n\n")
                out.write("done:\n")
                out.write("  ret i32 %sum5\n}\n\n")

        if args.eh:
            with open(source, "w", encoding="utf-8") as out:
                out.write("declare i32 @may_throw()\n")
                out.write("declare i32 @__gxx_personality_v0(...)\n\n")
                for index in range(args.functions):
                    out.write(
                        f"define i32 @f{index}() personality ptr @__gxx_personality_v0 {{\n"
                    )
                    out.write("entry:\n")
                    out.write(
                        "  %r = invoke i32 @may_throw() to label %normal unwind label %lpad\n"
                    )
                    out.write("normal:\n")
                    out.write("  ret i32 %r\n")
                    out.write("lpad:\n")
                    out.write(
                        "  %lp = landingpad { ptr, i32 } cleanup catch ptr null "
                        "filter [0 x ptr] zeroinitializer\n"
                    )
                    out.write("  resume { ptr, i32 } %lp\n}\n\n")

        if args.wineh:
            with open(source, "w", encoding="utf-8") as out:
                out.write("declare i32 @may_throw()\n")
                out.write("declare i32 @__CxxFrameHandler3(...)\n")
                out.write("declare void @dtor(ptr)\n\n")
                for index in range(args.functions):
                    out.write(
                        f"define i32 @f{index}() personality ptr @__CxxFrameHandler3 {{\n"
                    )
                    out.write("entry:\n")
                    out.write(
                        "  %r = invoke i32 @may_throw() to label %cont unwind label %dispatch\n"
                    )
                    out.write("dispatch:\n")
                    out.write(
                        "  %cs = catchswitch within none [label %catch1, label %catch2] "
                        "unwind label %cleanup\n"
                    )
                    out.write("catch1:\n")
                    out.write(
                        "  %cp1 = catchpad within %cs [ptr null, i32 64, ptr null]\n"
                    )
                    out.write("  catchret from %cp1 to label %cont\n")
                    out.write("catch2:\n")
                    out.write(
                        "  %cp2 = catchpad within %cs [ptr null, i32 64, ptr null]\n"
                    )
                    out.write("  catchret from %cp2 to label %cont\n")
                    out.write("cleanup:\n")
                    out.write("  %lp = cleanuppad within none [ptr @dtor]\n")
                    out.write("  cleanupret from %lp unwind to caller\n")
                    out.write("cont:\n")
                    out.write("  ret i32 0\n}\n\n")

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
