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
- `atomicrmw` syntax is `atomicrmw [volatile] <op> <ptrty> <ptr>, <valty>
  <val> [syncscope(...)] <ordering>, align N`; it rejects `unordered`,
  `not_atomic`, and missing alignment. Bitcode code 59 is `[ptr, val, op,
  volatile, ordering, syncscope, align]` with LLVM's variable `pushValueAndType`
  encoding; operation codes are LLVM 19 `RMW_*` values 0..16.
- `cmpxchg` syntax is `cmpxchg [weak] [volatile] <ptrty> <ptr>, <ty> <cmp>,
  <ty> <new> [syncscope(...)] <success> <failure>, align N`; success rejects
  `unordered`/`not_atomic`, failure allows only `monotonic`, `acquire`, or
  `seq_cst`. Its result is a literal `{ ty, i1 }` struct. Bitcode code 46 is
  `[ptr, cmp, new, volatile, success, syncscope, failure, weak, align]`.
  LLVM 19 accepts a failure ordering stronger than success, so do not add a
  strength comparison that the oracle does not enforce.
- Constant expressions use `CST_CODE_CE_GEP = 32` and
  `CST_CODE_CE_CAST = 11`. A GEP record is
  `[source_ty, flags, ptr_ty, ptr, index_ty, index, ...]`; flags bit 0 is
  `inbounds`, bit 1 is `nusw`, and bit 2 is `nuw`. LLVM 19 spells textual
  `inbounds` as flags 3, so write 3 and accept 0, 1, or 3. Cast opcodes use
  the bitcode `CAST_*` values, not the instruction enum values. Resolve
  nested forward global/function references inside constant expressions
  before verification and bitcode emission.
- `va_arg` uses `FUNC_CODE_INST_VAARG = 23` with the fixed record
  `[valistty, valist, instty]`. The value is a plain relative value, not a
  `pushValueAndType` pair, because its type is already in slot 0. The list
  operand must be a pointer; the result type is `instty`.
- `blockaddress` uses `CST_CODE_BLOCKADDRESS = 21` with
  `[function_ty, function_value, block_index]`. It can appear in the
  module-level constants block or a function-local constants block. Decode
  module-level constants in one pass: add the constant, then queue its ID for
  block resolution after all function bodies are read. Only function-local
  constants preallocate slots, because local aggregates may reference a later
  local constant. Preallocating every module constant just to support
  blockaddress cost about 5% on a 50k-global module. The function value must
  resolve to a `Function`, not a global wrapper.
- `indirectbr` uses `FUNC_CODE_INST_INDIRECTBR = 31` with
  `[opty, address, bb0, bb1, ...]`. The address is a relative value; the
  destinations are absolute basic-block indices, not value IDs. An empty
  destination list is valid in LLVM 19.
- `FUNC_CODE_BLOCKADDR_USERS = 60` is a lazy-loading hint emitted after the
  terminator of a function whose blocks are referenced by `blockaddress`.
  Ignore it in the function body reader; treating it as an instruction fails
  with "instruction after the last block".
- `callbr` uses `FUNC_CODE_INST_CALLBR = 57` with `[attr, flags, default_dest,
  num_indirect, dest..., fty, callee, args...]`. The destinations are absolute
  block indices, and flags bit 15 (`CALL_EXPLICIT_TYPE`) must be set. Its
  result type comes from `fty`. The verifier requires a non-empty indirect
  destination list, an inline-asm callee, and label operands that match the
  destinations.
- `invoke` uses `FUNC_CODE_INST_INVOKE = 13` with `[attr, flags, normal_dest,
  unwind_dest, fty, callee, args...]`. Both destinations are absolute block
  indices and flags bit 13 (`CALL_EXPLICIT_TYPE`) must be set. Unlike `call`,
  the destinations come before the function type.
- `resume` uses `FUNC_CODE_INST_RESUME = 39` with a single `getValueTypePair`
  operand. `landingpad` uses `FUNC_CODE_INST_LANDINGPAD = 47` with
  `[ty, cleanup, num_clauses, kind0, value0, ...]`; kind 0 is `catch`, 1 is
  `filter`. The legacy `LANDINGPAD_OLD = 40` record inserts a value before the
  cleanup flag and is accepted on read.
