# AGENTS.md

Repository-specific notes for agents working on this fork of `llvm-project`.
The migration itself is described in `MIGRATION_TO_RUST.md`; read that first.

## What this fork is

- Shallow clone on branch `rust-migration-phase1` (`git rev-parse
  --is-shallow-repository` returns `true`). History before the tip is absent.
- The Rust migration of `lib/Support` components lives under
  `llvm/lib/Support/rust/<component>/`.
- `llvm/test` and `llvm/docs` are **not** present. LLVM regression tests
  (`ninja check-llvm`, `llvm-lit`) cannot be run. Do not claim they passed.
  Each Rust port therefore ships its own differential and golden tests.

## Toolchain in this container

- `rustc`/`cargo` 1.100.0 (stable), edition 2024 crates (`rust-version = "1.85"`).
- `cmake` 4.4.5 and `ninja` 1.13 are installed in `~/.local/bin`.
- GCC 14.2 at `/usr/bin/g++`. Host CPU: AMD EPYC 9B14 (Zen 4) with SHA-NI.

Export the paths before building:

```sh
export PATH=$HOME/.local/bin:$HOME/.cargo/bin:$PATH
```

## Build

`llvm/test` and `llvm/docs` are missing, so `-DLLVM_INCLUDE_TESTS=OFF` and
`-DLLVM_INCLUDE_DOCS=OFF` are required:

```sh
cmake -S llvm -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DLLVM_ENABLE_RUST_SIPHASH=ON -DLLVM_ENABLE_RUST_CRC=ON \
  -DLLVM_ENABLE_RUST_MD5=ON -DLLVM_ENABLE_RUST_SHA1=ON \
  -DLLVM_ENABLE_RUST_SHA256=ON -DLLVM_ENABLE_RUST_BASE64=ON \
  -DLLVM_TARGETS_TO_BUILD=X86 \
  -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_TERMINFO=OFF \
  -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_DOCS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
  -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_ENABLE_BINDINGS=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF

ninja -C build LLVMSupport llvm-sha1-stable llvm-sha256-stable llc
```

Full LLVM builds are large; prefer building `LLVMSupport` and the specific
tool you need. Long builds must be started in the background and polled.

## Rust crate conventions

Every crate under `llvm/lib/Support/rust/` follows the same rules:

- `#![cfg_attr(not(test), no_std)]`, `panic = "abort"`, aborting
  `#[panic_handler]`, no dependencies, no crates.io at build time.
- Only `extern "C"` functions cross the boundary. Data is `(pointer, length)`
  or fixed-size buffers; the C++ caller owns all memory.
- Symbols are prefixed `llvm_<component>_<function>`.
- The C++ class layout and public headers do not change. Only the compression
  function/body is replaced; buffering and padding stay in C++.
- Each component has its own `LLVM_ENABLE_RUST_<COMPONENT>` switch, default
  `OFF`, and `Cargo.lock` is committed.

Checks for a crate:

```sh
cd llvm/lib/Support/rust/<component>
cargo test
cargo build --release --locked
```

`cargo test` runs FIPS/RFC vectors, randomized length sweeps against a
loop-based oracle, and on x86-64 a SHA-NI-vs-scalar comparison.

Differential and golden tests are built by CMake behind the option:

```sh
ninja -C build llvm-rust-sha256-differential llvm-sha256-stable
./build/bin/llvm-sha256-stable          # golden values, exercises update()
./build/bin/llvm-rust-sha256-differential   # C++ vs Rust + throughput table
```

To prove a backend-neutral result, configure the option `OFF`, rebuild
`LLVMSupport` and the `*-stable` test, and confirm identical output.

## Benchmarks

- Compare at the same optimization level: `g++ -O3` against
  `-C opt-level=3`. Do not compare `-O2` C++ with `-O3` Rust.
- The crates link as a plain static archive, so the differential test can be
  built without CMake:
  `g++ -O3 test/differential.cpp -Ltarget/release -lllvm_sha256 -o /tmp/diff`
  (add `-march=native` only when comparing native codegen on both sides).
  Base64 follows the same shape:
  `cd llvm/lib/Support/rust/base64 && cargo build --release --locked && g++ -O3 -std=c++17 test/differential.cpp -Ltarget/release -lllvm_base64 -o /tmp/base64-diff && /tmp/base64-diff`.
- `-C target-cpu=native` is exposed through `LLVM_RUST_FLAGS`; it is not a
  safe default for redistributed binaries.

## x86 backend experiments

When changing `llvm/lib/Target/X86`, keep the pre-change `llc` so the effect
can be measured against the same input:

```sh
cp build/bin/llc /tmp/llc-base
# ... edit ...
ninja -C build llc
rustc --emit=llvm-ir -O ...        # IR for a crate
/tmp/llc-base -O3 -mcpu=x86-64 in.ll -o base.s
build/bin/llc -O3 -mcpu=x86-64 in.ll -o new.s
```

There is no CodeGen test suite in this fork, so any backend change must be
justified with instruction counts and, ideally, wall-clock measurements.

## CI

GitHub Actions is disabled for this repository (`Actions has been disabled for
this repository`), so the Rust support workflow cannot be relied on. Reproduce
its checks locally instead.

```sh
ninja -C build llvm-rust-sha1-differential llvm-sha1-stable
./build/bin/llvm-rust-sha1-differential && ./build/bin/llvm-sha1-stable

# IR for the LEA fold experiments.
(cd llvm/lib/Support/rust/md5 &&
  RUSTFLAGS="-C lto=no -C target-cpu=generic" \
  cargo rustc --release -- --emit=llvm-ir)
IR=$(find "$PWD/llvm/lib/Support/rust/md5/target" -name '*.ll' | head -1)
build/bin/llc -O3 -mcpu=x86-64 -filetype=obj "$IR" -o /tmp/base.o
build/bin/llc -O3 -mcpu=x86-64 -filetype=obj -x86-fold-add-into-lea \
  "$IR" -o /tmp/fold.o

# The MIR fixtures live in .github/workflows/rust-support.yml (step "MIR flag
# regression"); save them as /tmp/{fold,carry,wide,dbg}.mir, then:
for case in fold carry wide dbg; do
  build/bin/llc -run-pass=x86-fixup-leas -x86-fold-add-into-lea \
    "/tmp/$case.mir" -o "/tmp/$case.out.mir"
done
grep -q 'LEA64_32r $rdi, 1, $noreg, 5, $noreg' /tmp/fold.out.mir
grep -q 'LEA64r $rdi, 1, $noreg, 5, $noreg' /tmp/wide.out.mir
grep -q ADD32ri /tmp/carry.out.mir              # carry live across INC
grep -q 'srcinst: 2, srcop: 0' /tmp/dbg.out.mir # ADD debug ref remapped

# Downstream link through llvm-config. Use g++, llvm-config does not add the
# C++ runtime.
g++ prog.cpp $(build/bin/llvm-config --cxxflags --ldflags --libs \
  --system-libs support)

# A component install has to carry the Rust archives too.
cmake --install build --prefix /tmp/llvminst --component LLVMSupport
```

`llvm-config` without a component list fails in this fork because the `engine`
group still lists the removed `interpreter` component; pass `support`
explicitly.
