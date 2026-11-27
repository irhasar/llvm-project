# AGENTS.md

Repository-specific notes for agents working on this fork of `llvm-project`.
The migration itself is described in `MIGRATION_TO_RUST.md`; read that first.

## What this fork is

- Shallow clone on the current Rust migration branch (`git rev-parse
  --is-shallow-repository` returns `true`). History before the tip is absent.
- The Rust migration of `lib/Support` components lives under
  `llvm/lib/Support/rust/<component>/`; demangler ports live under
  `llvm/lib/Demangle/rust/{rustdemangle,dlangdemangle}/`.
- `llvm/test` and `llvm/docs` are **not** present. LLVM regression tests
  (`ninja check-llvm`, `llvm-lit`) cannot be run. Do not claim they passed.
  Each Rust port therefore ships its own differential and golden tests.

## Toolchain in this container

- `rustc`/`cargo` 1.100.0 (stable), edition 2024 crates (`rust-version = "1.85"`).
  `rust-toolchain.toml` pins that stable release; CI also runs the crates with
  `nightly-2026-11-23` for compatibility. Use `cargo +nightly-2026-11-23` for
  the nightly leg; nightly is not required by the default build.
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
  -DLLVM_ENABLE_RUST_VERSIONTUPLE=ON -DLLVM_ENABLE_RUST_OPTIONSTRCMP=ON \
  -DLLVM_ENABLE_RUST_NATIVEFORMATTING=ON -DLLVM_ENABLE_RUST_UNICODE=ON \
  -DLLVM_ENABLE_RUST_STRINGEXTRAS=ON \
  -DLLVM_ENABLE_RUST_RUSTDEMANGLE=ON -DLLVM_ENABLE_RUST_DLANG=ON \
  -DLLVM_TARGETS_TO_BUILD=X86 \
  -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_TERMINFO=OFF \
  -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_INCLUDE_TESTS=OFF \
  -DLLVM_INCLUDE_DOCS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
  -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_ENABLE_BINDINGS=OFF \
  -DLLVM_ENABLE_LIBEDIT=OFF

ninja -C build LLVMSupport llvm-sha1-stable llvm-sha256-stable \
  llvm-convertutf-stable llvm-convertebcdic-stable llvm-djb-stable \
  llvm-versiontuple-stable llvm-optionstrcmp-stable \
  llvm-rustdemangle-stable llvm-dlang-stable llvm-unicode-stable \
  llvm-rust-rustdemangle-differential llvm-rust-dlang-differential \
  llvm-rust-djb-differential llvm-rust-versiontuple-differential \
  llvm-rust-optionstrcmp-differential llvm-rust-unicode-differential llc
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
- `AddRustLibrary` uses `INTERFACE_LINK_DEPENDS` on Ninja. Do not replace it
  with `add_dependencies` on the imported archive: CMake then makes every C++
  object in a consumer wait for Cargo. Makefile and IDE generators keep the
  target-level dependency.
- The staged archive is a byproduct of the staging-stamp rule. Keep the stamp;
  making the archive itself the rule output makes Make recopy it every build
  after a byte-identical Cargo rebuild. Staging and Make restores write a
  temporary file and rename it, so an interrupted copy cannot leave a partial
  archive that the next build accepts. Keep `RustStageArchive.cmake` in the
  staging rule's dependencies so editing the script restages. The Make fallback
  for a missing Cargo archive verifies the staged archive against the content
  stamp before using it.
- Release staticlibs use fat LTO and one codegen unit. Small standalone Rust
  binaries use LTO off; larger ones try thin LTO first. Development and test
  builds use the incremental dev profile. See `MIGRATION_TO_RUST.md` for the
  measured rationale.
- Exception: `llvm/lib/Support/rust/unicode` uses `lto = false`. Its
  table-driven function gains nothing from cross-crate LTO, and rustc's LTO
  pass was the peak-RSS bottleneck for this small crate. The exception is
  measured in the Unicode resource table; new crates still require fat LTO.

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
- Track A, this branch, remains the production path. Track B is a long-term
  Rust-native LLVM core on `rust-migration-track-b` and in a separate
  `llvm-rust/` workspace; it must not change the default build or any Track A
  `LLVM_ENABLE_RUST_*` switch.
