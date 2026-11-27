# AGENTS.md - Track B: Rust LLVM core

This branch is for **Track B only**. Track A, the C ABI staticlib ports into
the existing C++ LLVM, stays on `rust-migration-phase5`. Do not continue
Track A work here.

Read `RUST_LLVM_TRACK_B.md` first; it is the design, scope, and gate document.

## Scope

- Build a Rust-native LLVM core in `llvm-rust/`: support/ADT, IR, bitcode,
  MC, object, X86, a codegen subset, and the matching tools.
- Start with a vertical slice: textual IR parse/print plus bitcode round-trip.
- LLD, compiler-rt, libc, libunwind, and upstream Clang/LLDB are later or
  separate programs. Do not pull them into the first rewrite.
- Do not modify `llvm/lib`, `llvm/tools`, the Track A CMake integration, or any
  `LLVM_ENABLE_RUST_*` switch for Track B experiments. C++ LLVM is the oracle
  and bootstrap.
- Do not change the default LLVM build. Track B is side-by-side until its gates
  pass.

## Non-negotiable rules

- Pure Rust core: no C++ wrapper inside the Rust LLVM core.
- Rust-native data structures: arena plus indices, not intrusive C++ pointers.
- Errors use `Result`; panics are bugs. No unwinding across an external ABI.
- Isolate `unsafe` in small audited modules with tests.
- Stable Rust 1.100.0 by default. Nightly is an explicit experiment, never the
  default build.
- No crates.io dependency without review and vendoring. Start with no external
  dependencies.
- C++ LLVM is the differential oracle. Every parser, object reader, and codegen
  slice needs a C++-vs-Rust comparison.
- No parity claim without the upstream `llvm/test` suite. Restore or vendor it
  first; this fork does not carry `llvm/test` or `llvm/docs`.
- Keep Track A crates and C++ fallbacks untouched. Reuse leaf algorithms only;
  do not copy the Track A `no_std`/C-ABI architecture into the Rust core.
- Bootstrap is staged. C++ LLVM is stage 0; do not make the default `rustc`
  depend on Rust LLVM before stage 1 and stage 2 are reproducible.

## Workspace layout

```text
llvm-rust/
  Cargo.toml                 # workspace, added with the first crate
  README.md
  crates/
    support/                 # arena, errors, diagnostics, small ADTs
    ir/                      # module, types, constants, instructions, verifier
    bitcode/                 # reader/writer
    mc/                      # MC layer
    object/                  # object file reader/writer
    x86/                     # X86-specific MC/codegen
    tools/                   # llvm-as, llvm-dis, llc equivalents
  tests/                     # differential and lit-style tests
```

Do not add a crate until its first vertical slice has a test and a C++ oracle.

## First spike

1. Create the `llvm-rust/` Cargo workspace and a `support` crate with arena,
   error, and diagnostic primitives.
2. Add an `ir` crate that can represent a small module, parse textual IR, and
   print it back.
3. Add a `bitcode` reader that round-trips the same corpus.
4. Add a differential harness that runs C++ `llvm-as`/`llvm-dis` and the Rust
   tools on the same inputs.
5. Record compile time, peak RSS, binary size, and runtime for both sides.
6. Stop and review at the feasibility gate before starting MC or codegen.

## Build and test

Once the workspace exists:

```sh
cd llvm-rust
cargo fmt --check
cargo test --workspace
cargo build --release --workspace
```

Use a separate target directory per experiment when comparing profiles:

```sh
CARGO_TARGET_DIR=/tmp/llvm-rust-target cargo build --release
```

Do not build all of LLVM or run a full LLVM test suite for a Track B slice.
Build only the Rust workspace and the specific C++ oracle tool needed.

## Measurement policy

- Compare Rust against the same C++ LLVM revision and the same input corpus.
- Record compile wall/user/sys, peak RSS, artifact size, and runtime.
- Use `taskset -c 0`, warmup, and at least five runs; report median and min-max.
- Separate final binary size from intermediate archive or object size.
- A performance claim needs both throughput and generated-code quality.

## Performance playbook (M1 lessons)

Track B must be faster than C++ on the migrated surface without a large LLVM
build. Optimize only after measuring the component, and keep every change
correctness-gated by `cargo test --workspace` and `tests/differential.sh`.

- Measure components separately: parse, print, bitcode read, bitcode write,
  assemble, and disassemble. `llvm-rust-bench` has modes for each; process
  startup and file I/O must be reported separately from in-process timings.
- Keep source text borrowed. Lexer tokens use `Cow<'a, str>` and consumed
  names are moved out of tokens instead of cloned. Quoted or escaped names may
  remain owned; do not add per-token `String` allocations back into the hot
  path.
- Avoid per-record and per-instruction heap allocation. Small bitcode records
  and instruction operands stay inline; tune inline capacity with
  measurements. Four inline operands beat two and eight on the M1 corpus
  because the larger inline forms hurt cache/RSS.
- Use dense IDs as vector-backed tables with a sentinel value, not `HashMap`,
  for type, global, function, constant, and local-value maps. Reserve hash
  maps for sparse or non-index keys.
