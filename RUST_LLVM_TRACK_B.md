# Track B: Rust-native LLVM core

Status: long-term, gated research program. Not part of the default LLVM build.

Track A remains the production migration path on `rust-migration-phase5`:
individual leaf components behind a C ABI staticlib, with the C++ fallback and
`LLVM_ENABLE_RUST_*` switches. Track B is a separate clean-room implementation
of the LLVM core in Rust. It must not change Track A, the default build, or the
C++ ABI.

## Why a separate track

The existing crates prove that leaf algorithms can match or beat C++ and that a
thin C ABI is cheap. They do not prove that LLVM's IR, MC, object, and codegen
layers can be replaced incrementally. Those layers need Rust-native ownership,
allocation, error handling, and data structures. Growing the `no_std`,
caller-owned staticlib crates into a compiler would preserve the wrong
architecture.

Track B therefore starts as a Rust workspace with a Rust API. C ABI and LLVM-C
are external boundaries, not the internal design.

## Scope

This fork contains approximately 1.15 million tracked C/C++ lines across the
components below. The first Track B program is LLVM core, not the whole
monorepo.

| Component | Approx. C/C++ lines | Track B |
| --- | ---: | --- |
| `llvm/lib` | 619k | First program: support, IR, bitcode, MC, object, X86, codegen, transforms |
| `llvm/tools` | 68k | First program: `llvm-as`, `llvm-dis`, `opt`, `llc` equivalents |
| `lld` | 109k | Later, separate linker program |
| `compiler-rt` | 256k | Separate runtime program; not part of the compiler core |
| `libc` | 76k | Separate C library program |
| `libunwind` | 21k | Separate unwinder program |
| Clang/LLDB/MLIR | not in this fork | Separate upstream programs |

The first milestone is X86 only. Target coverage expands only after the X86
vertical slice is correct, reproducible, and performance-competitive.

## Compatibility contract

| Surface | Required | Notes |
| --- | --- | --- |
| Textual IR | Yes | Parser and printer must round-trip the upstream corpus |
| Bitcode | Yes | Reader/writer compatibility and deterministic output |
| Object files | Yes | ELF first; COFF/Mach-O later |
| DWARF/debug info | Yes | Verify with existing tools |
| CLI tools | Yes | Side-by-side binaries first; no flag day |
| LLVM-C API | Yes | Stable external boundary for other languages |
| C++ API/ABI | No, unless explicitly re-scoped | Full compatibility would force the wrapper path |
| C++ plugins | Partial | C ABI facade for selected interfaces |

If full C++ ABI compatibility becomes mandatory for all downstream consumers,
Track B cannot replace LLVM. In that case Track A's C ABI wrapper is the
supported answer.

## Architecture

- Pure Rust core. No C++ wrapper or facade inside the implementation.
- Arena plus index/handle based IR. Do not translate intrusive C++ lists and
  raw pointers directly.
- `Result` for recoverable errors; panics are bugs. No unwinding across an
  external ABI.
- `unsafe` isolated in small audited modules with focused tests.
- Stable Rust 1.100.0 by default. Nightly is an explicit experiment, never the
  default build.
- Minimal dependencies. Start with none; vendor and review anything added.
- Deterministic output and reproducible builds from the first slice.
- C++ LLVM is the differential oracle and the bootstrap, not a runtime
  dependency of the Rust core.

## Workspace layout

```text
llvm-rust/
  Cargo.toml
  README.md
  crates/
    support/       # arena, errors, diagnostics, small ADTs
    ir/            # module, types, constants, instructions, verifier
    bitcode/       # reader/writer
    mc/            # MC layer
    object/        # object reader/writer
    x86/           # X86 MC and codegen
    tools/         # llvm-as, llvm-dis, opt, llc equivalents
  tests/           # differential and lit-style tests
```

Do not add a crate until its first vertical slice has a C++ differential test.

## Bootstrap

`rustc` currently depends on LLVM. A Rust LLVM cannot be the first compiler used
to build `rustc` without creating a cycle. The bootstrap is staged:

1. **Stage 0:** C++ LLVM and the installed stable `rustc` build Rust LLVM.
2. **Stage 1:** Rust LLVM is validated against C++ LLVM on the compatibility
   suite.
3. **Stage 2:** A `rustc` linked against Rust LLVM builds the next stage.
4. **Stage 3:** Reproduce stage 2 and compare outputs and generated code.

Rules:

- C++ LLVM remains the bootstrap seed until stage 2 is reproducible.
- Do not make the default `rustc` depend on Rust LLVM before stage 2 passes.
- Do not require a Rust LLVM to build the first Rust LLVM.
- Keep a documented binary seed or stage-0 toolchain for reproducibility.

## Milestones

These are order-of-magnitude estimates for a funded core team, not a schedule
commitment.

| Milestone | Deliverable | Gate |
| --- | --- | --- |
| M0 - RFC | Scope, compatibility contract, bootstrap policy, test plan | Approved before broad coding |
| M1 - Feasibility spike (3-6 months) | `support` + `ir` parse/print + bitcode reader + differential harness | IR round-trip on agreed corpus; memory/build data recorded |
| M2 - IR/bitcode parity | Verifier, `llvm-as`/`llvm-dis` equivalents | Upstream IR test subset passes |
| M3 - MC/object/X86 | Object reader, assembler/disassembler, ELF | Object/asm differential passes |
| M4 - X86 codegen subset | `llc` equivalent for a constrained function set | Correct output and performance baseline |
| M5 - Transforms | Core analysis and optimization pipeline | `opt` parity on agreed tests |
| M6 - Bootstrap | Stage 1/stage 2 reproducible | Self-host comparison passes |
| M7 - Ecosystem | LLVM-C, packaging, more targets | Downstream pilot passes |