- A new port ships only if it clears the measurement gate below; otherwise keep
  the C++ implementation. “Rust” is not itself a performance result.
- Keep the C ABI staticlib as the permanent boundary for existing C++ callers.
  Pure Rust from the start is for new Rust-only tools, not a replacement for
  LLVM's C++ entry points. Keep the Rust core safe and the C ABI adapter thin
  so a future Rust caller can reuse the core without another port.

Gate per component, measured on the same machine and input distribution:

- runtime at least 1.05x faster, or a clearly documented safety/size win;
- peak RSS no more than 2% higher;
- compile time no more than 10% higher;
- final linked binary not larger;
- option `OFF` builds without cargo and stays byte-identical;
- `cargo test`, differential fuzzing, and golden tests pass with the option
  `ON` and `OFF`.

The Rust staticlib archive is reported separately from the final binary. It
carries `core`, `compiler_builtins`, and debug sections that the final linker
removes; `strip --strip-debug` is a release-packaging option. Do not reject a
component on archive size alone when the linked binary and installed payload
are not larger.

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

## VersionTuple port

`llvm/lib/Support/rust/versiontuple` is gated by
`LLVM_ENABLE_RUST_VERSIONTUPLE` (default `OFF`). Only
`VersionTuple::tryParse` is replaced; the class layout, constructors,
`getAsString`, `operator<<`, and `withMajorReplaced` stay in C++. The Rust
entry point is `llvm_versiontuple_try_parse(data, len, *out)` returning 0 on
success; `out` carries five `uint32_t` components and a flags word.

```sh
(cd llvm/lib/Support/rust/versiontuple &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-versiontuple-differential llvm-versiontuple-stable
./build/bin/llvm-versiontuple-stable
./build/bin/llvm-rust-versiontuple-differential
```

The differential target links `LLVMSupport` and measures the public
`VersionTuple::tryParse` path. `test/reference.cpp` carries the original parse
loop as a free-function oracle. Coverage is the boundary corpus plus 300,000
random ASCII strings; the observed result is 300,093 inputs and 0 mismatches.
`test/stable.cpp` checks component presence and that a failed parse leaves the
destination unchanged.

Measured here with `g++ -O3` and `-C opt-level=3` on five-component versions,
including the C++ wrapper's tuple construction in the Rust row: C++ 15.75 ns
(15.50-16.04), Rust public wrapper 14.75 ns (14.47-15.48). Both rows consume all
five tuple fields, so the compiler cannot drop the Rust tuple's unused fields.
The raw C ABI parser is about 11.5 ns, but the public API must still build the
`VersionTuple`; the public speedup is 1.02x-1.09x with a 1.07x median across
five runs. The median clears the runtime gate, but one run was just under it,
so this is a narrow win. The port stays opt-in because the archive-size and
compile-resource gates still fail. The parser uses wrapping `u32` arithmetic
and masks minor/subminor to 31 bits so oversized components match the C++
bitfield truncation.

## OptionStrCmp port

`llvm/lib/Support/rust/optionstrcmp` is gated by
`LLVM_ENABLE_RUST_OPTIONSTRCMP` (default `OFF`). `StrCmpOptionName` becomes a
C-ABI call to `llvm_optionstrcmp_name`; `StrCmpOptionPrefixes` stays in C++
and calls the wrapper for each pair, so the `ArrayRef` iteration and its
layout remain on the C++ side.

```sh
(cd llvm/lib/Support/rust/optionstrcmp &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-optionstrcmp-differential llvm-optionstrcmp-stable
./build/bin/llvm-optionstrcmp-stable
./build/bin/llvm-rust-optionstrcmp-differential
```

The differential target compiles the original `OptionStrCmp.cpp` and supplies
the one `StringRef::compare_insensitive` entry point it needs, so it does not
link `LLVMSupport`. Coverage includes every pair of single bytes, the option
name corpus with both fallback settings, prefix arrays of different lengths,
and 300,000 random pairs: 449,588 inputs, 0 mismatches. `test/stable.cpp`
checks the public API with the option on and off.