- Windows funclets use `CLEANUPRET = 48` (`[pad]` or `[pad, unwind_bb]`),
  `CATCHRET = 49` (`[pad, successor_bb]`), `CATCHPAD = 50` and
  `CLEANUPPAD = 51` (`[parent_pad, num_args, typed args...]`), and
  `CATCHSWITCH = 52` (`[parent_pad, num_handlers, handler_bb...,
  unwind_bb?]`). The parent pad is a relative value; handlers and unwind
  destinations are absolute basic-block indices. `catchswitch` requires at
  least one handler, and `catchret` has no unwind destination.
- A funclet parent of `none` is the `token` null constant. LLVM 19 always
  writes that constant into the module constants block, so the writer adds it
  when any pad has no parent and numbers it before the function body. Textual
  `catchret`/`cleanupret` print the pad without its `token` type, unlike the
  typed arguments of `catchpad` and `cleanuppad`.
- The function record personality operand is index 16, after prologue (12),
  DLL storage (13), comdat (14), and prefix data (15). Writing it at index 15
  makes C++ `llvm-dis` print `prefix ptr @personality` instead of
  `personality`. The operand is `value_id + 1`, with zero meaning absent.
- Textual `landingpad` clauses and the `to`/`unwind` labels of `invoke` and
  `callbr` may start on a new line in C++ pretty output. Skip EOL tokens before
  `cleanup`, each clause, and each label.
- Inline asm uses `CST_CODE_INLINEASM = 30` with `[fty, flags, asm_len,
  asm_bytes..., constraint_len, constraint_bytes...]`; flags are sideeffect
  (1), alignstack (2), inteldialect (4), and unwind (8). Textual inline asm
  spells only the return type before `asm`, so infer the function type after
  parsing the argument list and patch the constant. Keep the asm and
  constraint strings as bytes; they need not be UTF-8.
- LLVM abbreviations may contain a zero-width `Fixed(0)` field, for example
  `SimpleGVarAbbrev` when the module has no alignment. `BitReader::read_bits`
  must return zero for width 0 without consuming bits, not reject the field.
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

### M1 performance baseline (2026-11-27)

Corpus: 1000 functions, 1,104,987 bytes, LLVM 19.1.7, `taskset -c 0`, 11 runs
after 3 warmups.

| Tool | Median | Min-max | Peak RSS |
| --- | ---: | ---: | ---: |
| `llvm-as` (C++) | 63.4 ms | 61.8-73.9 ms | 60.5 MiB |
| `llvm-as-rust` | 24.4 ms | 24.0-25.1 ms | 14.8 MiB |
| `llvm-dis` (C++) on C++ bitcode | 51.8 ms | 50.1-52.1 ms | 58.3 MiB |
| `llvm-dis-rust` on C++ bitcode | 25.2 ms | 24.9-26.0 ms | 19.2 MiB |

In-process component medians: parse 12.21 ms, print 3.59 ms, bitcode write
4.26 ms, bitcode read 8.57 ms, assemble 15.56 ms, disassemble 12.20 ms, verify
12.25 ms. The optimization audit reduced parse from 8.48 ms, read from 6.66 ms,
print from 2.35 ms, and disassemble from 8.98 ms on the earlier corpus; the
current corpus adds vector operations and atomic load/store, so its absolute
component times are larger. Regression target: do not regress these medians
beyond noise; investigate any consistent slowdown before adding features.

Post-`atomicrmw`/`cmpxchg` spot check: assembly was 63.8 ms C++ / 24.6 ms Rust
and disassembly 50.5 ms / 25.2 ms, with peak RSS 60.6/14.7 MiB and
58.1/19.2 MiB. Those are within noise of the baseline, so the new instruction
arms did not move throughput or memory.

Post-constant-expression spot check: assembly was 62.9 ms C++ / 25.1 ms Rust
and disassembly 51.1 ms / 25.1 ms, with peak RSS 60.6/14.8 MiB and
58.3/19.5 MiB. Also within noise.

