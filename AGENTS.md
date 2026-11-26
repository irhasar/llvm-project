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
