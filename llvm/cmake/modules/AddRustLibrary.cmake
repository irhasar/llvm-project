#===----------------------------------------------------------------------===#
# Build support for incrementally porting LLVM components to Rust.
#
# add_llvm_rust_staticlib(<target> <cargo-manifest>)
#
# Builds a Rust crate as a static library with cargo and exposes it as the
# STATIC IMPORTED target <target>. <target> must match the crate's [lib] name
# so that the produced archive is lib<target>.a (or <target>.lib on Windows).
#
# Crates are built out-of-tree into the CMake binary directory and are only
# pulled in when a component explicitly links them, so the Rust toolchain stays
# an optional dependency of the LLVM build.
#===----------------------------------------------------------------------===#

find_program(LLVM_RUST_CARGO_EXECUTABLE
  NAMES cargo
  HINTS "$ENV{CARGO_HOME}/bin" "$ENV{HOME}/.cargo/bin")
mark_as_advanced(LLVM_RUST_CARGO_EXECUTABLE)

function(add_llvm_rust_staticlib name manifest)
  if(NOT LLVM_RUST_CARGO_EXECUTABLE)
    message(FATAL_ERROR
      "Rust support was requested but cargo was not found. Install a Rust "
      "toolchain or set LLVM_RUST_CARGO_EXECUTABLE to the cargo binary.")
  endif()

  get_filename_component(crate_dir "${manifest}" DIRECTORY)
  file(GLOB_RECURSE crate_sources CONFIGURE_DEPENDS
    "${crate_dir}/src/*.rs" "${crate_dir}/Cargo.lock")
  list(APPEND crate_sources "${manifest}")

  # Cargo picks its own target, independently of the C++ compiler, so ask it
  # which host triple it will build for and derive the archive name from that.
  execute_process(
    COMMAND "${LLVM_RUST_CARGO_EXECUTABLE}" -vV
    OUTPUT_VARIABLE rust_version
    ERROR_QUIET
    OUTPUT_STRIP_TRAILING_WHITESPACE)
  if(rust_version MATCHES "host: ([A-Za-z0-9_.-]+)")
    set(rust_host "${CMAKE_MATCH_1}")
  endif()

  set(target_dir "${CMAKE_CURRENT_BINARY_DIR}/rust/${name}")
  # Rust's MSVC targets write <name>.lib, the GNU targets write lib<name>.a.
  if(rust_host MATCHES "-msvc$" OR (NOT rust_host AND MSVC))
    set(archive "${target_dir}/release/${name}.lib")
  else()
    set(archive "${target_dir}/release/lib${name}.a")
  endif()

  # Makefiles do not rerun a rule when only its recipe changes, so the
  # configured flags have to be a real dependency of the archive.
  set(flags_file "${target_dir}/rust-flags.txt")
  file(GENERATE OUTPUT "${flags_file}" CONTENT "${LLVM_RUST_FLAGS}\n")
  list(APPEND crate_sources "${flags_file}")

  # RUSTFLAGS has to be passed through the environment; only set it when the
  # user opted into extra flags so the ambient environment keeps working.
  if(LLVM_RUST_FLAGS)
    set(cargo_env "${CMAKE_COMMAND}" -E env "RUSTFLAGS=${LLVM_RUST_FLAGS}")
  else()
    set(cargo_env "")
  endif()

  add_custom_command(OUTPUT "${archive}"
    COMMAND ${cargo_env} "${LLVM_RUST_CARGO_EXECUTABLE}" build --release --locked
            --manifest-path "${manifest}" --target-dir "${target_dir}"
    DEPENDS ${crate_sources}
    COMMENT "Building Rust crate ${name}"
    VERBATIM)

  add_custom_target(${name}-rust DEPENDS "${archive}")
  add_library(${name} STATIC IMPORTED GLOBAL)
  set_target_properties(${name} PROPERTIES IMPORTED_LOCATION "${archive}")
  add_dependencies(${name} ${name}-rust)
endfunction()