Post-`va_arg` spot check on the 1,170,987-byte corpus with one `va_arg` per
function: assembly was 66.5 ms C++ / 26.7 ms Rust and disassembly 54.0 ms /
27.4 ms, with peak RSS 60.8/15.0 MiB and 58.5/21.2 MiB. The larger corpus
raises absolute times; the assembly ratio stayed at 2.5x and disassembly at
2.0x.

Post-`blockaddress`/`indirectbr` spot check on the 1,308,925-byte corpus with a
dispatch block and one blockaddress constant per function: assembly was 72.3 ms
C++ / 31.4 ms Rust and disassembly 59.9 ms / 34.0 ms, with peak RSS
62.0/16.4 MiB and 59.4/26.1 MiB. Rust stayed at 2.3x assembly and 1.8x
disassembly throughput with lower RSS.

Post-`callbr`/inline-asm check: on the same 1,308,925-byte corpus (11 runs, 3
warmups), assembly was 73.3 ms C++ / 32.6 ms Rust and disassembly 62.0 ms /
34.6 ms, with peak RSS 61.9/18.1 MiB and 59.6/27.9 MiB. A separate
178,890-byte corpus with 1000 `callbr` functions measured assembly 26.8 ms C++
/ 7.9 ms Rust and disassembly 23.9 ms / 8.2 ms, with peak RSS 54.3/14.1 MiB
and 53.5/14.1 MiB. The new terminator therefore keeps Rust at about 3x
throughput on a callbr-heavy input while using about 3.8x less memory.

Post-`invoke`/`landingpad`/`resume` check: on a 276,955-byte corpus with 1000
exception-handling functions (11 runs, 3 warmups), assembly was 28.6 ms C++ /
7.7 ms Rust and disassembly 25.7 ms / 7.4 ms, with peak RSS 54.5/14.9 MiB and
53.6/14.9 MiB. The same mixed 1,308,925-byte corpus stayed at 73.4/33.3 ms
assembly and 59.3/34.2 ms disassembly, so the new records did not regress the
existing path. Rust remains about 3.7x faster with 3.6x less memory on the
exception-heavy input.

Post-Windows-funclet check: on a 543,977-byte corpus with 1000 functions that
mix `invoke`, `catchswitch`, two `catchpad`/`catchret` handlers, and a
`cleanuppad`/`cleanupret` (7 runs, 2 warmups), assembly was 41.0 ms C++ /
13.1 ms Rust and disassembly 34.9 ms / 12.5 ms, with peak RSS 55.6/15.0 MiB
and 55.1/15.0 MiB. Rust is about 3x faster with 3.7x less memory on this input.
The mixed and legacy-EH corpora stayed at 77.9/36.4 ms and 64.4/37.8 ms for
assembly/disassembly and 29.2/7.8 ms and 26.4/7.7 ms respectively, so the new
records did not regress existing paths.

The absolute jump from the earlier 26.7/27.4 ms Rust numbers is corpus growth,
not a code regression: a 25-pair A/B on the identical 1,170,987-byte corpus
measured assembly 26.86 -> 26.95 ms (+0.3%) and disassembly stayed at about
28.2 ms. A constants-heavy 50k-global module did expose a real regression from
preallocating every module constant for blockaddress: read 23.8 -> 25.0 ms
(+5.0%) and disassemble 28.4 -> 30.0 ms (+5.8%). Restoring single-pass module
constants with deferred blockaddress IDs removed it; the 25-pair A/B then
measured read -0.7% and disassemble -1.5%. The slice also fixed a reader bug
that only appeared with unaligned globals: zero-width abbreviation fields must
decode as zero.

Performance lesson: a full-file `Vec<Token>` retained 354k tokens at 48 bytes
each (552k capacity) and dominated peak RSS and page-fault time. The parser now
lexes through a two-token streaming window. Internal parse CPU rose about 1 ms,
but process assembly fell from 33.5 ms to 24.4 ms and peak RSS from 26.0 MiB
to 14.8 MiB. Compare on the same corpus before calling a ratio change a
regression: adding atomic load/store made the corpus 13.8% larger, which moved
the ratio while per-byte work improved. A naive 64-bit load fast path in
`BitReader::read_bits` was measured slower than the byte loop; do not replace
the loop without a component-level win.

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