Do not start M3/M4 before M1/M2 prove the architecture.

M1 spike status (2026-11-26): `llvm-rust/` implements `support`, `ir`, and a
bitcode reader/writer plus `llvm-as-rust` and `llvm-dis-rust`. The differential
corpus in `llvm-rust/tests/corpus/` matches LLVM 19.1.7 in both directions, and
the build/runtime/memory numbers are recorded in `llvm-rust/README.md`. This is
not an M2 parity claim: the upstream `llvm/test` suite is still not vendored,
and the slice does not cover metadata, attributes, constant expressions,
atomics, or aggregate/vector instructions.

## Gates and kill criteria

A milestone does not advance on feature count alone. It must pass:

- IR and bitcode round-trip on the agreed corpus.
- Object/asm/DWARF output compared with C++ LLVM.
- Generated code correct for the X86 subset.
- Generated-code performance at least 0.95x C++ LLVM.
- Compile time and peak RSS within the agreed budget.
- Reproducible stage 1 and stage 2 builds.
- LLVM-C, CLI, and format compatibility tests.
- Upstream test suite runnable; this fork must restore or vendor `llvm/test`.

Kill or re-scope if:

- Full C++ ABI compatibility is required for all consumers.
- Performance cannot approach C++ after the codegen spike.
- Bootstrap cannot be made reproducible.
- The upstream test suite cannot be restored.
- The team cannot sustain the core and target maintenance.
- Scope expands into Clang, LLDB, LLD, or runtime libraries too early.

## Build and toolchain policy

| Mode | Profile | LTO | codegen-units | incremental | Use |
| --- | --- | --- | --- | --- | --- |
| Daily development | dev | off | default | on | Edit/test loop |
| Performance test | release | thin | 16 | off | Fast iteration with cross-crate optimization |
| Release candidate | release | fat | 1 | off | Final benchmark, PGO/BOLT, packaging |
| Bootstrap | release | fat | 1 | off | Reproducible stage builds |

Additional rules:

- Use `std`/`alloc`; do not force `no_std` on the compiler core.
- Use PGO and BOLT for release benchmarks; compare at the same optimization
  level as C++.
- Keep debug info available in build artifacts; strip only release packages.
- Cross-compilation and target support are explicit milestones, not defaults.
- Record compile wall/user/sys, peak RSS, binary size, and runtime for every
  comparison.

## Testing strategy

- Restore or vendor the upstream `llvm/test` suite and `llvm-lit`; no parity
  claim without it.
- Differential tests against C++ LLVM for IR, bitcode, object, MC, and codegen.
- Golden tests for published formats and deterministic output.
- Fuzzing for parsers, bitcode readers, and object readers.
- Cross-target tests for every supported target, starting with X86.
- Bootstrap stage comparisons: stage 1 vs stage 2 output and codegen.
- ABI tests for LLVM-C and selected C++ facades, if any.
- Performance suite: compile time, memory, generated-code quality, binary size.

## First six months

1. Write the Track B RFC/ADR and freeze scope, compatibility, and bootstrap.
2. Create `llvm-rust/` with a `support` crate: arena, error, diagnostics.
3. Add an `ir` crate that parses, represents, and prints a small module.
4. Add a `bitcode` reader that round-trips the same corpus.
5. Add a differential harness using C++ `llvm-as`/`llvm-dis` as the oracle.
6. Benchmark both sides: wall, CPU, RSS, binary size, and compile time.
7. Hold the M1 go/no-go review before adding MC or codegen.

## Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Bootstrap cycle | Cannot self-host | C++ LLVM stage 0; stage gates |
| C++ ABI/ecosystem | Downstream cannot migrate | LLVM-C/CLI/format contract; facade only where needed |
| Performance | Compiler slower than C++ | PGO/BOLT, arena design, per-milestone benchmarks |
| Scope creep | Program stalls | Core first; Clang/LLD/LLDB/runtimes separate |
| Target coverage | Only X86 works | Target registry; expand after X86 |
| Debug info | Incompatible output | DWARF tests from M3 |
| Test suite absent | No parity proof | Restore/vendor upstream tests first |
| Maintainer capacity | Project abandoned | Funded core team; gates; documented ADRs |
| Supply chain | Unsafe dependencies | Minimal deps; vendor and audit |
| Licensing | Non-LLVM terms | Apache-2.0 WITH LLVM-exception |

## Relationship to Track A

- Track A remains the production path and must not be destabilized.
- Track A crates can seed Track B with tested leaf algorithms, but Track B does
  not inherit the `no_std`/C-ABI architecture.
- Do not duplicate an algorithm in both tracks without a single source of truth
  or a documented reason.
- Track B changes stay on `rust-migration-track-b` and in `llvm-rust/`.
- Merging Track B into Track A is not planned; they have different goals and
  compatibility contracts.

## Open decisions

These must be resolved in M0 before broad implementation:

1. Is the target LLVM core only, or eventually all components in this fork?
2. Is LLVM-C plus CLI/format compatibility sufficient, or is C++ ABI required?
3. Which upstream test suite and corpus define parity?
4. What is the bootstrap seed and reproducibility policy?
5. What are the performance and resource budgets per milestone?
6. Which dependencies, if any, are allowed?
7. Who owns X86, object formats, debug info, and release packaging?
8. What is the downstream pilot and rollback plan?

## Worktree

Track B is developed in a separate worktree so the Track A checkout stays on
`rust-migration-phase5`:

```sh
git worktree add ../llvm-track-b rust-migration-track-b
cd ../llvm-track-b/llvm-rust
```

Do not switch the Track A worktree to this branch.
