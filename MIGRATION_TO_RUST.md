# Incremental migration of LLVM components to Rust

This document describes an experiment in porting LLVM components to Rust one at
a time, without a flag day and without making the Rust toolchain a hard
dependency of the build. The Phase 1 `lib/Support` slices are implemented and
verified, and Phase 2 started with `llvm/lib/Demangle/RustDemangle.cpp` and
`llvm/lib/Demangle/DLangDemangle.cpp`; the remaining phases below are the plan.

## Goals and non-goals

Goals:

* Replace individual, self-contained components with Rust implementations that
  are bit-for-bit compatible with the C++ they replace.
* Keep every step independently revertible, reviewable, and off by default.
* Keep the C++ ABI stable: callers of `llvm/Support/*.h` must not notice.

Non-goals (for now):

* Rewriting the IR, the code generator, or the target backends. Those are
  data-structure-heavy and dominated by C++ template use; they are the last
  candidates, not the first.
* Introducing Rust into the bootstrap path. Building LLVM with a Rust compiler
  remains optional.
* A mixed-language build for downstream consumers that do not opt in.

## Ground rules for the FFI boundary

1. Only `extern "C"` functions cross the boundary. No Rust `struct`, `enum`,
   `Result`, `Option`, `String`, or `Vec` appears in a signature.
2. Data is passed as `(pointer, length)` or fixed-size byte arrays. The C++
   caller owns all memory; Rust only reads inputs and writes caller-provided
   output buffers.
3. Panics abort. Crates are `no_std` where possible, built with
   `panic = "abort"`, and install an aborting `#[panic_handler]`. Unwinding
   across an FFI frame is never allowed.
4. No global mutable state, no thread-local storage, and no allocation unless
   the C++ side already allocated. This keeps the port compatible with LLVM's
   existing threading and allocator assumptions. The exceptions are write-once,
   relaxed atomic caches of the CPU feature checks used to pick
   runtime-dispatched paths (SSSE3, SHA-NI, BMI1/BMI2, AVX2); they are idempotent and
   thread-safe.
5. Symbol names are prefixed with the component: `llvm_<component>_<function>`.
6. Every crate carries the LLVM license header and
   `license = "Apache-2.0 WITH LLVM-exception"` in `Cargo.toml`.
7. Third-party crates are not pulled from crates.io at build time. If a
   dependency is unavoidable it is vendored under `third-party/` and reviewed
   like any other LLVM dependency.

## Build integration

`llvm/cmake/modules/AddRustLibrary.cmake` provides
`add_llvm_rust_staticlib(<target> <manifest>)`. It:

* locates `cargo` (or `LLVM_RUST_CARGO_EXECUTABLE`),
* builds the crate into the CMake binary tree with `--release --locked`,
* exposes the archive as a `STATIC IMPORTED` target that components link.

Crates live next to the component they replace
(`llvm/lib/<Component>/rust/<crate>/`), and their `Cargo.lock` is committed.
All crates use edition 2024 (`rust-version = "1.85"`) with `no_std`,
`panic = "abort"`, LTO and one codegen unit. `rust-toolchain.toml` pins the
latest nightly (tested with `rustc 1.102.0-nightly`); the crates still build on
stable Rust 1.85 and newer.
Each component is gated behind its own `LLVM_ENABLE_RUST_<COMPONENT>` option,
defaulting to `OFF`.

Known limitation: cross-compiling needs an explicit `--target` mapping that the
helper does not implement yet. Until then, Rust ports are only built for
native configurations.

## Phases

### Phase 0 - infrastructure and pilot (done)

* `AddRustLibrary.cmake`, `LLVM_ENABLE_RUST_SIPHASH`, and the first crate.
* `SipHash.cpp` dispatches to Rust behind the existing `LLVM_ABI` entry points.
* Differential and golden tests wired into the build.

### Phase 1 - pure leaf functions in lib/Support (in progress)

Done: `CRC.cpp` (`LLVM_ENABLE_RUST_CRC`, 6x faster than the C++ fallback),
`MD5.cpp` (`LLVM_ENABLE_RUST_MD5`), `SHA1.cpp` (`LLVM_ENABLE_RUST_SHA1`),
`SHA256.cpp` (`LLVM_ENABLE_RUST_SHA256`), `Base64.cpp`
(`LLVM_ENABLE_RUST_BASE64`), `ConvertUTF.cpp`
(`LLVM_ENABLE_RUST_CONVERTUTF`), `ConvertEBCDIC.cpp`
(`LLVM_ENABLE_RUST_CONVERTEBCDIC`), and `xxhash.cpp`
(`LLVM_ENABLE_RUST_XXHASH`). Selection criteria: small, deterministic,
byte-buffer in / byte-buffer out, no LLVM types in the interface, existing unit
tests.

`Base64` needed one structural concession: `encodeBase64` is a template in the
public header, so its body cannot be swapped per translation unit without the
option's define leaking into every consumer. The template now resizes the
result string and calls the exported `llvm_base64_encode` symbol. `Base64.cpp`
always defines that symbol, using the original scalar loop when the Rust crate
is off and forwarding to the separately named `llvm_base64_encode_rust` when it
is on. Keeping the `LLVM_ABI`-annotated definition in C++ also keeps the symbol
exported from Windows DLL builds. The public API and the output are unchanged.
The cost when the option is off is one out-of-line call per `encodeBase64`
invocation instead of an inlined loop.

