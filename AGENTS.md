# AGENTS.md

Repository-specific notes for agents working on this fork of `llvm-project`.
The migration itself is described in `MIGRATION_TO_RUST.md`; read that first.

## What this fork is

- Shallow clone on branch `rust-migration-phase5` (`git rev-parse
  --is-shallow-repository` returns `true`). History before the tip is absent.
- The Rust migration of `lib/Support` components lives under
  `llvm/lib/Support/rust/<component>/`; demangler ports live under
  `llvm/lib/Demangle/rust/{rustdemangle,dlangdemangle}/`.
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
  -DLLVM_ENABLE_RUST_CONVERTUTF=ON -DLLVM_ENABLE_RUST_CONVERTEBCDIC=ON \
  -DLLVM_ENABLE_RUST_XXHASH=ON -DLLVM_ENABLE_RUST_DJB=ON \
  -DLLVM_ENABLE_RUST_RUSTDEMANGLE=ON -DLLVM_ENABLE_RUST_DLANG=ON \
  -DLLVM_TARGETS_TO_BUILD=X86 \
  -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_TERMINFO=OFF \
  -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_DOCS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
  -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_ENABLE_BINDINGS=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF

ninja -C build LLVMSupport llvm-sha1-stable llvm-sha256-stable \
  llvm-convertutf-stable llvm-convertebcdic-stable llvm-djb-stable \
  llvm-rustdemangle-stable llvm-dlang-stable \
  llvm-rust-rustdemangle-differential llvm-rust-dlang-differential \
  llvm-rust-djb-differential llc
