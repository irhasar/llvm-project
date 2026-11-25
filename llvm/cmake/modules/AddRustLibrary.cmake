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

  # Cargo's target has to agree with the C++ toolchain: MinGW cannot link an
  # MSVC archive and MSVC cannot link a GNU one. Diagnose that here instead of
  # failing at link time with an archive cargo never wrote.
  if(rust_host)
    if(MSVC AND NOT rust_host MATCHES "-msvc$")
      message(FATAL_ERROR
        "The Rust toolchain builds for ${rust_host} but the C++ compiler is "
        "MSVC. Use a *-windows-msvc Rust target.")
    elseif(NOT MSVC AND rust_host MATCHES "-msvc$")
      message(FATAL_ERROR
        "The Rust toolchain builds for ${rust_host} but the C++ compiler is "
        "not MSVC. Use a GNU Rust target.")
    endif()
  endif()

  set(target_dir "${CMAKE_CURRENT_BINARY_DIR}/rust/${name}")
  # Rust's MSVC targets write <name>.lib, the GNU targets write lib<name>.a.
  if(rust_host MATCHES "-msvc$" OR (NOT rust_host AND MSVC))
    set(rust_lib_name "${name}.lib")
  else()
    set(rust_lib_name "lib${name}.a")
  endif()
  set(archive "${target_dir}/release/${rust_lib_name}")
  # Stage the archive next to the other LLVM libraries so that llvm-config's
  # -l<name> resolves in the build tree and the install rule can pick it up.
  set(staged "${LLVM_LIBRARY_OUTPUT_INTDIR}/${rust_lib_name}")

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

  add_custom_command(OUTPUT "${staged}"
    COMMAND ${cargo_env} "${LLVM_RUST_CARGO_EXECUTABLE}" build --release --locked
            --manifest-path "${manifest}" --target-dir "${target_dir}"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${archive}" "${staged}"
    DEPENDS ${crate_sources}
    COMMENT "Building Rust crate ${name}"
    VERBATIM)

  add_custom_target(${name}-rust DEPENDS "${staged}")
  add_library(${name} STATIC IMPORTED GLOBAL)
  set_target_properties(${name} PROPERTIES IMPORTED_LOCATION "${staged}")
  add_dependencies(${name} ${name}-rust)

  # The archive belongs to the LLVMSupport install component so that
  # `cmake --install --component LLVMSupport` yields a linkable package.
  # Multi-config generators keep each configuration in its own directory;
  # LLVM spells that as $(Configuration), which only MSBuild expands, so name
  # the path with $<CONFIG> in the install rule.
  if(CMAKE_CONFIGURATION_TYPES)
    install(FILES
              "${LLVM_BINARY_DIR}/$<CONFIG>/lib${LLVM_LIBDIR_SUFFIX}/${rust_lib_name}"
            DESTINATION lib${LLVM_LIBDIR_SUFFIX} COMPONENT LLVMSupport)
  else()
    install(FILES "${staged}" DESTINATION lib${LLVM_LIBDIR_SUFFIX}
            COMPONENT LLVMSupport)
  endif()
endfunction()