The Rust encoder uses an SSSE3 path (runtime-detected through CPUID, with the
scalar path as the fallback) that turns 12 bytes into 16 characters per
iteration. The decoder validates four characters at a time. The public wrapper
validates before sizing the output, so invalid input never allocates the
maximum decoded length and keeps the same partial output as the C++ loop. It
reproduces the C++ `=` placement rules and the same partial output on error.
On this Zen 4 host at `-O3` versus `-C opt-level=3`, encoding is at parity to
1.1x faster at 12 bytes and 1.5x to 6.0x faster from 48 bytes to 1 MiB. The
differential target links `LLVMSupport` and calls the real `llvm::decodeBase64`,
so its correctness sweep covers the public wrapper. The throughput table times
the public path, which is 3.0x to 4.0x faster across the same range.
`llvm/lib/Support/rust/base64/test/differential.cpp` carries the reference
implementations and both the single-pass and public-path throughput rows.

Measured on this Zen 4 host, `g++ -O3` against `-C opt-level=3`, both on the
generic x86-64 target unless noted:

| Component       | Rust vs C++                                   |
|-----------------|-----------------------------------------------|
| CRC-32          | 6.2x vs the byte loop, 1.3x vs slicing-by-8    |
| SHA-1           | 1.9x (SHA-NI path)                            |
| SHA-256         | 4.1x (SHA-NI path)                            |
| Base64 encode   | 1.0x-1.1x at 12 B, 1.5x at 48 B, 6.0x at 1 MiB |
| Base64 decode   | 3.0x at 12 B, 3.8x at 1 MiB                   |
| MD5             | 1.03x to 1.13x (LEA asm + BMI1/BMI2 path)     |
| ConvertUTF      | 4.5x UTF-8 ASCII, 1.2-1.5x mixed; 1.6x UTF-32->UTF-16 ASCII; 12x `isLegalUTF8String` |
| ConvertEBCDIC   | 1.11x UTF-8 output, 1.16-1.22x EBCDIC output on 16-64 B payloads |
| xxHash 64-bit   | 0.99x at 12 B, 1.1x at 48 B, 2.5x at 768 B, 3.5x at 1 MiB |
| xxHash 128-bit  | 1.04x at 12 B, 1.2x at 48 B, 2.3x at 768 B, 3.5x at 1 MiB |

MD5 initially lost to GCC at the generic target (0.83x). LLVM's reassociation
moved the round constant to the end of `a + f + x + t`, leaving three register
operands for the backend, while GCC folded `a + x + t` into one LEA
(base + index + displacement) and added `f` separately. The crate now emits
that LEA explicitly with a small `asm!` block, which brings the generic body to
parity, and adds a `#[target_feature(enable = "bmi1,bmi2")]` body selected by
CPUID so `rorx` and `andn` are used when the CPU has them. On this Zen 4 host
the dispatched body is 1.03x to 1.13x faster than the C++ reference; the
generic body is 0.99x to 1.07x (parity to slightly faster). The `asm!` path is
x86-64 only; other targets use the pure-Rust grouping. The crate tests
compare the generic and BMI bodies bit for bit.

The only sub-1.0 rows are SHA-1's scalar fallback on long inputs
(0.96x-0.97x, within the 5% gate), xxHash 64-bit at 12 bytes (0.99x, parity),
UTF-16 -> UTF-8 mixed at 16 units (0.99x), and the exact-size UTF-32 -> UTF-16
path at 16-64 units (0.94x-0.98x); the last three are small-input overhead. The default SHA-NI path is 1.8x faster, so
the shipped configuration still wins. SipHash is at parity under generic
codegen (0.99x-1.03x) and 5-9% faster with `-C target-cpu=native`.

The point of this phase is volume: prove that the plumbing works for many small
crates, and settle conventions (error reporting via return codes, output buffer
sizes, debug info) before touching anything structural.

### Phase 2 - demanglers (in progress)

The first Phase 2 port is `llvm/lib/Demangle/RustDemangle.cpp` behind
`LLVM_ENABLE_RUST_RUSTDEMANGLE` (default `OFF`). The crate lives in
`llvm/lib/Demangle/rust/rustdemangle` and follows the same rules as the
`lib/Support` ports: `no_std`, no dependencies, `panic = "abort"`, and a
committed `Cargo.lock`.

The Rust entry point is
`llvm_rustdemangle_demangle(input, input_len, uint8_t **output) -> isize`.
Unlike the byte-buffer ports, it owns the output allocation: it grows a buffer
with `realloc`, returns its length, and leaves the pointer for the C++ caller to
release with `std::free`. That matches the existing `llvm::rustDemangle`
contract, which returns a malloc'd NUL-terminated string. `RustDemangle.cpp`
becomes a thin wrapper when the option is on; the original C++ implementation
remains in the `#else` branch and is still the reference used by the
differential test.

Correctness coverage:

* `cargo test` runs four golden groups, a suffix-dot scan test, a punycode
  identifier, and a `.llvm.<hash>` suffix.
* `llvm-rust-rustdemangle-differential` demangles 205 real `libstd` symbols
  extracted from the toolchain's `libstd-*.rlib`, then 50,000 random byte
  strings and 50,000 mutations of the corpus. Both backends must agree on
  success, failure, and the exact output.
* `llvm-rustdemangle-stable` contains the same golden vectors and runs with the
  option on and off, so the C++ and Rust backends are checked against identical
  expected values.

The corpus and the differential test are the performance harness. Three
changes moved the port from slightly slower to consistently faster:

* Branchy ASCII classification in identifier and number parsing became three
  256-byte lookup tables (`CHAR_CLASS`, `BASE62_DIGITS`, `HEX_DIGITS`).
* The `.<suffix>` scan uses an eight-byte-at-a-time match instead of a byte
  loop, matching what `std::string_view::find` gets from `memchr`. On short
  symbols this scan was the largest remaining cost.
* Short inputs (at most 64 bytes) start with a 32-byte output buffer; the C++
  `OutputBuffer` hysteresis is kept once the buffer reaches 1 KiB.

Output growth reserves the terminator only once at the end, so the parser pays
one capacity compare per append instead of a growth check.

Measured on this Zen 4 host with `g++ -O3` against `-C opt-level=3`, generic
x86-64 on both sides:

