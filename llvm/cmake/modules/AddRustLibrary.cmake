#===----------------------------------------------------------------------===#
# Build support for incrementally porting LLVM components to Rust.
#
# add_llvm_rust_staticlib(<target> <cargo-manifest>
#   [INSTALL_COMPONENT <component>] [INSTALL_WITH_TOOLCHAIN] [STRIP_DEBUG])
#
# Builds a Rust crate as a static library with cargo and exposes it as the
# STATIC IMPORTED target <target>. <target> must match the crate's [lib] name
# so that the produced archive is lib<target>.a (or <target>.lib on Windows).
#
# Crates are built out-of-tree into the CMake binary directory and are only
# pulled in when a component explicitly links them, so the Rust toolchain stays
# an optional dependency of the LLVM build. STRIP_DEBUG runs CMAKE_STRIP on the
# cargo archive before staging it.
#===----------------------------------------------------------------------===#

find_program(LLVM_RUST_CARGO_EXECUTABLE
  NAMES cargo
  HINTS "$ENV{CARGO_HOME}/bin" "$ENV{HOME}/.cargo/bin")
mark_as_advanced(LLVM_RUST_CARGO_EXECUTABLE)

function(add_llvm_rust_staticlib name manifest)
  cmake_parse_arguments(ARG "INSTALL_WITH_TOOLCHAIN;STRIP_DEBUG"
    "INSTALL_COMPONENT" "" ${ARGN})
  if(NOT ARG_INSTALL_COMPONENT)
    set(ARG_INSTALL_COMPONENT LLVMSupport)
  endif()

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
  set_property(GLOBAL PROPERTY LLVM_RUST_${name}_LIBRARY_NAME "${rust_lib_name}")
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

  # Strip a separate copy so the Cargo cache stays untouched, and stage the
  # stripped copy with copy_if_different so an unchanged archive does not
  # rewrite the output and force dependents to relink.
  set(staging_source "${archive}")
  set(strip_command "")
  if(ARG_STRIP_DEBUG AND CMAKE_STRIP AND NOT MSVC)
    if(APPLE)
      set(strip_args -S)
    else()
      set(strip_args --strip-debug)
    endif()
    set(staging_source "${target_dir}/release/${rust_lib_name}.stripped")
    set(strip_command
      COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${archive}" "${staging_source}"
      COMMAND "${CMAKE_STRIP}" ${strip_args} "${staging_source}")
  endif()

  # Separate the Cargo build, content check, and staging stamps. The build
  # stamp is per configuration so Debug cannot mark a stale Release archive
  # up to date. The content stamp only changes when the stripped archive does,
  # which keeps an unchanged archive from relinking dependents. Staging is its
  # own output so Make can recreate a deleted archive.
  set(build_stamp "${target_dir}/build-$<CONFIG>.stamp")
  set(content_stamp "${target_dir}/content.stamp")
  if(CMAKE_CURRENT_FUNCTION_LIST_DIR)
    set(archive_stamp_script "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/RustArchiveStamp.cmake")
  else()
    set(archive_stamp_script "${CMAKE_CURRENT_LIST_DIR}/RustArchiveStamp.cmake")
  endif()

  add_custom_command(OUTPUT "${build_stamp}"
    BYPRODUCTS "${staging_source}"
    COMMAND ${cargo_env} "${LLVM_RUST_CARGO_EXECUTABLE}" build --release --locked
            --manifest-path "${manifest}" --target-dir "${target_dir}"
    ${strip_command}
    COMMAND "${CMAKE_COMMAND}" -E touch "${build_stamp}"
    DEPENDS ${crate_sources}
    COMMENT "Building Rust crate ${name}"
    VERBATIM)

  add_custom_command(OUTPUT "${content_stamp}"
    COMMAND "${CMAKE_COMMAND}"
            "-DSRC=${staging_source}" "-DSTAMP=${content_stamp}"
            -P "${archive_stamp_script}"
    DEPENDS "${build_stamp}"
    COMMENT "Checking Rust archive ${name}"
    VERBATIM)

  add_custom_command(OUTPUT "${staged}"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${staging_source}" "${staged}"
    DEPENDS "${content_stamp}"
    COMMENT "Staging Rust archive ${name}"
    VERBATIM)

  add_custom_target(${name}-rust DEPENDS "${build_stamp}" "${content_stamp}" "${staged}")
  add_library(${name} STATIC IMPORTED GLOBAL)
  set_target_properties(${name} PROPERTIES IMPORTED_LOCATION "${staged}")

  # A static component archives its C++ objects without a final link, so the
  # archive has to be staged by an explicit component target before a direct
  # `cmake --install --component <component>` can copy it.
  if(NOT TARGET ${ARG_INSTALL_COMPONENT}-rust)
    add_custom_target(${ARG_INSTALL_COMPONENT}-rust)
  endif()
  add_dependencies(${ARG_INSTALL_COMPONENT}-rust ${name}-rust)

  # A target-level dependency makes every object file in a consumer wait for
  # Cargo, because CMake adds it to the consumer's object order-only
  # dependencies. The archive is already a link input, so Ninja can use a
  # link-only dependency and let Cargo overlap with the C++ compile. Makefile
  # and IDE generators cannot express that dependency across directories, so
  # they keep the target-level dependency.
  if(CMAKE_GENERATOR MATCHES "Ninja")
    set_property(TARGET ${name} PROPERTY INTERFACE_LINK_DEPENDS "${staged}")
  else()
    add_dependencies(${name} ${name}-rust)
  endif()

  # Both install variants build the owning component, but a static component
  # does not link the Rust archive. Make them stage it explicitly.
  if(TARGET install)
    add_dependencies(install ${ARG_INSTALL_COMPONENT}-rust)
  endif()
  if(TARGET install-${ARG_INSTALL_COMPONENT})
    add_dependencies(install-${ARG_INSTALL_COMPONENT} ${ARG_INSTALL_COMPONENT}-rust)
  endif()
  if(TARGET install-${ARG_INSTALL_COMPONENT}-stripped)
    add_dependencies(install-${ARG_INSTALL_COMPONENT}-stripped ${ARG_INSTALL_COMPONENT}-rust)
  endif()

  # The archive belongs to the owning component's install component so that
  # `cmake --install --component <component>` yields a linkable package. It is
  # omitted from toolchain-only installs unless the owner opts in.
  # Multi-config generators keep each configuration in its own directory;
  # LLVM spells that as $(Configuration), which only MSBuild expands, so name
  # the path with $<CONFIG> in the install rule.
  if(NOT LLVM_INSTALL_TOOLCHAIN_ONLY OR ARG_INSTALL_WITH_TOOLCHAIN)
    if(CMAKE_CONFIGURATION_TYPES)
      install(FILES
                "${LLVM_BINARY_DIR}/$<CONFIG>/lib${LLVM_LIBDIR_SUFFIX}/${rust_lib_name}"
              DESTINATION lib${LLVM_LIBDIR_SUFFIX}
              COMPONENT ${ARG_INSTALL_COMPONENT})
    else()
      install(FILES "${staged}" DESTINATION lib${LLVM_LIBDIR_SUFFIX}
              COMPONENT ${ARG_INSTALL_COMPONENT})
    endif()
  endif()
endfunction()
