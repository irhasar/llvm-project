# llvm-rust

Track B workspace for a Rust-native LLVM core. This directory is not built by
the default LLVM CMake build and is not wired into `LLVM_ENABLE_RUST_*`.

Read `../RUST_LLVM_TRACK_B.md` and `../AGENTS.md` before adding code.

## First slice

1. Add a Cargo workspace and a `support` crate with arena, error, and
   diagnostic primitives.
2. Add an `ir` crate that parses, represents, and prints a small textual IR
   module.
3. Add a `bitcode` reader that round-trips the same corpus.
4. Add a differential harness against C++ `llvm-as`/`llvm-dis`.
5. Record build and runtime metrics, then hold the M1 feasibility review.

Do not start with MC, CodeGen, or the target backends.

## Planned layout

```text
crates/
  support/
  ir/
  bitcode/
  mc/
  object/
  x86/
  tools/
tests/
```

## Commands

```sh
cargo fmt --check
cargo test --workspace
cargo build --release --workspace
```

Use a separate target directory when comparing build profiles:

```sh
CARGO_TARGET_DIR=/tmp/llvm-rust-target cargo build --release
```
