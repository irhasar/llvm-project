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
   existing threading and allocator assumptions. The one exception is a
   write-once, relaxed atomic cache of the CPU feature check used to pick the
   SHA-NI path; it is idempotent and thread-safe.
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
`MD5.cpp` (`LLVM_ENABLE_RUST_MD5`), `SHA1.cpp` (`LLVM_ENABLE_RUST_SHA1`) and
`SHA256.cpp` (`LLVM_ENABLE_RUST_SHA256`). Remaining candidates: `Base64.cpp`
and `ConvertUTF.cpp`. Selection criteria: small, deterministic, byte-buffer in /
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

Performance, ns per call, generic codegen at the same optimization level
(`-O3` vs `-C opt-level=3`):

| bytes | C++   | Rust   | ratio |
| ----- | ----- | ------ | ----- |
| 16    | 112.1 | 123.8  | 0.90x |
| 64    | 206.6 | 231.9  | 0.89x |
| 256   | 488.2 | 572.2  | 0.85x |
| 1024  | 1619  | 1932   | 0.84x |
| 4096  | 6148  | 7391   | 0.83x |
| 65536 | 96542 | 119146 | 0.81x |

With matched target flags (`g++ -O3 -march=native` and
`-C target-cpu=native`) the result flips: Rust is 1.06-1.13x faster.

This is the first component where the Rust port is slower by default, and the
cause is visible in the instruction mix. Each MD5 step is
`a + f(b,c,d) + m + t`, and GCC fuses those additions into LEA
(82 `add` + 116 `lea` per block), while LLVM's generic x86 model emits plain
adds (232 `add`) and needs a `mov` in front of every `rol`. Enabling BMI2 alone
produces `rorx` but still no LEA, so it does not help; only a real scheduling
model makes LLVM use both `rorx` and LEA.

Because the port misses the 5% performance gate under generic codegen, MD5
stays opt-in and is documented as "needs a tuned `LLVM_RUST_FLAGS`". It is
kept in the tree as the control case for the experiment: it shows that the
gap is a codegen-model issue rather than anything specific to the algorithm.

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

## Codegen investigation: why the generic x86-64 build loses

The MD5 regression is not specific to Rust. It is the difference between GCC and
LLVM for the same algorithm on baseline x86-64, and it is visible pass by pass.
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
| Downstream static linking | In-tree tools link; installed `LLVMSupport` still needs the Rust archives added |

## Rollback

Each component has its own `LLVM_ENABLE_RUST_<COMPONENT>` switch. Turning it off
restores the C++ implementation with no code changes and no ABI change, which
keeps every step of the migration reversible.

## Next steps

1. Keep the SHA-NI dispatch as the default for SHA-1 and SHA-256. Port
   `Base64.cpp` and `ConvertUTF.cpp` next; both are byte-buffer in/out and fit
   the existing crate conventions.
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
6. Merge the enabled Rust archives into the installed `LLVMSupport`, or
   install them as companions and list them in `llvm-config`, so downstream
   static links keep working outside the build tree.
