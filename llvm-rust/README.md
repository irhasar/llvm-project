# llvm-rust

Track B workspace for a Rust-native LLVM core. This directory is not built by
the default LLVM CMake build and is not wired into `LLVM_ENABLE_RUST_*`.

Read `../RUST_LLVM_TRACK_B.md` and `../AGENTS.md` before adding code.

## M1 scope

The M1 slice is implemented; it is not an empty workspace proposal. It
contains a Cargo workspace, the `support`, `ir`, `bitcode`, and `tools`
crates, and runnable `llvm-as-rust`/`llvm-dis-rust` binaries. The M1
feasibility review is still open because generated-code evidence and the
upstream test suite are not available yet.

The remaining M1 steps are:

1. Keep the textual IR parser/printer and bitcode reader/writer in
   differential agreement with the C++ oracle.
2. Expand the corpus until the unsupported-feature list is the only gap.
3. Record build and runtime metrics, then hold the M1 feasibility review.

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

## Status: M1 vertical slice

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
  functions. Forward global/function constants and instruction operands are
  resolved after parsing, and globals/functions are emitted in definition
  order. Empty and unquoted numeric symbol names are canonicalized to LLVM
  unnamed slots (`@0`, `@1`, ...); quoted numeric names such as `@"0"` stay
  named, and slots are renumbered in the printer's global-then-function
  emission order. Nonzero global and pointer address spaces are rejected
  explicitly instead of being dropped. Half and bfloat literals round
  directly from binary64 without narrowing through `f32`; tiny half inputs
  return signed zero instead of shifting by more than 63 bits.
- `bitcode`: LLVM bitstream writer and reader covering the module, type,
  constants, function, value-symbol-table, string-table, and
  `PARAMATTR`/`PARAMATTR_GROUP` blocks. The reader handles abbreviations and
  block-info abbreviations emitted by LLVM 19. The writer deduplicates
  attribute groups and lists. The reader interns a default pointer type when
  the on-disk type table has none, so direct calls do not require a pointer
  type record.
- `bitcode` attributes: return, parameter, function, call-site, and
  argument-indexed call-site attributes; `#N` group references; common enum
  attributes (`noundef`, `signext`, `zeroext`, `nonnull`, `noalias`, `inreg`,
  `nounwind`, `noreturn`, `willreturn`, `nofree`, `nosync`, `norecurse`,
  `cold`, `hot`, `optnone`, `alwaysinline`, `noinline`, `inlinehint`, and the
  rest of the legacy enum table); integer attributes (`align`, `alignstack`,
  `dereferenceable`, `dereferenceable_or_null`); type attributes (`byval`,
  `byref`, `sret`, `inalloca`, `preallocated`, `elementtype`); string
  attributes such as `"frame-pointer"="all"` with raw non-UTF-8 bytes; and
  `memory(none|read|write)` plus the legacy
  `readnone`/`readonly`/`writeonly` spellings. Type attributes are parsed as
  real types, so quoted names such as `byval(%"a b")` and type-only arguments
  such as `byval([8 x i8])` resolve correctly. Group-only spellings such as
  `align=16` and `alignstack=16` are preserved in printed groups and expanded
  for bitcode encoding. The reader canonicalizes the legacy memory enum values
  to `memory(none|read|write)` only at function scope; parameter and return
  attributes keep `readnone`/`readonly`/`writeonly`, which LLVM accepts there.
  Multiple function alignment groups are merged in order, with the last value
  winning and no `align` left in the function attribute list. Attribute
  groups and lists are shared through `Arc`, so declarations that reference
  the same list do not copy it; the memory budget is charged once for the
  stored group, not once per referencing declaration. Lists that repeat a
  slot are merged once with LLVM's last-wins rule for the same attribute kind
  or string key, and call-site attribute lists use the same shared
  representation. Legacy function memory flags are intersected within each
  inline or numbered-group source, then emitted as one modern `memory(...)`
  attribute keyed by the memory kind. Numbered groups apply after inline
  attributes, so a scoped group can override an inline flag. Modern memory
  uses LLVM 19's three-location encoding. An explicit errno clause is
  accepted only when it matches the final other-memory effect; target
  spellings are rejected. The reader folds errno into other memory and
  validates version-2 target fields against inaccessible memory on AArch64
  (other memory elsewhere), matching LLVM's downgrade path.
  Parameter and return `readnone`/`readonly`/`writeonly` remain distinct
  flags.
