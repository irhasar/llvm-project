# Incremental migration of LLVM components to Rust

This document describes an experiment in porting LLVM components to Rust one at
a time, without a flag day and without making the Rust toolchain a hard
dependency of the build. The first slice, `llvm/lib/Support/SipHash.cpp`, is
implemented and verified; everything else here is the plan for the next phases.

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
   existing threading and allocator assumptions.
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

Done: `CRC.cpp` (`LLVM_ENABLE_RUST_CRC`, 6x faster than the C++ fallback).
Remaining candidates: `MD5.cpp`, `SHA1.cpp`, `SHA256.cpp`, `Base64.cpp`,
`ConvertUTF.cpp`. Selection criteria: small, deterministic, byte-buffer in /
byte-buffer out, no LLVM types in the interface, existing unit tests.

The point of this phase is volume: prove that the plumbing works for many small
crates, and settle conventions (error reporting via return codes, output buffer
sizes, debug info) before touching anything structural.

### Phase 2 - demanglers

`llvm/lib/Demangle` is a string-to-string component with no LLVM data
structures and a clean ownership story: the caller passes a buffer and a
length. It is large enough to be a real test of the workflow and isolated
enough to be safe.

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
| 8           | 11.5      | 11.7       | 0.98x |
| 16          | 14.8      | 14.7       | 1.00x |
| 32          | 20.3      | 20.6       | 0.99x |
| 64          | 32.7      | 32.7       | 1.00x |
| 256         | 103.4     | 103.3      | 1.00x |
| 1024        | 386.1     | 386.2      | 1.00x |
| 4096        | 1519.9    | 1667.3     | 0.91x |

Same flags, host-tuned codegen (C++: `g++ -O3 -march=native`; Rust:
`-C target-cpu=native -C opt-level=3`):

| input bytes | C++ ns/op | Rust ns/op | ratio |
| ----------- | --------- | ---------- | ----- |
| 8           | 11.5      | 10.9       | 1.06x |
| 16          | 14.7      | 13.5       | 1.09x |
| 32          | 20.3      | 19.0       | 1.07x |
| 64          | 32.2      | 30.3       | 1.06x |
| 256         | 102.8     | 97.5       | 1.05x |
| 1024        | 386.2     | 367.0      | 1.05x |
| 4096        | 1515.1    | 1438.2     | 1.05x |

With identical target flags the Rust port is 5-9% faster at every size,
including 4 KiB. With generic codegen the two are at parity up to 1 KiB, where
most `getStableSipHash` calls live, and Rust is 9% slower at 4 KiB. Comparing
`g++ -O2` against `-C opt-level=3` is not a fair fight and makes Rust look
faster on short inputs than it is; the tables above use the same optimization
level on both sides.

The long-input gap is a scheduling-model artifact, not an algorithmic one.
`GenericX86Model` in `X86Schedule.td` sets `CompleteModel = 0` and provides no
instruction schedules, so the machine scheduler has no latency data for the
rotate chain and emits a schedule that stalls roughly two cycles per 8-byte
block on this core. Selecting a real model (`-C target-cpu=native`, or a
specific `-C target-cpu=znver4`) removes the gap. Measured and rejected
alternatives, none of which help: enabling BMI2 alone (`rorx` with the generic
model), disabling the machine scheduler, forcing source-order scheduling,
`-pre-RA-sched=list-ilp` (partial, 0.90x), manual two-block unrolling, a rolled
inner loop, and `-O2` instead of `-O3`.

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
the CMake build on the same machine (AMD EPYC 9B14, GCC 14.2 at `-O2`,
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

## Rollback

Each component has its own `LLVM_ENABLE_RUST_<COMPONENT>` switch. Turning it off
restores the C++ implementation with no code changes and no ABI change, which
keeps every step of the migration reversible.

## Next steps

1. Decide whether host-tuned Rust builds (`LLVM_RUST_FLAGS=-Ctarget-cpu=native`)
   are acceptable for release builds; the generic codegen path still loses
   about 9% on SipHash 4 KiB inputs because of the generic scheduling model.
2. Port the hash family (`MD5.cpp`, `SHA1.cpp`, `SHA256.cpp`) next; they are the
   largest remaining leaf functions in `lib/Support`.
3. Add a CI leg that builds `LLVMSupport` with `LLVM_ENABLE_RUST_SIPHASH=ON`
   and `LLVM_ENABLE_RUST_CRC=ON` and runs `check-llvm-rust-siphash` and
   `check-llvm-rust-crc`.
4. Extend `AddRustLibrary.cmake` with a `--target` mapping for cross builds.