| Corpus                         | Full string path | Pointer API |
|--------------------------------|------------------|-------------|
| 205 real `libstd` symbols      | 1.08x - 1.14x    | 1.07x - 1.17x |
| 50 shortest symbols            | 1.03x - 1.10x    | 1.03x - 1.09x |
| 50 longest symbols             | 1.11x - 1.14x    | 1.09x - 1.13x |

Each row is the best of five rounds, alternating the two backends, to keep CPU
frequency drift out of the comparison. The harness compiles the C++ backend
under `llvm::rustDemangleCpp`, so the Rust column calls the actual public
`llvm::rustDemangle` wrapper, including its prefix check and terminator write.
The pointer-API column applies `strlen` to both backends so both do the same
work; the full path times the same call plus the caller's string copy. Every
row is faster, including the short-symbol pointer API.

A local configure with only `LLVM_ENABLE_RUST_RUSTDEMANGLE=ON` produced
`LLVM_SYSTEM_LIBS = "-lrt -ldl -lm -lllvm_rustdemangle"`. The
`llvm-rust-rustdemangle-differential` and `llvm-rustdemangle-stable` targets
build without `LLVMSupport`, and `cmake --install --component LLVMDemangle`
installs both `libLLVMDemangle.a` and `libllvm_rustdemangle.a`. Configuring the
option `OFF` produces the original system-libs list and the stable test still
passes against the C++ backend.

The second Phase 2 port is `llvm/lib/Demangle/DLangDemangle.cpp`, gated by
`LLVM_ENABLE_RUST_DLANG` (default `OFF`). The crate lives in
`llvm/lib/Demangle/rust/dlangdemangle` and exports
`llvm_dlangdemangle_demangle(input, input_len, uint8_t **output) -> isize`; it
uses the same `realloc`-owned output and NUL-terminator contract as the Rust
demangler.
`DLangDemangle.cpp` keeps the original implementation under `#else`, which the
differential harness compiles as `llvm::dlangDemangleCpp`.