```

Full LLVM builds are large; prefer building `LLVMSupport` and the specific
tool you need. Long builds must be started in the background and polled.

## Rust crate conventions

Every crate under `llvm/lib/*/rust/` follows the same rules:

- `#![cfg_attr(not(test), no_std)]`, `panic = "abort"`, aborting
  `#[panic_handler]`, no dependencies, no crates.io at build time.
- Only `extern "C"` functions cross the boundary. Data is `(pointer, length)`
  or fixed-size buffers; the C++ caller owns all memory. The two demanglers are
  deliberate exceptions: they return a `realloc`-allocated buffer that C++ frees
  with `std::free`, matching the `char *` contract of `llvm::rustDemangle` and
  `llvm::dlangDemangle`.
- Symbols are prefixed `llvm_<component>_<function>`.
- The C++ class layout and public headers do not change. Only the compression
  function/body is replaced; buffering and padding stay in C++.
- Each component has its own `LLVM_ENABLE_RUST_<COMPONENT>` switch, default
  `OFF`, and `Cargo.lock` is committed.

## Migration policy (agreed 2026-11-25)

The experiments showed that Rust can beat C++ on leaf components, but they do
not justify a whole-LLVM rewrite or a core-free default. Keep the migration
optional and incremental.

- Migrate self-contained leaf components first. Use the stable Rust toolchain
  and the normal `no_std` (plus `alloc` or `std` only when the component
  genuinely needs it) staticlib/C-ABI path. The default build must not invoke
  `cargo`, nightly, `build-std`, or `RUSTC_BOOTSTRAP`.
- The core-free artifact experiment (`-Z build-std=core,compiler_builtins`,
  `panic=immediate-abort`) is an **opt-in experiment for small, allocation-free
  leaf crates**, not the default and not a model for the whole project. It
  changes panic semantics and adds nightly/rust-src requirements.
- Direct C++ ABI exports (mangled symbols, `std::string_view`/`SmallVector`
  layout, Itanium ABI) are research prototypes only. Production components
  cross a C ABI; the public C++ headers and class layouts stay unchanged.
- Do not rewrite LLVM's IR, code generator, or target backends merely to gain
  performance. Rust and C++ both lower through LLVM; backend speed comes from
  algorithms, data layout, allocation, PGO/BOLT, ThinLTO, and target tuning.
  A whole-LLVM/rustc-backend rewrite is a separate research project with a
  bootstrap problem, not an incremental migration step.
- A new port ships only if it clears the measurement gate below; otherwise keep
  the C++ implementation. “Rust” is not itself a performance result.

Gate per component, measured on the same machine and input distribution:

- runtime at least 1.05x faster, or a clearly documented safety/size win;
- peak RSS no more than 2% higher;
- compile time no more than 10% higher;
- archive and linked binary not larger;
- option `OFF` builds without cargo and stays byte-identical;
- `cargo test`, differential fuzzing, and golden tests pass with the option
  `ON` and `OFF`.

Record rejected experiments and their numbers in `MIGRATION_TO_RUST.md` so the
same idea is not retried blindly.

MD5 is the exception that proves the codegen rule: it carries a generic x86-64
body that emits `a + x + t` as an explicit `lea` through `asm!` (LLVM otherwise
reassociates the constant to the end of the chain and loses the LEA) and a
`#[target_feature(enable = "bmi1,bmi2")]` copy selected by CPUID. The generic
copy is within 1% of `g++ -O3`; the BMI copy is 3-13% faster on Zen 4. Other
architectures use the pure-Rust add grouping.

ConvertUTF is the first whole-file port: every exported function in
`ConvertUTF.cpp` forwards to `llvm_convertutf_*`, and the C++ static tables and
fallback bodies are compiled only when the option is off. The public header and
`ConvertUTFWrapper.cpp` are unchanged. Its differential test compiles the
original `ConvertUTF.cpp` next to the Rust archive, so it needs the LLVM
include directories and does not link `LLVMSupport`.

Checks for a crate:

```sh
cd llvm/lib/Support/rust/<component>
cargo test
cargo build --release --locked
cargo check --target aarch64-unknown-linux-gnu   # xxhash: NEON path
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
`LLVMSupport` and the `*-stable` test, and confirm identical output. For
`LLVM_ENABLE_RUST_RUSTDEMANGLE` or `LLVM_ENABLE_RUST_DLANG`, rebuild
`LLVMDemangle` and the matching `*-stable` test instead.

## DJB port

`llvm/lib/Support/rust/djb` is gated by `LLVM_ENABLE_RUST_DJB` (default
`OFF`). `DJB.cpp` keeps the original implementation under `#else`; with the
option on, `llvm::caseFoldingDjbHash` is a C-ABI call to
`llvm_djb_case_folding_hash`. The Rust ASCII path uses a 256-byte fold table
and the same `h * 33 + c` recurrence as C++; the Unicode path reuses the
lenient one-code-point UTF-8 behaviour of `ConvertUTF`.

```sh
(cd llvm/lib/Support/rust/djb &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-djb-differential llvm-djb-stable
./build/bin/llvm-djb-stable
./build/bin/llvm-rust-djb-differential
```

The differential harness checks every legal Unicode scalar value, all
single-byte inputs, and 200,000 random valid UTF-8 names, then benchmarks
ASCII and Unicode. The C++ reference has an out-of-bounds write when a valid
code point is followed by a truncated sequence, so malformed multi-byte input
is not used as a differential oracle; the Rust port replaces it with U+FFFD.
Measured here with `g++ -O3` and `-C opt-level=3`: 1.6x to 1.7x on ASCII
debug-info names and 1.4x to 1.6x on non-ASCII names. The linked stable test
shrinks from 30,376 to 20,640 bytes.

## Demangler port

`llvm/lib/Demangle/rust/rustdemangle` is the Phase 2 port of
`RustDemangle.cpp`, gated by `LLVM_ENABLE_RUST_RUSTDEMANGLE` (default `OFF`).
`RustDemangle.cpp` keeps the original implementation under `#else`; with the
option on, its `llvm::rustDemangle` is a thin wrapper around
`llvm_rustdemangle_demangle`. The Rust side grows its own output with
`realloc`, reserves room for the NUL terminator, and returns the length; the C++
wrapper writes the terminator and returns the pointer for the caller to free
with `std::free`.

```sh
(cd llvm/lib/Demangle/rust/rustdemangle &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-rustdemangle-differential llvm-rustdemangle-stable
./build/bin/llvm-rustdemangle-stable
./build/bin/llvm-rust-rustdemangle-differential
```

`test/differential.cpp` compares the Rust archive against `RustDemangle.cpp`
on 205 real `libstd` symbols from the toolchain's `libstd-*.rlib`, 50,000
random byte strings, and 50,000 mutated symbols. It also prints the full
string-path and pointer-API throughput. `test/stable.cpp` runs the same golden
vectors with the option on and off. `test/corpus.txt` is the real-symbol
corpus; regenerate it from `nm -A libstd-*.rlib` if the toolchain changes.

The CMake targets build only `LLVMDemangle` and the Rust archive, not
`LLVMSupport`. With the option on, `llvm-config --system-libs` gains
`-lllvm_rustdemangle`, and `cmake --install --component LLVMDemangle` installs
`libLLVMDemangle.a` plus `libllvm_rustdemangle.a`.

Measured on this host (`g++ -O3` vs `-C opt-level=3`, generic x86-64, best of
five alternating rounds, with the Rust column calling the public wrapper):
1.08x to 1.14x on the full string path and 1.07x to 1.17x on the pointer API
over the 205-symbol corpus. The 50 longest symbols are 1.11x to 1.14x on the
full path and 1.09x to 1.13x on the pointer API. The 50 shortest symbols are
1.03x to 1.10x on the full path and 1.03x to 1.09x on the pointer API. The
character-class, base62, and hex lookup tables, the eight-byte suffix-dot scan,
and the 32-byte initial buffer for short inputs are what moved the parser ahead;
keep them when touching identifier, number, or suffix parsing.

`llvm/lib/Demangle/rust/dlangdemangle` is the Phase 2 port of
`DLangDemangle.cpp`, gated by `LLVM_ENABLE_RUST_DLANG` (default `OFF`). The
crate exports
`llvm_dlangdemangle_demangle(input, input_len, uint8_t **output) -> isize`
and uses the same output-ownership model as `llvm_rustdemangle_demangle`.
`DLangDemangle.cpp` becomes a thin wrapper when the option is on; the original
body remains under `#else` and is the differential reference.

```sh
(cd llvm/lib/Demangle/rust/dlangdemangle &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-dlang-differential llvm-dlang-stable
./build/bin/llvm-dlang-stable
./build/bin/llvm-rust-dlang-differential
```

The differential harness generates 448 valid symbols, 100,000 random byte
strings, 100,000 mutations of the corpus, and 100,000 targeted back-reference
inputs. It compares the raw `(pointer, length)` outputs, including bytes after
an embedded NUL, and calls the public `dlangDemangleViaFFI` wrapper so its
terminator write is checked for every input. It reports 0 mismatches and
benchmarks the public wrapper against the C++ reference. Mismatch diagnostics
print full byte sequences rather than C strings. Measured here with `g++ -O3`
and `-C opt-level=3` on generic x86-64, best of five rounds: 1.17x to 1.20x
on the full string path (including the caller's std::string copy), 1.21x to
1.26x on the pointer API, and 1.18x to 1.29x on long qualified names with
repeated back references. The first NUL is
found once with `memchr` and cached lazily, so NUL-free names do not rescan for
back references. A successful parse that stops at NUL is accepted even when the
explicit input length has trailing bytes, matching the C++ C-string check. With
the option on, `llvm-config --system-libs` gains
`-lllvm_dlangdemangle`; `cmake --install --component LLVMDemangle`
installs `libllvm_dlangdemangle.a` alongside `libLLVMDemangle.a`. An
install-only consumer links and runs without the build tree; the fork's partial
install still needs placeholder files for the other uninstalled LLVM libraries.
A toolchain-only install omits the companion archive along with `LLVMDemangle`.

## Benchmarks

Every C++/Rust comparison must measure both sides, not only throughput:

- **Compile:** clean and incremental wall time, user/system CPU, peak RSS,
  object/archive bytes, and final linked binary bytes. Record compiler flags
  and Rust flags.
- **Runtime:** wall time, user/system CPU, peak RSS, throughput, and code size
  or instruction counts where relevant.
- Use the same optimization level (`g++ -O3` against `-C opt-level=3`), input
  corpus, CPU affinity, and warmup. Report at least five repetitions as median
  and min-max. Never compare `-O2` C++ with `-O3` Rust.
- Toolchain caveat: `cargo build` invokes the installed `rustc`, which uses its
  own bundled LLVM (currently 23.1.6), not the LLVM built from this repository.
  The C++ reference is built by GCC 14.2. The repository's LLVM is used by
  `llc` only in the explicit IR/backend experiments. Do not describe these as
  same-LLVM codegen comparisons.
- Separate process-level CPU/RSS from per-operation timings. Isolate a backend
  with `--no-checks --bench-backend=cpp|rust` before comparing resources.
- `llvm/utils/rust-migration/bench_metrics.py` measures wall time, user/system
  CPU, peak RSS, and artifact bytes for either a compile command or a runtime
  benchmark:

```sh
python3 llvm/utils/rust-migration/bench_metrics.py \
  --label cpp-compile --repeat 5 --artifact /tmp/ebcdic.o \
  --prepare 'rm -f /tmp/ebcdic.o' -- sh /tmp/compile-ebcdic.sh

python3 llvm/utils/rust-migration/bench_metrics.py \
  --label rust-runtime --repeat 5 -- \
  ./build/bin/llvm-rust-convertebcdic-differential --no-checks \
  --bench-backend=rust
```

`size`/`nm` report final linked and section sizes. A large static Rust archive
is not the same as a large linked binary; report both.

- The crates link as a plain static archive, so the differential test can be
  built without CMake:
  `g++ -O3 test/differential.cpp -Ltarget/release -lllvm_sha256 -o /tmp/diff`
  (add `-march=native` only when comparing native codegen on both sides).
  Base64 is the exception: its differential calls `llvm::decodeBase64`, so it
  needs `LLVMSupport` and is built by CMake:
  `ninja -C build llvm-rust-base64-differential && ./build/bin/llvm-rust-base64-differential`.
  xxHash compiles the C++ reference as a second translation unit:
  `cd llvm/lib/Support/rust/xxhash && cargo build --release --locked && g++ -O3 -std=c++17 -I ../../../../include -I <build>/include test/differential.cpp ../../xxhash.cpp -Ltarget/release -lllvm_xxhash -o /tmp/xxhash-diff && /tmp/xxhash-diff`.
  ConvertUTF compiles the C++ reference as a second translation unit:
  `cd llvm/lib/Support/rust/convertutf && cargo build --release --locked && g++ -O3 -std=c++17 -I ../../../../include -I <build>/include test/differential.cpp ../../ConvertUTF.cpp -Ltarget/release -lllvm_convertutf -o /tmp/convertutf-diff && /tmp/convertutf-diff`.
  Add `-DLLVM_RUST_CONVERTUTF` to exercise the C++ shim instead of the
  reference bodies.
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

ninja -C build llvm-rust-xxhash-differential llvm-xxhash-stable
./build/bin/llvm-rust-xxhash-differential && ./build/bin/llvm-xxhash-stable

ninja -C build llvm-rust-rustdemangle-differential llvm-rustdemangle-stable
./build/bin/llvm-rust-rustdemangle-differential
./build/bin/llvm-rustdemangle-stable

ninja -C build llvm-rust-dlang-differential llvm-dlang-stable
./build/bin/llvm-rust-dlang-differential && ./build/bin/llvm-dlang-stable

# The AArch64 NEON job runs the crate tests under qemu. Use a subshell so the
# later checks stay at the repository root.
(
  cd llvm/lib/Support/rust/xxhash
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER="qemu-aarch64 -L /usr/aarch64-linux-gnu" \
  cargo test --target aarch64-unknown-linux-gnu
)

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
cmake --install build --prefix /tmp/llvminst --component LLVMDemangle
```

`llvm-config` without a component list fails in this fork because the `engine`
group still lists the removed `interpreter` component; pass `support`
explicitly.