Measured here with `g++ -O3` and `-C opt-level=3`, alternating runs over 205
option names: C++ 6.23 ns, public Rust wrapper 5.00 ns, or 1.25x faster. A
direct call to `llvm_optionstrcmp_name` is 4.74 ns; the 0.26 ns difference is
the C++ wrapper and C-ABI call. The Rust loop folds ASCII in one pass, reads
both lengths before the loop, and avoids the two `StringRef::substr`
temporaries and the out-of-line `compare_insensitive` call.

## Unicode case folding port

`llvm/lib/Support/rust/unicode` is gated by `LLVM_ENABLE_RUST_UNICODE`
(default `OFF`). The generated `UnicodeCaseFold.cpp` stays untouched; with the
option on, `UnicodeCaseFoldRust.cpp` replaces it and `foldCharSimple` is a
C-ABI call to `llvm_unicode_fold_char_simple`. The Rust implementation uses
691 sorted ranges and a binary search instead of the generated comparison
chain, with an ASCII fast path. Starts are stored as `u8` offsets from
256-code-point block bases selected by a packed 5-bit block index; lengths are
`u8`. Delta 1 is a bit in a bitset, and the other 146 deltas are selected by
rank from a sparse `i32` table. All table reads use checked slice access with
non-panicking fallbacks; there is no `get_unchecked` in the lookup path.
`PRUNE_ARCHIVE` removes unused `compiler_builtins` members, so the default
staged archive is 6,304 bytes instead of 5 MiB. Pruning is conditional: if the
retained object references any symbol outside a small libc/panic allowlist,
the script keeps the complete archive. Overflow checks trigger that fallback,
and the wrapper supplies a weak `rust_eh_personality` for the pulled-in `core`.
The script uses `list(FIND)` rather than policy-dependent `IN_LIST`, so its
standalone `cmake -P` invocation works with CMake 3.x; it accepts both GNU
`nm` and `llvm-nm` POSIX records. The overflow test tracks `LLVM_RUST_FLAGS`
through a generated dependency, matching the production rule.

```sh
(cd llvm/lib/Support/rust/unicode &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build llvm-rust-unicode-differential llvm-unicode-stable
./build/bin/llvm-unicode-stable
./build/bin/llvm-rust-unicode-differential

# Overflow/debug-check regression: prunes the checks-enabled archive and links.
ninja -C build check-llvm-rust-unicode-overflow
```

The differential target compiles the C++ fallback and compares every code
point from -1 through 0x110000; the stable target exercises the public API
with the option on and is built in both configurations. Five pinned runs over
five million random code points: C++ 40.47 ns, Rust 18.68 ns, 2.17x faster.
The overflow target builds the crate with `-Coverflow-checks=yes`; a separate
configure with `-DLLVM_RUST_FLAGS=-Coverflow-checks=yes` links
`llvm-unicode-stable` as well.

The public CMake package must import the Rust archive, not leave a bare
`llvm_unicode` name in `LLVMSupport`'s interface:

```sh
cmake -S /tmp/unicode-consumer -B /tmp/unicode-consumer-build -G Ninja \
  -DLLVM_DIR=$PWD/build/lib/cmake/llvm
ninja -C /tmp/unicode-consumer-build
```

The resource gate is cleared. The C++ one-TU compile is 418.98 ms and 95.93
MiB peak RSS; the Rust crate plus wrapper is 141.77 ms and 94.31 MiB peak RSS
(0.34x wall, -1.7% RSS). Incremental rebuilds, including copy, prune, strip,
and wrapper compile, are 336.84 ms and 94.32 MiB against 414.38 ms and 96.00
MiB for the C++ recompile (0.81x wall, -1.8% RSS). The wrapper includes only
`Compiler.h`; pulling in `Unicode.h` costs 354 ms for the one-line wrapper and
breaks the compile gate. `llvm-min-tblgen` is byte-identical (1,796,632
bytes). `llvm-unicode-stable` is 20,288 bytes with C++ and 16,408 bytes with
Rust (-3,880). The option stays opt-in by policy.