- Use a fast non-cryptographic hasher (FNV-1a) for compiler-internal maps.
  This is a deliberate compiler trade-off; if a map is exposed to adversarial
  input, revisit hash-flooding risk and document the trust boundary.
- Share immutable abbreviation tables across blocks with `Arc` and
  copy-on-write. Do not clone an abbreviation vector for every block.
- Decode function bodies by iterating records by reference. Do not clone an
  entire body just to satisfy borrow checking.
- Move decoded instruction kinds into the instruction and compute any derived
  flags first. Borrow function parameter lists and local constant records
  instead of cloning them. Skip unnamed-slot assignment when every parameter,
  local, and block already has a name.
- Cache interned primitive types and named-type lookups in `TypeTable`; do not
  build an `Rc<str>` just to look up an existing named type. Keep the caches
  lazy and covered by the interning tests.
- Vector-aware type checks must inspect the element type, not just the top
  level: floating-point vector binops decode as FP ops, vector comparisons
  return a vector of `i1`, and vector `select` conditions are vector `i1`.
  Keep textual, verifier, reader, and writer paths in agreement.
- LLVM sync-scope IDs are `SingleThread = 0` and `System = 1`; a default
  `fence` uses `System` and prints without a `syncscope(...)` prefix. Do not
  write zero for the default, and reject unknown scope IDs instead of
  silently changing their meaning.
- Atomic load/store syntax places `syncscope(...)` after the pointer and
  before the ordering, unlike `fence`, where it precedes the ordering.
  Atomic loads reject `release`/`acq_rel`; atomic stores reject
  `acquire`/`acq_rel`; both require an explicit alignment. Encode them with
  bitcode codes 41/45 and keep the non-atomic 20/44 paths unchanged.
- Consume parser tokens with `take` when the current token is always advanced.
  Do not clone `TokenKind` in type or constant parsing; `CStr` payloads are
  heap vectors and cloning them is avoidable.
- In the printer, write names and quoted byte strings directly into the output
  buffer. Do not allocate a temporary `String` per global/function reference,
  a `Vec` per call attribute partition, or a `format!` per escaped byte.
  Format decimal constants with a small integer formatter.
- Ordered ID lists have unique keys: preallocate the result and use
  `sort_unstable_by_key` instead of a stable sort.
- Short-lived CLI tools may skip arena teardown with `process::exit(0)` after
  flushing output. Never do this in library code or tests.
- Do not keep unhelpful preallocation or cache tweaks. Measure both throughput
  and RSS; revert changes that only add memory or do not move the median.
- For M1 performance claims use `taskset -c 0`, 11 runs after 3 warmups, the
  same corpus for both sides, and the same C++-produced bitcode for both
  disassemblers. Report median, min-max, and peak RSS.
- For M1 parser/printer/bitcode, report throughput and memory only and label
  them as such. A full performance claim still needs generated-code quality
  once codegen exists; never turn M1 numbers into an LLVM parity claim.

### M1 performance baseline (2026-11-26)

Corpus: 1000 functions, 664,987 bytes, LLVM 19.1.7, `taskset -c 0`, 11 runs
after 3 warmups.

| Tool | Median | Min-max | Peak RSS |
| --- | ---: | ---: | ---: |
| `llvm-as` (C++) | 64.7 ms | 62.4-74.4 ms | 60.6 MiB |
| `llvm-as-rust` | 33.5 ms | 33.0-35.5 ms | 26.0 MiB |
| `llvm-dis` (C++) on C++ bitcode | 53.0 ms | 51.3-74.2 ms | 58.1 MiB |
| `llvm-dis-rust` on C++ bitcode | 26.1 ms | 25.9-27.5 ms | 19.2 MiB |

In-process component medians: parse 11.18 ms, print 3.59 ms, bitcode write
3.62 ms, bitcode read 8.51 ms, assemble 15.62 ms, disassemble 12.00 ms, verify
11.62 ms. The optimization audit reduced parse from 8.48 ms, read from 6.66 ms,
print from 2.35 ms, and disassemble from 8.98 ms on the earlier corpus; the
current corpus adds vector operations and atomic load/store, so its absolute
component times are larger. Regression target: do not
regress these medians beyond noise; investigate any consistent slowdown before
adding features.

Before merging a performance change: capture the component baseline, make the
change, run `cargo test --workspace` and `tests/differential.sh`, rerun the
11-run/3-warmup benchmark, compare median and peak RSS, and keep the change
only when correctness passes and the hot component improves.

## Do not

- Do not start with CodeGen, SelectionDAG, GlobalISel, or the target backends.
- Do not rewrite Clang, LLDB, LLD, compiler-rt, libc, or libunwind here.
- Do not line-by-line translate C++ templates or intrusive lists.
- Do not add a C++ facade or wrapper inside the core.
- Do not claim LLVM parity from a subset of tests.
- Do not make Track B changes on `rust-migration-phase5`.

## References

- `RUST_LLVM_TRACK_B.md` - detailed Track B plan, milestones, and gates.
- `MIGRATION_TO_RUST.md` - Track A history and the long-term Track B summary.
- `llvm-rust/README.md` - workspace bootstrap notes.
