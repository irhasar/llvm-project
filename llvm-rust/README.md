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
  floating-point, pointer, array, vector, struct, function, and opaque types;
  TLS models, sections, function alignment, wide integers, and per-kind
  floating-point hex literals; binary/unary, `icmp`/`fcmp`, `alloca`, `load`,
  `store`, `getelementptr`, `call`, `phi`, `select`, casts, `ret`, `br`,
  `condbr`, `switch`, `unreachable`. Unnamed values and blocks share one
  numbering sequence, and `phi` accepts forward and self references.
  `local_unnamed_addr` and `unnamed_addr` are distinct on globals and
  functions. Forward global/function references are resolved after parsing.
  Nonzero global and pointer address spaces are rejected explicitly instead
  of being dropped.
- `bitcode`: LLVM bitstream writer and reader covering the module, type,
  constants, function, value-symbol-table, string-table, and
  `PARAMATTR`/`PARAMATTR_GROUP` blocks. The reader handles abbreviations and
  block-info abbreviations emitted by LLVM 19.
- `bitcode` attributes: return, parameter, function, call-site, and
  argument-indexed call-site attributes; `#N` group references; common enum
  attributes (`noundef`, `signext`, `zeroext`, `nonnull`, `noalias`, `inreg`,
  `nounwind`, `noreturn`, `willreturn`, `nofree`, `nosync`, `norecurse`,
  `cold`, `hot`, `optnone`, `alwaysinline`, `noinline`, `inlinehint`, and the
  rest of the legacy enum table); integer attributes (`align`, `alignstack`,
  `dereferenceable`, `dereferenceable_or_null`); type attributes (`byval`,
  `byref`, `sret`, `inalloca`, `preallocated`, `elementtype`); string
  attributes such as `"frame-pointer"="all"`; and `memory(none|read|write)`
  plus the legacy `readnone`/`readonly`/`writeonly` spellings. Type attributes
  are parsed as real types, so quoted names such as `byval(%"a b")` and
  type-only arguments such as `byval([8 x i8])` resolve correctly. Group-only
  spellings such as `alignstack=16` are accepted and written back inline. The
  reader canonicalizes the legacy memory enum values to
  `memory(none|read|write)`.
- `tools`: `llvm-as-rust`, `llvm-dis-rust`, and `llvm-rust-bench`.

Half and bfloat conversion handles subnormals, infinities, quiet NaNs, and
signaling NaNs with round-to-nearest-even; float printing preserves signaling
NaN payloads. Section, source filename, triple, data layout, and symbol names
must be valid UTF-8 in bitcode and are rejected otherwise instead of being
lossily rewritten.

This slice deliberately does not cover the metadata block (metadata strings
parse and print in text, but do not round-trip through bitcode),
`allocsize`/`stackalign`/`vscale_range`-style integer attributes, comdats,
debug info, constant expressions, atomics, `invoke`/`landingpad`,
aggregate/vector instructions, or `fp128`/`x86_fp80`/`ppc_fp128` constants
(wide float records are rejected). MC, object, X86, and codegen are still out
of scope.

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

Differential harness: all fifteen modules in `tests/corpus/` match the C++
LLVM 19.1.7 oracle in both directions after normalizing `source_filename` and
`ModuleID`. The corpus covers TLS models, sections, opaque types, `i128`
constants, hexadecimal floats and subnormal/signaling-NaN cases, function
alignment, type attributes with quoted names, argument-indexed call-site
attributes, `local_unnamed_addr`, forward references, UTF-8 section names,
unnamed value/block numbering, and `phi` self references.

- Rust writer output is accepted by C++ `llvm-dis`.
- Rust reader output is accepted by C++ `llvm-as`, and C++ `llvm-dis` of that
  result matches the oracle byte-for-byte (after the same normalization).

Benchmark on a synthetic corpus of 1000 functions, 10 instructions each
(289,890 bytes), LLVM 19.1.7, `taskset -c 0`, five runs:

| Tool | Median | Min-max | Peak RSS |
| --- | ---: | ---: | ---: |
| `llvm-as` (C++) | 35.8 ms | 35.1-43.7 ms | 55.7 MiB |
| `llvm-as-rust` | 22.7 ms | 21.4-25.3 ms | 14.6 MiB |
| `llvm-dis` (C++) on Rust bitcode | 30.3 ms | 29.9-31.3 ms | 54.7 MiB |
| `llvm-dis-rust` on C++ bitcode | 19.2 ms | 18.9-19.6 ms | 14.6 MiB |

The Rust tools are about 1.6x faster and use about 3.8x less peak memory on
this corpus. The Rust binaries are 4.5 MiB (`llvm-as-rust`) and 3.7 MiB
(`llvm-dis-rust`); the C++ tools need the 124 MiB `libLLVM.so.19.1`.

These numbers are a slice result, not an LLVM parity claim. The upstream
`llvm/test` suite is still required before any parity gate, and the synthetic
corpus does not exercise the unsupported features listed above.
