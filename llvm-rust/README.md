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

## Status: first vertical slice

The M1 spike is implemented and differential-tested against LLVM 19.1.7.

- `support`: arena, error, diagnostics.
- `ir`: textual parser, printer, verifier; module/globals/functions; integer,
  floating-point, pointer, array, vector, struct, and function types;
  binary/unary, `icmp`/`fcmp`, `alloca`, `load`, `store`, `getelementptr`,
  `call`, `phi`, `select`, casts, `ret`, `br`, `condbr`, `switch`,
  `unreachable`.
- `bitcode`: LLVM bitstream writer and reader covering the module, type,
  constants, function, value-symbol-table, string-table, and
  `PARAMATTR`/`PARAMATTR_GROUP` blocks. The reader handles abbreviations and
  block-info abbreviations emitted by LLVM 19.
- `bitcode` attributes: return, parameter, function, and call-site attributes;
  `#N` group references; common enum attributes (`noundef`, `signext`,
  `zeroext`, `nonnull`, `noalias`, `inreg`, `nounwind`, `noreturn`,
  `willreturn`, `nofree`, `nosync`, `norecurse`, `cold`, `hot`, `optnone`,
  `alwaysinline`, `noinline`, `inlinehint`, and the rest of the legacy enum
  table); integer attributes (`align`, `dereferenceable`,
  `dereferenceable_or_null`); string attributes such as
  `"frame-pointer"="all"`; and the legacy memory attributes (`readnone`,
  `readonly`, `writeonly`). LLVM 19 stores the memory attributes as
  `ATTR_KIND_MEMORY`; the reader maps the three legacy values back to their
  textual spellings.
- `tools`: `llvm-as-rust`, `llvm-dis-rust`, and `llvm-rust-bench`.

This slice deliberately does not cover metadata, type attributes (`byval`,
`sret`, `elementtype`, `byref`, `inalloca`), parameter-indexed call-site
attributes, `allocsize`/`stackalign`/`vscale_range`-style integer attributes,
comdats, debug info, constant expressions, atomics, `invoke`/`landingpad`,
aggregate/vector instructions, or the exact `fp128`/`x86_fp80` payload. MC,
object, X86, and codegen are still out of scope.

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

Differential check against the C++ oracle (uses an installed LLVM, never
builds LLVM):

```sh
./tests/differential.sh
# or: LLVM_BIN=/path/to/llvm/bin ./tests/differential.sh
```

Reproducible benchmark (5 runs, `taskset -c 0`, median and min/max):

```sh
CARGO_TARGET_DIR=/tmp/llvm-rust-target cargo build --release \
  -p llvm-rust-tools --bin llvm-as-rust --bin llvm-dis-rust
python3 tests/bench.py --runs 5
```

## First-slice results

Differential harness: all seven modules in `tests/corpus/` match the C++ LLVM
19.1.7 oracle in both directions after normalizing `source_filename` and
`ModuleID`.

- Rust writer output is accepted by C++ `llvm-dis`.
- Rust reader output is accepted by C++ `llvm-as`, and C++ `llvm-dis` of that
  result matches the oracle byte-for-byte (after the same normalization).

Benchmark on a synthetic corpus of 1000 functions, 10 instructions each
(289,890 bytes), LLVM 19.1.7, `taskset -c 0`, five runs:

| Tool | Median | Min-max | Peak RSS |
| --- | ---: | ---: | ---: |
| `llvm-as` (C++) | 32.2 ms | 32.1-33.2 ms | 55.4 MiB |
| `llvm-as-rust` | 17.6 ms | 17.4-18.8 ms | 14.6 MiB |
| `llvm-dis` (C++) on Rust bitcode | 28.9 ms | 28.5-29.6 ms | 54.7 MiB |
| `llvm-dis-rust` on C++ bitcode | 16.9 ms | 16.8-17.6 ms | 14.6 MiB |

The Rust tools are about 1.8x faster and use about 3.8x less peak memory on
this corpus. The Rust binaries are 4.3 MiB (`llvm-as-rust`) and 3.6 MiB
(`llvm-dis-rust`); the C++ tools need the 124 MiB `libLLVM.so.19.1`.

These numbers are a slice result, not an LLVM parity claim. The upstream
`llvm/test` suite is still required before any parity gate, and the synthetic
corpus does not exercise the unsupported features listed above.