## StringExtras port

`llvm/lib/Support/rust/stringextras` ports the ASCII camel/snake conversions
behind `LLVM_ENABLE_RUST_STRINGEXTRAS` (default `OFF`). The public
`StringExtras.h` API and the other `StringExtras.cpp` functions are unchanged;
the C++ wrapper owns the `std::string` and the Rust crate writes into a
caller-sized buffer. The wrapper is the minimal `StringExtrasRust.cpp`, not
`StringExtras.cpp`, so it does not pull `raw_ostream` into small consumers.
The C++ fallback keeps the original `<cctype>` body so option `OFF` is
byte-identical to the pre-port build. The Rust path and differential oracle
use LLVM's C-locale ASCII helpers; the differential runs in the C locale. The
wrapper also supplies a weak `rust_eh_personality` for debug-assertion
archives.
The crate uses `PRUNE_ARCHIVE` and `STRIP_DEBUG`, so the staged archive is
1,900 bytes. The archive is installed with `LLVMSupport`, and
`llvm-config --system-libs support` emits `-lllvm_stringextras`.

```sh
(cd llvm/lib/Support/rust/stringextras &&
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test &&
  cargo build --release --locked)

ninja -C build check-llvm-rust-stringextras check-llvm-rust-stringextras-debug
```

`test/differential.cpp` compares the public Rust path with an exact C++
reference over 300,810 inputs, including all 256 single bytes and 100,000
random strings; `test/stable.cpp` is built with the option on and off.
`check-llvm-rust-stringextras-debug` builds the crate with
`-Cdebug-assertions=yes -Coverflow-checks=yes` and links it without
`LLVMSupport`. It encodes those flags, plus `LLVM_RUST_FLAGS` and any ambient
`CARGO_ENCODED_RUSTFLAGS`, so Cargo cannot ignore the checks. Its staging uses
the normal build/content/staged stamp scripts, so Make does not repeat staging
after a byte-identical Cargo rebuild. Five pinned runs
on this Zen 4 host: camel-to-snake is 1.33x faster (96.44 ns -> 72.36 ns),
snake-to-camel is 1.10x faster (26.65 ns -> 24.31 ns). The Rust stable binary
is 17,424 bytes versus 1,558,560 bytes for the C++ fallback (-1,541,136
bytes); the staged archive is 1,900 bytes. The option-off object and stable
binary are byte-identical to the baseline. The compile rows include the
`StringExtras.cpp` object that is still built with Rust on; the actual
incremental Ninja build is 842 ms for Rust versus 880 ms for C++ (0.96x), and
peak RSS is 113.94 MiB versus 116.23 MiB (-2.0%). The option stays opt-in.

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
  benchmark. On Linux it compiles and probes `measure_exec.c` in a temporary
  directory, then forks a distinct child, so `ru_maxrss` excludes Python's
  pre-exec pages. A `noexec` temp mount makes `auto` fall back to `wait4` with a
  warning; `--rss-backend=launcher` fails with an actionable error. The
  temporary directory is removed after every run. Artifact paths resolve
  against `--cwd`, and output reports median plus min-max for wall, user, and
  system time and peak RSS:

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

# The Unicode archive must also link when Rust checks are enabled.
ninja -C build check-llvm-rust-unicode-overflow

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

# A component install has to carry the Rust archives too. A static component
# does not link them, so build the aggregate first for a direct install.
ninja -C build LLVMSupport LLVMSupport-rust LLVMDemangle LLVMDemangle-rust
cmake --install build --prefix /tmp/llvminst --component LLVMSupport
cmake --install build --prefix /tmp/llvminst --component LLVMDemangle

# LLVMConfig.cmake must recreate every enabled Rust archive as an imported
# target. Check it from a consumer without adding the prefix lib dir:
#   get_target_property(loc llvm_convertebcdic IMPORTED_LOCATION)
```

`llvm-config` without a component list fails in this fork because the `engine`
group still lists the removed `interpreter` component; pass `support`
explicitly.