The differential test generates 448 valid symbols, 100,000 random byte strings,
100,000 mutations of the generated corpus, and 100,000 targeted back-reference
inputs. It reports 0 mismatches on all 300,000 fuzz inputs and on the corpus.
`llvm-dlang-stable` runs the golden vectors with the option on and off. Measured
on the same Zen 4 host with `g++ -O3` and `-C opt-level=3` on generic x86-64,
best of five rounds, the public wrapper is 1.09x to 1.17x faster on the full
string path (including the caller's std::string copy), 1.10x to 1.21x faster on
the pointer API, and 1.11x to 1.19x faster on long qualified names with repeated
back references. The first NUL is found once with `memchr` and cached lazily, so
NUL-free names do not rescan for back references. A successful parse that stops
at NUL is accepted even when the explicit input length has trailing bytes,
matching the C++ C-string check.

The CMake targets build only `LLVMDemangle` and the Rust archive. With the
option on, `llvm-config --system-libs` gains `-lllvm_dlangdemangle`, and
`cmake --install --component LLVMDemangle` installs `libllvm_dlangdemangle.a`
next to `libLLVMDemangle.a`; an install-only downstream consumer links and runs
without the build tree (with the fork's usual placeholders for uninstalled LLVM
libraries); toolchain-only installs omit the archive with `LLVMDemangle`.
Configuring `LLVM_ENABLE_RUST_DLANG=OFF` keeps the
original C++ body and still passes `llvm-dlang-stable`.

### Phase 3 - object file readers

`llvm/lib/Object` needs structured data across the boundary. This is where a
handle-based C ABI (`llvm_object_open` returning an opaque pointer) has to be
designed and reviewed. Expect a longer design phase than implementation phase.

### Phase 4 - standalone tools

Tools such as `llvm-objdump` and `llvm-readobj` are process-level programs. They
can be rewritten as Rust binaries with no ABI concerns at all, and they are the
cheapest place to gain whole-program safety wins.

## Verification protocol

Every port must pass all of the following before it can ship enabled:

1. Unit tests with published or generated vectors inside the crate.
2. Differential fuzzing against the C++ implementation: random inputs across
   all length classes, including zero-length and non-multiple-of-block sizes.
3. End-to-end golden tests through the public C++ header, built twice (C++ and
   Rust backends) and compared byte-for-byte.
4. The full existing test suite in both configurations, with the option `ON`
   and `OFF`.
5. Performance measured on the same machine and input distribution; a
   regression above 5% needs an explicit justification.
6. Binary size delta for a representative consumer; above 2% needs a
   justification.
7. No new mandatory dependency when the option is `OFF`.

## Pilot results: SipHash

Implementation: `llvm/lib/Support/rust/siphash` (`no_std`, zero dependencies,
64- and 128-bit outputs, C ABI matching `llvm::getSipHash_2_4_{64,128}`).

Correctness:

* 16 published SipHash-2-4 vectors match.
* 262,656 randomized differential cases (lengths 0..512, random keys and data)
  are bit-identical to `third-party/siphash` for both output sizes.
* The golden test over `getStableSipHash` and
  `getPointerAuthStableSipHash` passes when `LLVMSupport` is built with either
  backend, and the two binaries produce identical results.

Performance. Measured on one x86-64 container (AMD EPYC 9B14, Zen 4,
`rustc 1.100.0`, GCC 14.2), 64-bit output, alternating A/B runs, minimum of 12
rounds per configuration:

Same optimization level, generic codegen (C++: `g++ -O3`; Rust:
`-C opt-level=3`):

| input bytes | C++ ns/op | Rust ns/op | ratio |
| ----------- | --------- | ---------- | ----- |
| 8           | 11.23     | 11.27      | 1.00x |
| 16          | 14.47     | 14.44      | 1.00x |
| 32          | 20.59     | 20.34      | 1.01x |
| 64          | 32.52     | 32.16      | 1.01x |
| 256         | 103.73    | 103.77     | 1.00x |
| 1024        | 389.40    | 388.36     | 1.00x |
| 4096        | 1524.60   | 1523.62    | 1.00x |
| 65536       | 24401.3   | 24371.4    | 1.00x |

Same flags, host-tuned codegen (C++: `g++ -O3 -march=native`; Rust:
`-C target-cpu=native -C opt-level=3`):

| input bytes | C++ ns/op | Rust ns/op | ratio |
| ----------- | --------- | ---------- | ----- |
| 8           | 12.32     | 11.26      | 1.09x |
| 16          | 14.72     | 13.71      | 1.07x |
| 32          | 20.86     | 19.16      | 1.09x |
| 64          | 32.56     | 30.81      | 1.06x |
| 256         | 103.56    | 98.56      | 1.05x |
| 1024        | 387.81    | 369.47     | 1.05x |
| 4096        | 1526.31   | 1449.31    | 1.05x |
| 65536       | 24347.5   | 23127.2    | 1.05x |

With identical target flags the Rust port is 5-9% faster at every size,
including 4 KiB. With generic codegen the two are at parity at every size
(1.00x-1.01x). Comparing `g++ -O2` against `-C opt-level=3` is not a fair
fight and makes Rust look faster on short inputs than it is; the tables above
use the same optimization level on both sides.

The old long-input gap was a scheduling-model artifact, not an algorithmic
one. `GenericX86Model` in `X86Schedule.td` sets `CompleteModel = 0` and
provides no instruction schedules, so the machine scheduler had no latency
data for the rotate chain and emitted a schedule that stalled roughly two
cycles per 8-byte block on this core. The fix is to interleave the two
independent halves of the round in the source: `v0 += v1` is issued together
with `v2 += v3`, and likewise for the two rotates and the two XORs. That gives
the generic scheduler independent work without a target model, and the 4 KiB
ratio is now 1.00x. Selecting a real model (`-C target-cpu=native`, or a
specific `-C target-cpu=znver4`) still makes Rust 5-9% faster. Measured and
rejected alternatives, none of which help: enabling BMI2 alone (`rorx` with
the generic model), disabling the machine scheduler, forcing source-order
scheduling, `-pre-RA-sched=list-ilp` (partial, 0.90x), manual two-block
unrolling, a rolled inner loop, and `-O2` instead of `-O3`.

Because `-C target-cpu=native` emits code for the build host, it is not a safe
default for binaries that ship to other machines. `LLVM_RUST_FLAGS` exposes
the knob:

```sh
cmake -S llvm -B build -DLLVM_ENABLE_RUST_SIPHASH=ON \
      -DLLVM_RUST_FLAGS=-Ctarget-cpu=native
```

Size: the stripped golden-test binary grows by 48 bytes (14,600 -> 14,648).
`libLLVMSupport.a` shrinks slightly (5,917,856 -> 5,914,912 bytes) because the
C++ SipHash body is replaced by a call into the Rust archive.

Build cost: the crate builds in well under a second; configurations with the
option `OFF` do not invoke cargo at all.

## Second port: CRC-32

Implementation: `llvm/lib/Support/rust/crc`, behind `LLVM_ENABLE_RUST_CRC`.
`llvm::crc32(uint32_t, ArrayRef<uint8_t>)` forwards to the crate; `JamCRC`
keeps its C++ code and inherits the speedup.

Correctness:

* Check value `0xCBF43926` for `"123456789"`.
* 830 differential cases (lengths 0..700 with random seeds, plus every
  incremental split of a 128-byte buffer) are bit-identical to the C++ code.
* The golden test over `llvm::crc32` and `JamCRC` passes with either backend,
  and the two binaries print identical values.
* `cargo test` covers the same properties against a bit-at-a-time oracle.

Performance. `llvm-crc-differential` prints this table; numbers below are from
the CMake build on the same machine (AMD EPYC 9B14, GCC 14.2 at `-O3`,
`rustc 1.100.0`), ns per call:

| bytes | C++ byte-wise | C++ slicing-by-8 | Rust | vs byte-wise | vs slicing-by-8 |
| ----- | ------------- | ---------------- | ---- | ------------ | --------------- |
| 16    | 41.0          | 8.3              | 7.5  | 5.45x        | 1.10x           |
| 64    | 151.4         | 31.6             | 24.3 | 6.24x        | 1.30x           |
| 256   | 604.3         | 124.3            | 93.7 | 6.45x        | 1.33x           |
| 1024  | 2414.9        | 494.9            | 372.8| 6.48x        | 1.33x           |
| 4096  | 9651.8        | 2028.8           | 1504.3| 6.42x       | 1.35x           |
| 65536 | 154289.8      | 32459.9          | 24804.7| 6.22x      | 1.31x           |

With matched target flags (`g++ -O3 -march=native` and
`-C target-cpu=native`) the picture holds: 5.4-6.7x over the byte-wise loop and
1.09-1.26x over the same slicing-by-8 algorithm.

The C++ implementation LLVM ships is byte-at-a-time, so most of the win comes
from the algorithm: 32 bytes per iteration with 32 slicing tables, then 16, 8,
4 and 1 byte steps. Tables are generated at compile time by a `const fn`, so
there is no runtime initialization and no build script.

One codegen trap is worth recording. With AVX2 enabled, LLVM's SLP vectorizer
turns the eight independent table lookups into `vpgatherdd`, which is much
slower than eight scalar loads; enabling `-C target-cpu=native` made the Rust
port 3x slower than the generic build. Reading the tables through
`read_volatile` blocks that transformation and is neutral on targets without
gathers. The same applies to any table-driven code in future crates: verify the
native build, not just the generic one.

## Third port: MD5

Implementation: `llvm/lib/Support/rust/md5`, behind `LLVM_ENABLE_RUST_MD5`.
Only the compression function (`MD5::body`) is replaced; the buffering,
padding and `MD5Result` handling stay in C++, so the class layout and the
ABI are untouched. The crate also exports a one-shot `llvm_md5_hash` that the
tests use.

Correctness:

* RFC 1321 vectors and 701 differential cases (lengths 0..700 over random
  data) are bit-identical to the C++ implementation.
* The golden test over `llvm::MD5::hash`, `MD5Result::digest`, `low()` and
  `high()` passes with either backend and prints identical values.
* `cargo test` checks every length from 0 to 4095 against a loop-based
  RFC 1321 oracle, plus the raw block interface.

Performance, ns per call, default path on this Zen 4 host (the BMI1/BMI2
body is selected by CPUID; the generic body is 0.99x-1.07x):

| bytes | C++    | Rust    | ratio |
| ----- | ------ | ------- | ----- |
| 16    | 111.37 | 100.98  | 1.10x |
| 64    | 207.86 | 185.19  | 1.12x |
| 256   | 489.88 | 455.10  | 1.08x |
| 1024  | 1619.78| 1534.31 | 1.06x |
| 4096  | 6135.86| 5852.95 | 1.05x |
| 65536 | 96489.4| 92283.2 | 1.05x |

With matched target flags (`g++ -O3 -march=native` and
`-C target-cpu=native`) Rust is 1.06-1.13x faster.

The first generic build was slower (0.83x). The cause is visible in the
instruction mix: each MD5 step is `a + f(b,c,d) + m + t`, and GCC fuses those
additions into LEA (82 `add` + 116 `lea` per block), while LLVM's generic x86
model emitted plain adds (232 `add`) and needed a `mov` in front of every
`rol`. The crate now computes `a + m + t` with an explicit LEA `asm!` block,
which removes the generic-target gap, and adds a
`#[target_feature(enable = "bmi1,bmi2")]` body selected by CPUID so `rorx`
and `andn` are used when the CPU has them. The generic body is now
0.99x-1.07x (parity to slightly faster); the dispatched body is 1.05x-1.12x
on this host and 1.03x-1.13x with matched native flags. The `asm!` path is
x86-64 only; other targets use the pure-Rust grouping, and the crate tests
compare the generic and BMI bodies bit for bit.

The port no longer misses the 5% performance gate. It remains behind the
default-off `LLVM_ENABLE_RUST_MD5` option like every other component, but it
does not need a tuned `LLVM_RUST_FLAGS`.

## Fourth port: SHA-1

Implementation: `llvm/lib/Support/rust/sha1`, behind `LLVM_ENABLE_RUST_SHA1`.
`SHA1::hashBlock()` forwards to `llvm_sha1_block`; the bulk path in `update()`
forwards whole blocks to `llvm_sha1_blocks`. Buffering, padding and the public
class layout stay in C++, so the ABI is unchanged.

Correctness:

* FIPS 180-4 vectors, including the one-million-`a` message.
* `cargo test` checks every length from 0 to 4095 against a loop-based
  reference, and the SHA-NI path against the scalar path on 2000 random
  blocks.
* The differential test checks 3 published vectors and 701 lengths against the
  C++ compression function copied verbatim from `SHA1.cpp`.
* The golden test over `llvm::SHA1::hash` passes with either backend and also
  checks that `update()` in odd-sized chunks matches the one-shot hash.

Performance, ns per call, generic codegen (`g++ -O3` vs `-C opt-level=3`, no
`-march=native`, AMD EPYC 9B14, rustc 1.102.0-nightly):

| bytes | C++ | Rust scalar | Rust auto | scalar vs C++ | auto vs C++ |
| ----- | --- | ----------- | --------- | ------------- | ----------- |
| 16    | 104.5 | 99.9      | 59.6      | 1.05x         | 1.75x       |
| 64    | 190.8 | 190.4     | 104.7     | 1.00x         | 1.82x       |
| 256   | 440.2 | 452.4     | 238.6     | 0.97x         | 1.84x       |
| 1024  | 1437.4 | 1502.2   | 774.9     | 0.96x         | 1.86x       |
| 4096  | 5438.9 | 5669.4   | 2919.5    | 0.96x         | 1.86x       |
| 65536 | 85607.0 | 88710.2 | 45774.1   | 0.97x         | 1.87x       |

The scalar port is at parity up to 64 bytes and about 4% behind on long inputs,
inside the 5% gate. The default (auto) path is 1.75-1.87x faster because it
dispatches at runtime to the x86 SHA extensions.

## Fifth port: SHA-256

Implementation: `llvm/lib/Support/rust/sha256`, behind `LLVM_ENABLE_RUST_SHA256`.
Only the compression function is replaced, using the same 16-word rolling
schedule and variable rotation as `SHA256.cpp`; buffering and padding stay in
C++.

Correctness: the same protocol as SHA-1 (FIPS 180-4 vectors, every length from
0 to 4095 against an independent oracle, 701 differential lengths against the
C++ code, golden values through `llvm::SHA256::hash`, incremental chunks, and
2000 random blocks comparing SHA-NI with scalar).

Performance, same machine and flags:

| bytes | C++ | Rust scalar | Rust auto | scalar vs C++ | auto vs C++ |
| ----- | --- | ----------- | --------- | ------------- | ----------- |
| 16    | 229.4 | 202.5     | 62.5      | 1.13x         | 3.67x       |
| 64    | 423.1 | 393.8     | 109.5     | 1.07x         | 3.86x       |
| 256   | 1003.6 | 962.5    | 250.2     | 1.04x         | 4.01x       |
| 1024  | 3327.3 | 3224.1   | 812.3     | 1.03x         | 4.10x       |
| 4096  | 12586.6 | 12306.0 | 3062.6    | 1.02x         | 4.11x       |
| 65536 | 198934.3 | 194453.7 | 48043.9 | 1.02x         | 4.14x       |

Unlike MD5, the scalar SHA-256 port already beats the C++ baseline on generic
codegen (2-13%). The default path is 3.7-4.1x faster.

### Runtime SHA-NI dispatch

Both crates contain a second compression function written with the x86 SHA
intrinsics (`sha256rnds2`, `sha256msg1/2`, `sha1rnds4`, `sha1nexte`,
`sha1msg1/2`). It is compiled with `#[target_feature(enable = "sha,sse2,ssse3,
sse4.1")]` and is only called after a CPUID check (SSSE3 and SSE4.1 in leaf 1,
SHA in leaf 7). The result is cached in a relaxed `AtomicU8`; a CPU without
SHA-NI runs the scalar code, so the archive stays usable on baseline x86-64 and
on other architectures.

This is the first place where the Rust port wins for a reason other than
instruction selection: the win comes from dispatching to hardware that the C++
baseline never uses. `llvm_sha{1,256}_hash_scalar` and
`llvm_sha{1,256}_has_sha_ni` are exported so the differential test can report
the scalar and accelerated numbers separately and state whether the host has
SHA-NI.

## Codegen investigation: the original MD5 generic regression

The original MD5 generic regression is not specific to Rust. It is the
difference between GCC and LLVM for the same algorithm on baseline x86-64, and
it is visible pass by pass. The crate now avoids it with the explicit LEA
described above; this section records the underlying difference and the
opt-in `X86FixupLEAs` fold that came out of the investigation.

Dumping the machine pipeline of the MD5 crate (`-C llvm-args=-print-after-all`)
and following `body_raw` gives:

| target | after ISel | after two-address | final asm |
| ------ | ---------- | ----------------- | --------- |
| `generic`   | 736 (64 `rol`)  | 868 | 646 (29 `lea`, 231 `add`, 64 `rol`) |
| `x86-64-v3` | 736 (64 `rorx`) | 773 | 650 (19 `lea`, 241 `add`, 64 `rorx`) |
| `znver4`    | 736 (64 `rorx`) | 773 | 593 (65 `lea`, 131 `add`, 64 `rorx`) |

Three separate effects:

1. `rol` versus `rorx` is decided during instruction selection and needs BMI2.
   A two-operand `rol` forces a copy whenever the input is still live, which is
   why the two-address pass grows the function from 736 to 868 instructions on
   `generic` but only to 773 with `rorx`.
2. `x86-64-v3` and `znver4` produce byte-identical machine code after the
   two-address pass. They first differ in the machine scheduler and diverge from
   there through register allocation. `x86-fixup-leas` then rewrites 34 LEAs back
   into ADDs in the generic and `x86-64-v3` runs but only 2 in the `znver4` run.
   This is why `-C target-feature=+bmi2` alone does not help: it buys `rorx` but
   not the register allocation that keeps the LEAs.
3. GCC fuses `add(add(x, y), imm)` into `lea imm(x, y)` (82 `add` + 116 `lea`
   per MD5 block) while LLVM emits 231 plain adds and only 29 LEAs. That fusion
   accounts for most of the remaining ~10% instruction-count gap.

So there is no single flag to flip. The `generic` model needs LEA fusion for
arithmetic chains to match GCC; the `znver4` model already has it.

A targeted improvement would be a post-RA fold in `X86FixupLEAs`: when an
`ADD32ri`/`ADD64ri32` reads the result of a `LEA` with a zero displacement, the
register is dead after the add and EFLAGS is dead, merge the immediate into the
LEA displacement and delete the add. That is worth about 64 instructions per MD5
block on baseline x86-64. It is not implemented here: it changes shared codegen
for every x86 target, and this fork does not carry `llvm/test`, so there is no
CodeGen suite to check it against.

### Follow-up: the immediate-into-LEA fold

The proposal above was implemented and measured. On rustc 1.102.0-nightly,
`-C target-cpu=generic` reproduces the reported shape exactly: `body_raw`
contains 29 `lea`, 232 `add` and 64 `rol`. All 29 LEAs are immediately followed
by `add $imm, <same register>`:

    leal (%r8,%rdi), %r11d
    addl $-176418897, %r11d

The fold lives in `X86FixupLEAs` behind a hidden, default-off option
(`-x86-fold-add-into-lea`), so it cannot change default codegen. It rewrites
the pair into one `leal imm(%r8,%rdi), %r11d` when:

* the ADD is `ADD32ri{,8}` or `ADD64ri{32,8}` with a physical two-address
  destination, and the preceding non-debug instruction is a width-matched LEA
  that defines the same register;
* the LEA has a base or index, and displacement plus immediate is representable
  (`int32` for 64-bit LEAs, wrapping for 32-bit LEAs);
* the flags written by the ADD are dead: no instruction reads EFLAGS before
  one that redefines it, and reaching the end of the block counts as live.

The measured saving in CI is 27 ADDs (234 -> 207 across the whole crate IR)
with the LEA count unchanged at 30. Two of the 29 pairs stay because the scan
stops at the end of the block, where EFLAGS counts as live. That is about 4% of
`body_raw`, not the 64 implied by the earlier estimate. It does not close the
~10% gap to GCC,
because most GCC LEAs are not folds of an existing LEA: GCC forms LEA directly
from add trees, while LLVM only forms LEA for addressing modes. Closing that
gap needs a DAG combine that selects LEA for arithmetic add chains on targets
where a three-operand LEA is cheap; that is a much larger change than this
peephole.

This fork has no CodeGen test suite, so the fold is validated by the
`codegen-lea` CI job: it builds `llc`, emits the MD5 crate's IR, compiles it
with and without the flag, requires the ADD count to drop, links both objects
against a harness and requires identical digests, and runs a pair of MIR tests
that pin the carry-preserving INC case. It is opt-in
because it changes shared x86 codegen and because the Rust ports no longer need
it to win: the default SHA-NI path is 1.8-4.1x faster than the C++ baseline.


## Sixth port: ConvertUTF

Implementation: `llvm/lib/Support/rust/convertutf`, behind
`LLVM_ENABLE_RUST_CONVERTUTF`. This is the first whole-file port rather than a
single compression body. `ConvertUTF.cpp` keeps the public signatures and
forwards all twelve exported entry points to `llvm_convertutf_*`; the static
tables and the C++ fallback bodies are compiled only when the option is off, so
the Rust build does not carry dead C++ code. `ConvertUTFWrapper.cpp` is
unchanged and continues to call the same C++ API.

The Rust code is a direct translation of the Unicode 6.3 rules, including the
`sourceExhausted` / `targetExhausted` / `sourceIllegal` pointer positions and
the maximal-subpart replacement used by `ConvertUTF8toUTF32` in lenient mode.
On top of that it adds ASCII fast paths: `ConvertUTF8toUTF16` and
`ConvertUTF8toUTF32` write ASCII directly, the UTF-16/UTF-32 directions consume
a leading ASCII run before entering the scalar loop, and UTF-32 -> UTF-16
writes four ASCII units per 64-bit store. `isLegalUTF8String` scans ASCII eight
bytes at a time, which matters for JSON and command-line parsing.

`test/differential.cpp` compiles the original `ConvertUTF.cpp` (without the
option define) next to the Rust archive and compares result codes, source and
target offsets, and every output element for all target capacities, both
conversion flags, valid and malformed UTF-8, UTF-16 with unpaired surrogates,
and UTF-32 values above U+10FFFF. It also exhaustively compares the helper
functions over every one- and two-byte sequence, every three-byte sequence
starting with `E0`-`EF`, and a sampled set of four-byte sequences.
`test/stable.cpp` is the golden test that runs against `LLVMSupport` with the
option both on and off.

Measured on this Zen 4 host, `g++ -O3` against `-C opt-level=3`:

| Conversion              | ASCII          | Mixed          |
|-------------------------|----------------|----------------|
| UTF-8 -> UTF-16         | 3.4x - 4.5x    | 1.20x - 1.26x  |
| UTF-8 -> UTF-32         | 3.8x - 4.7x    | 1.27x - 1.37x  |
| UTF-16 -> UTF-8         | 1.38x - 1.50x  | 0.99x - 1.01x  |
| UTF-32 -> UTF-8         | 1.31x - 1.37x  | 1.04x - 1.09x  |
| UTF-16 -> UTF-32        | 1.29x - 1.49x  | 1.00x - 1.06x  |
| UTF-32 -> UTF-16        | 1.45x - 1.61x  | 1.00x - 1.01x  |
| `isLegalUTF8String`     | 2.3x at 16 B, 6x - 12x from 64 B | 1.5x - 2.2x |

The UTF-32 -> UTF-16 direction now has a guaranteed-capacity loop that skips
the per-code-point target checks when the caller provided two UTF-16 slots per
input code point, an out-of-line ASCII block that writes four UTF-16 units per
64-bit store, and a single 32-bit store for each surrogate pair. ASCII input is
1.45x-1.61x faster than C++; mixed BMP/supplementary input is at parity
(1.00-1.01x) from 256 units upward. The mixed column above gives two UTF-16
slots per code point. A second benchmark with an exact-size target (5 slots
per 4 code points) exercises the checked path: it is 0.94x-0.98x at 16-64
units and 0.99x-1.00x from 256 units upward. The only other sub-1.0 row is
UTF-16 -> UTF-8 mixed at 16 units (0.99x); these are small-input overhead, and
every large-input row is at or above parity. The helper functions are
bit-identical to the C++ implementation; the exhaustive check covers the
`findMaximalSubpart` table directly.

## Seventh port: xxHash

`xxhash.cpp` exposes `llvm::xxh3_64bits` and `llvm::xxh3_128bits`. Both are
pure byte-buffer functions with a fixed 192-byte secret, so the crate can
implement them behind the same two `LLVM_ABI` entry points. The scalar paths
are a direct translation of the C++ code; the public 128-bit result is returned
as two `uint64_t` values through an out parameter, which avoids relying on the
platform ABI for a two-field struct return.

On x86-64 the long-input path (`len > 240`) has an AVX2 implementation selected
at runtime. It processes four 64-bit accumulators per iteration, using
`_mm256_mul_epu32` for the 32x32->64 product and a shifted multiply for the
scramble step. The C++ implementation only has a scalar x86-64 path and a NEON
path, so this is the source of the large-input win. CPUID is serializing and
costs microseconds in virtualized guests; the first version called it on every
hash and lost badly at 768-4096 bytes. The crate now caches the result in a
relaxed `AtomicU8`, so the check is paid once per process.

On little-endian AArch64 the crate mirrors the C++ NEON loops with
`uint64x2_t` accumulate and scramble functions; NEON is baseline on AArch64, so
the path is selected without a runtime check. The crate cross-compiles for
`aarch64-unknown-linux-gnu` and the `neon_matches_scalar` test checks it
against the scalar path on AArch64. No ARM hardware was available here, so the
throughput table below is x86-64 only; the NEON code is a direct translation
of the already-tested C++ NEON loop rather than an unmeasured scalar fallback.

`test/differential.cpp` compiles `xxhash.cpp` as a separate translation unit
without `LLVM_RUST_XXHASH`, so the reference is always the C++ code. It checks
every length from 0 to 2048, 20,000 random lengths, and 4 KiB, 64 KiB, and
1 MiB inputs for both variants. It also calls the explicit `*_scalar` Rust
entry points, so the scalar fallback is checked on hosts where AVX2 or NEON
dispatch is active. `test/stable.cpp` checks published vectors and a
deterministic 768/1024/4096-byte pattern through the public API, so it runs
with the Rust option both on and off.

Measured on this Zen 4 host, `g++ -O3` against `-C opt-level=3`, generic
x86-64 target, AVX2 detected at runtime:

| Bytes  | 64-bit | 128-bit |
|--------|--------|---------|
| 12     | 0.99x  | 1.04x   |
| 48     | 1.06x  | 1.19x   |
| 192    | 1.17x  | 1.18x   |
| 768    | 2.54x  | 2.28x   |
| 4096   | 3.46x  | 3.28x   |
| 65536  | 3.72x  | 3.74x   |
| 1 MiB  | 3.53x  | 3.50x   |

The 12-byte 64-bit row is parity (0.99x); from 48 bytes upward Rust is faster,
and the AVX2 long path is 3.5x faster at 1 MiB. The 128-bit long path uses the
same AVX2 accumulate/scramble loops and gets the same win.

A local configure with only `LLVM_ENABLE_RUST_XXHASH=ON` produced
`LLVM_SYSTEM_LIBS = "-lrt -ldl -lm -lllvm_xxhash"`, so `llvm-config
--system-libs` carries the companion archive. The
`llvm-rust-xxhash-differential` CMake target built and ran, and the
`llvm-xxhash-stable` object compiled; `LLVMSupport` itself was not built. The
AArch64 differential was cross-compiled and run under `qemu-aarch64
--check-only`, where it matched the C++ NEON path bit for bit. The CI
`aarch64` job runs the crate tests, including `neon_matches_scalar`, under
`qemu-aarch64`.

## Eighth port: ConvertEBCDIC

`ConvertEBCDIC.cpp` exposes two byte-buffer functions:
`ConverterEBCDIC::convertToEBCDIC` and `convertToUTF8`. The Rust crate keeps
the same `SmallVectorImpl<char>`-owned output contract: the C++ wrapper sizes
the vector to the maximum possible output, the crate writes through the
`(pointer, capacity)` pair, and the wrapper shrinks to the returned length.
The crate reports both a status and the number of bytes written, so failed
UTF-8 input leaves the same partial output as the C++ loop.

The hot path assumes the wrapper's maximum-size allocation and drops the
per-byte capacity check. The EBCDIC-to-UTF-8 path always writes two bytes and
advances by one or two, which removes the output branch; the UTF-8-to-EBCDIC
path keeps the C++ validation rules for `C2`/`C3` sequences and rejects
truncated or malformed input with the same `std::errc` mapping.

`test/differential.cpp` compares both directions against the original C++
implementation compiled under `ConverterEBCDICCpp`, over golden inputs and
100,000 random byte strings. `test/stable.cpp` checks the public wrapper with
the option both on and off. The standalone test targets carry a tiny
`SmallVector` growth stub and do not link `LLVMSupport`, so the port can be
checked without a full support build. Measured on this Zen 4 host with
`g++ -O3` and `-C opt-level=3` on generic x86-64, best of five rounds, on
16-64-byte payloads:

| Direction       | Rust vs C++ |
|-----------------|-------------|
| EBCDIC -> UTF-8 | 1.11x         |
| UTF-8 -> EBCDIC | 1.16-1.22x    |

The option is `LLVM_ENABLE_RUST_CONVERTEBCDIC` (default `OFF`). A local
configure adds `llvm_convertebcdic` to `LLVM_SYSTEM_LIBS`; the standalone
tests build without `LLVMSupport`, so no large LLVM target was rebuilt for the
measurement.

## Risks and mitigations

| Risk | Mitigation |
| ---- | ---------- |
| Toolchain availability on buildbots | Option defaults to `OFF`; Rust jobs are a separate CI leg |
| Behaviour drift between backends | Differential and golden tests run in CI with the option `ON` |
| Panic across FFI | `panic = "abort"`, aborting panic handler, no unwinding |
| Supply-chain exposure | No crates.io dependencies; vendored deps reviewed as LLVM code |
| Debuggability of Rust code | Ship debug info in Release builds; verify in phase 1 |
| Performance regressions | Per-component benchmark gate with a 5% threshold |
| Maintainer burden | Only accept ports of components that are stable and well tested |
| Cross-compilation | Not supported yet; must be solved before phase 3 |
| Downstream static linking | Rust archives are in `LLVM_SYSTEM_LIBS`; `llvm-config --system-libs` emits them |

## Rollback

Each component has its own `LLVM_ENABLE_RUST_<COMPONENT>` switch. Turning it off
restores the C++ implementation with no code changes and no ABI change, which
keeps every step of the migration reversible.

## Next steps

1. Keep the SHA-NI dispatch as the default for SHA-1 and SHA-256. The Phase 1
   candidate list (CRC, MD5, SHA-1, SHA-256, Base64, ConvertUTF, ConvertEBCDIC,
   xxHash) is complete. Phase 2 now covers the Rust and D demanglers; evaluate the
   Itanium and Microsoft demanglers next.
2. Decide whether `-x86-fold-add-into-lea` should become default on targets
   where a three-operand LEA is cheap. That requires a CodeGen test suite and
   wider benchmarking; the peephole is opt-in until then.
3. If the generic-codegen gap matters for non-SHA components, implement LEA
   formation for arithmetic add chains in the DAG combiner rather than
   extending the peephole.
4. Extend `AddRustLibrary.cmake` with a `--target` mapping for cross builds.
   Runtime dispatch inside the crates already keeps the produced archives
   usable across x86-64 feature levels.
5. Keep `rust-toolchain.toml` on a pinned nightly and the CI legs in
   `.github/workflows/rust-support.yml` (lint, C++/Rust golden tests,
   differential checks, and the `codegen-lea` llc job).
6. Keep installing enabled Rust archives as companions and listing them in
   `llvm-config`. `LLVMDemangle` now owns `libllvm_rustdemangle.a` and
   `libllvm_dlangdemangle.a`; follow the same per-component install pattern for
   the remaining ports.