- `tools`: `llvm-as-rust`, `llvm-dis-rust`, and `llvm-rust-bench`.

Half and bfloat conversion handles subnormals, infinities, quiet NaNs, and
signaling NaNs with round-to-nearest-even; float printing preserves signaling
NaN payloads. `CST_DATA` records use float constants when their element type
is floating-point, and function addresses in operands and constants use
pointer type rather than the function signature. Section, source filename,
triple, data layout, and symbol names must be valid UTF-8 in bitcode and are
rejected otherwise instead of being lossily rewritten.

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

## M1 results

Differential harness: all 30 modules in `tests/corpus/` match the C++ LLVM
19.1.7 oracle in both directions after normalizing `source_filename` and
`ModuleID`. The corpus covers TLS models, sections, opaque types, `i128`
constants, hexadecimal floats and subnormal/signaling-NaN cases, float and
double `CST_DATA` arrays, function and multiple-group alignment, type
attributes with quoted names, raw non-UTF-8 attribute strings, parameter
memory attributes, argument-indexed call-site attributes,
`local_unnamed_addr`, forward references in constants and instructions,
UTF-8 section names, unnamed globals/functions, explicit quoted numeric
global/function names, unnamed/global slot ordering, unnamed/name collisions,
forward numbered references that must not consume definition slots, direct
calls without pointer type records, repeated function memory effects,
modern/legacy memory equivalence, inline legacy flags overridden by numbered
scoped-memory groups, modern `memory(argmem: ...)`/
`memory(inaccessiblemem: ...)` forms, duplicate attribute groups in a
checked-in bitcode fixture that is read and reassembled with both Rust tools,
unnamed value/block numbering, exact shift/divide flags, and `phi` self
references.

- Rust writer output is accepted by C++ `llvm-dis`.
- Rust reader output is accepted by C++ `llvm-as`, and C++ `llvm-dis` of that
  result matches the oracle byte-for-byte (after the same normalization).
- `tests/corpus/duplicate_attrs.bc` is a checked-in bitcode fixture with two
  groups for one parameter slot and a repeated group reference for a
  declaration. It is produced by the round-trip test when
  `LLVM_RUST_DUPLICATE_FIXTURE` is set; the differential harness reads it with
  both disassemblers and canonicalizes the Rust output through C++ `llvm-as`.

Benchmark on a synthetic corpus of 1000 functions, 10 arithmetic instructions
and one attributed call each (342,956 bytes), LLVM 19.1.7, `taskset -c 0`,
five runs:

| Tool | Median | Min-max | Peak RSS |
| --- | ---: | ---: | ---: |
| `llvm-as` (C++) | 37.2 ms | 36.2-37.4 ms | 55.7 MiB |
| `llvm-as-rust` | 23.8 ms | 23.4-24.5 ms | 14.7 MiB |
| `llvm-dis` (C++) on C++ bitcode | 30.8 ms | 30.7-31.8 ms | 54.5 MiB |
| `llvm-dis-rust` on C++ bitcode | 19.4 ms | 18.8-20.4 ms | 14.7 MiB |

Both disassemblers read the same C++-produced bitcode, so the comparison
isolates decoder work from writer record layout. On this parser/printer-only
corpus, Rust throughput was about 1.6x both C++ tools, and peak RSS was
about 3.7x lower. Each tool gets one discarded
warmup run before the five measured runs. These are throughput and memory
measurements, not a generated-code performance claim. The Rust binaries are 5.4 MiB (`llvm-as-rust`) and 4.3 MiB
(`llvm-dis-rust`); the C++ tools need the 124 MiB `libLLVM.so.19.1`.

A clean release build of the two Rust tools, using a separate target
directory, took 9.5 s wall, 31.5 s user, and 1.9 s system time. This is a
Rust-only observation, not a cross-toolchain build-time result. A C++ compile
baseline is not available in this workspace: only the installed LLVM 19.1.7
tools and `libLLVM.so` are present, without LLVM development headers or a
prebuilt C++ build tree, and building LLVM is deliberately avoided. The M1
gate therefore remains open on compile-time comparison as well as on
generated-code quality, which is not measured because M1 has no codegen.

These numbers are a slice result, not an LLVM parity claim. The upstream
`llvm/test` suite is still required before any parity gate, and the synthetic
corpus does not exercise the unsupported features listed above.
