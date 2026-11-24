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

### Phase 1 - pure leaf functions in lib/Support

Candidates: `CRC.cpp`, `MD5.cpp`, `SHA1.cpp`, `SHA256.cpp`, `Base64.cpp`,
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

Same flags, generic codegen (C++: `g++ -O2`; Rust: `-C opt-level=3`):

| input bytes | C++ ns/op | Rust ns/op | ratio |
| ----------- | --------- | ---------- | ----- |
| 8           | 12.7      | 11.8       | 1.08x |
| 16          | 15.9      | 15.0       | 1.06x |
| 32          | 22.2      | 21.6       | 1.03x |
| 64          | 32.8      | 34.3       | 0.96x |
| 256         | 98.0      | 110.8      | 0.89x |
| 1024        | 362.3     | 417.2      | 0.87x |
| 4096        | 1426.2    | 1725.9     | 0.83x |

Same flags, host-tuned codegen (C++: `g++ -O3 -march=native`; Rust:
`-C target-cpu=native -C opt-level=3`):

| input bytes | C++ ns/op | Rust ns/op | ratio |
| ----------- | --------- | ---------- | ----- |
| 64          | 33.1      | 31.3       | 1.06x |
| 256         | 106.0     | 100.9      | 1.05x |
| 1024        | 399.3     | 377.1      | 1.06x |
| 4096        | 1563.8    | 1481.9     | 1.06x |

With identical target flags the Rust port is consistently about 5-6% faster at
every size, including 4 KiB. With generic flags it wins on short inputs, where
most `getStableSipHash` calls live, but loses on long ones.

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
   10-17% on long inputs because of the generic scheduling model.
2. Port `CRC.cpp` as the second crate to exercise the conventions at small
   scale.
3. Add a CI leg that builds `LLVMSupport` with `LLVM_ENABLE_RUST_SIPHASH=ON` and
   runs `check-llvm-rust-siphash`.
4. Extend `AddRustLibrary.cmake` with a `--target` mapping for cross builds.
