include(ExtendPath)
include(LLVMDistributionSupport)
include(FindPrefixFromConfig)

# Rust archives linked privately into a static C++ library remain in that
# library's exported link interface. Record the enabled support archives so
# LLVMConfig.cmake can recreate their imported targets in both the build and
# install packages.
function(llvm_record_rust_imported_library target)
  get_property(libname GLOBAL PROPERTY LLVM_RUST_${target}_LIBRARY_NAME)
  if(NOT libname)
    set(libname
      "${CMAKE_STATIC_LIBRARY_PREFIX}${target}${CMAKE_STATIC_LIBRARY_SUFFIX}")
  endif()
  set_property(GLOBAL APPEND PROPERTY LLVM_CONFIG_RUST_IMPORTED_TARGETS
    "${target}|${libname}")
endfunction()

# The imported-target location is assembled from this directory and the
# archive name in LLVMConfig.cmake. Keep paths out of the per-target metadata so
# a comma (or another separator) in the prefix cannot truncate a location.
function(llvm_record_rust_imported_directory library_dir)
  if(CMAKE_CONFIGURATION_TYPES)
    string(REPLACE "${CMAKE_CFG_INTDIR}" "__LLVM_RUST_CONFIG__"
      library_dir "${library_dir}")
    set(LLVM_CONFIG_RUST_CONFIGURATIONS "${CMAKE_CONFIGURATION_TYPES}")
  else()
    set(LLVM_CONFIG_RUST_CONFIGURATIONS "")
  endif()
  set(LLVM_CONFIG_RUST_LIBRARY_DIR "${library_dir}" PARENT_SCOPE)
  set(LLVM_CONFIG_RUST_CONFIGURATIONS "${LLVM_CONFIG_RUST_CONFIGURATIONS}"
    PARENT_SCOPE)
endfunction()

set(LLVM_CONFIG_RUST_SUPPORT_PORTS
  SIPHASH CRC MD5 SHA1 SHA256 BASE64 CONVERTUTF CONVERTEBCDIC XXHASH DJB
  VERSIONTUPLE OPTIONSTRCMP)

# CMAKE_INSTALL_PACKAGEDIR might be absolute, so don't reuse below.
string(REPLACE "${CMAKE_CFG_INTDIR}" "." llvm_cmake_builddir "${LLVM_LIBRARY_DIR}")
set(llvm_cmake_builddir "${llvm_cmake_builddir}/cmake/llvm")

# First for users who use an installed LLVM, create the LLVMExports.cmake file.
set(LLVM_EXPORTS_FILE ${llvm_cmake_builddir}/LLVMExports.cmake)
get_property(LLVM_EXPORTS GLOBAL PROPERTY LLVM_EXPORTS)
export(TARGETS ${LLVM_EXPORTS} FILE ${LLVM_EXPORTS_FILE})

# Then for users who want to link against the LLVM build tree, provide the
# normal targets and the build tree only targets.
set(LLVM_BUILDTREEONLY_EXPORTS_FILE ${llvm_cmake_builddir}/LLVMBuildTreeOnlyTargets.cmake)
get_property(LLVM_EXPORTS_BUILDTREE_ONLY GLOBAL PROPERTY LLVM_EXPORTS_BUILDTREE_ONLY)
export(TARGETS ${LLVM_EXPORTS_BUILDTREE_ONLY} FILE ${LLVM_BUILDTREEONLY_EXPORTS_FILE})

get_property(LLVM_AVAILABLE_LIBS GLOBAL PROPERTY LLVM_LIBS)

foreach(lib ${LLVM_AVAILABLE_LIBS})
  get_property(llvm_lib_deps GLOBAL PROPERTY LLVMBUILD_LIB_DEPS_${lib})
  set(all_llvm_lib_deps
    "${all_llvm_lib_deps}\nset_property(GLOBAL PROPERTY LLVMBUILD_LIB_DEPS_${lib} ${llvm_lib_deps})")
endforeach(lib)

# CMake requires that all targets expressed as dependencies exist, so we can't
# have intrinsics_gen in LLVM_COMMON_DEPENDS when it is written out, otherwise
# projects building out of tree will have CMake errors. This only gets hit when
# LLVM_ENABLE_MODULES=On. Eventually we should come up with a better solution to
# this, but there is no easy solution.
if(intrinsics_gen IN_LIST LLVM_COMMON_DEPENDS)
  list(REMOVE_ITEM LLVM_COMMON_DEPENDS intrinsics_gen)
endif()
if(omp_gen IN_LIST LLVM_COMMON_DEPENDS)
  list(REMOVE_ITEM LLVM_COMMON_DEPENDS omp_gen)
endif()
if(vt_gen IN_LIST LLVM_COMMON_DEPENDS)
  list(REMOVE_ITEM LLVM_COMMON_DEPENDS vt_gen)
endif()

#
# Generate LLVMConfig.cmake for the build tree.
#

set(LLVM_CONFIG_CODE "
# LLVM_BUILD_* values available only from LLVM build tree.
set(LLVM_BUILD_BINARY_DIR \"${LLVM_BINARY_DIR}\")
set(LLVM_BUILD_LIBRARY_DIR \"${LLVM_LIBRARY_DIR}\")
set(LLVM_BUILD_MAIN_INCLUDE_DIR \"${LLVM_MAIN_INCLUDE_DIR}\")
set(LLVM_BUILD_MAIN_SRC_DIR \"${LLVM_MAIN_SRC_DIR}\")
")

set(LLVM_CONFIG_MAIN_INCLUDE_DIR "${LLVM_MAIN_INCLUDE_DIR}")
set(LLVM_CONFIG_INCLUDE_DIR "${LLVM_INCLUDE_DIR}")
set(LLVM_CONFIG_INCLUDE_DIRS
  "${LLVM_CONFIG_MAIN_INCLUDE_DIR}"
  "${LLVM_CONFIG_INCLUDE_DIR}"
  )
list(REMOVE_DUPLICATES LLVM_CONFIG_INCLUDE_DIRS)

set(LLVM_CONFIG_LIBRARY_DIR "${LLVM_LIBRARY_DIR}")
set(LLVM_CONFIG_LIBRARY_DIRS
  "${LLVM_CONFIG_LIBRARY_DIR}"
  # FIXME: Should there be other entries here?
  )
list(REMOVE_DUPLICATES LLVM_CONFIG_LIBRARY_DIRS)

set_property(GLOBAL PROPERTY LLVM_CONFIG_RUST_IMPORTED_TARGETS "")
llvm_record_rust_imported_directory("${LLVM_LIBRARY_DIR}")
foreach(rust_port IN LISTS LLVM_CONFIG_RUST_SUPPORT_PORTS)
  if(LLVM_ENABLE_RUST_${rust_port})
    string(TOLOWER "${rust_port}" rust_library)
    llvm_record_rust_imported_library("llvm_${rust_library}")
  endif()
endforeach()
get_property(LLVM_CONFIG_RUST_IMPORTED_TARGETS GLOBAL
  PROPERTY LLVM_CONFIG_RUST_IMPORTED_TARGETS)

# The Rust archive is an imported target in the build tree. Recreate it in
# LLVMConfig.cmake so an exported static component can resolve its private
# link dependency from the installed package. A multi-config build must expose
# concrete per-config paths; a downstream generator cannot expand the
# producer's CMAKE_CFG_INTDIR placeholder.
set(LLVM_RUSTDEMANGLE_LIBRARY_NAME "")
set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATION "")
set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATIONS "")
if(LLVM_ENABLE_RUST_RUSTDEMANGLE)
  get_property(LLVM_RUSTDEMANGLE_LIBRARY_NAME GLOBAL
    PROPERTY LLVM_RUST_llvm_rustdemangle_LIBRARY_NAME)
  if(NOT LLVM_RUSTDEMANGLE_LIBRARY_NAME)
    set(LLVM_RUSTDEMANGLE_LIBRARY_NAME
      "${CMAKE_STATIC_LIBRARY_PREFIX}llvm_rustdemangle${CMAKE_STATIC_LIBRARY_SUFFIX}")
  endif()
  if(CMAKE_CONFIGURATION_TYPES)
    foreach(llvm_config IN LISTS CMAKE_CONFIGURATION_TYPES)
      string(TOUPPER "${llvm_config}" llvm_config_upper)
      set(llvm_rustdemangle_location
        "${LLVM_LIBRARY_DIR}/${LLVM_RUSTDEMANGLE_LIBRARY_NAME}")
      string(REPLACE "${CMAKE_CFG_INTDIR}" "${llvm_config}"
        llvm_rustdemangle_location "${llvm_rustdemangle_location}")
      list(APPEND LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATIONS
        "${llvm_config_upper}=${llvm_rustdemangle_location}")
    endforeach()
  else()
    set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATION
      "${LLVM_LIBRARY_DIR}/${LLVM_RUSTDEMANGLE_LIBRARY_NAME}")
  endif()
endif()

set(LLVM_RUST_DLANG_LIBRARY_NAME "")
set(LLVM_CONFIG_DLANG_IMPORTED_LOCATION "")
set(LLVM_CONFIG_DLANG_IMPORTED_LOCATIONS "")
if(LLVM_ENABLE_RUST_DLANG)
  get_property(LLVM_RUST_DLANG_LIBRARY_NAME GLOBAL
    PROPERTY LLVM_RUST_llvm_dlangdemangle_LIBRARY_NAME)
  if(NOT LLVM_RUST_DLANG_LIBRARY_NAME)
    set(LLVM_RUST_DLANG_LIBRARY_NAME
      "${CMAKE_STATIC_LIBRARY_PREFIX}llvm_dlangdemangle${CMAKE_STATIC_LIBRARY_SUFFIX}")
  endif()
  if(CMAKE_CONFIGURATION_TYPES)
    foreach(llvm_config IN LISTS CMAKE_CONFIGURATION_TYPES)
      string(TOUPPER "${llvm_config}" llvm_config_upper)
      set(llvm_dlangdemangle_location
        "${LLVM_LIBRARY_DIR}/${LLVM_RUST_DLANG_LIBRARY_NAME}")
      string(REPLACE "${CMAKE_CFG_INTDIR}" "${llvm_config}"
        llvm_dlangdemangle_location "${llvm_dlangdemangle_location}")
      list(APPEND LLVM_CONFIG_DLANG_IMPORTED_LOCATIONS
        "${llvm_config_upper}=${llvm_dlangdemangle_location}")
    endforeach()
  else()
    set(LLVM_CONFIG_DLANG_IMPORTED_LOCATION
      "${LLVM_LIBRARY_DIR}/${LLVM_RUST_DLANG_LIBRARY_NAME}")
  endif()
endif()

set(LLVM_CONFIG_BINARY_DIR "${LLVM_BINARY_DIR}")
set(LLVM_CONFIG_CMAKE_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
set(LLVM_CONFIG_TOOLS_BINARY_DIR "${LLVM_TOOLS_BINARY_DIR}")

# Generate a default location for lit
if (LLVM_BUILD_UTILS)
  if (CMAKE_HOST_WIN32 AND NOT CYGWIN)
    set(LLVM_CONFIG_DEFAULT_EXTERNAL_LIT "${LLVM_CONFIG_TOOLS_BINARY_DIR}/llvm-lit.py")
  else()
    set(LLVM_CONFIG_DEFAULT_EXTERNAL_LIT "${LLVM_CONFIG_TOOLS_BINARY_DIR}/llvm-lit")
  endif()
endif()

if (LLVM_LINK_LLVM_DYLIB)
  set(LLVM_CONFIG_LINK_LLVM_DYLIB
      "set(LLVM_LINK_LLVM_DYLIB ${LLVM_LINK_LLVM_DYLIB})")
endif()

# We need to use the full path to the LLVM Exports file to make sure we get the
# one from the build tree. This is due to our cmake files being split between
# this source dir and the binary dir in the build tree configuration and the
# LLVM_CONFIG_CMAKE_DIR being the source directory. In contrast in the install
# tree, both the generated LLVMExports.cmake file and the rest of the cmake
# source files are put in the same cmake directory.
set(LLVM_CONFIG_EXPORTS "${LLVM_EXPORTS};${LLVM_EXPORTS_BUILDTREE_ONLY}")
set(LLVM_CONFIG_INCLUDE_EXPORTS "include(\"${LLVM_EXPORTS_FILE}\")")
set(llvm_config_include_buildtree_only_exports
"include(\"${LLVM_BUILDTREEONLY_EXPORTS_FILE}\")")
configure_file(
  LLVMConfig.cmake.in
  ${llvm_cmake_builddir}/LLVMConfig.cmake
  @ONLY)
set(llvm_config_include_buildtree_only_exports)

# For compatibility with projects that include(LLVMConfig)
# via CMAKE_MODULE_PATH, place API modules next to it.
# Copy without source permissions because the source could be read-only,
# but we need to write into the copied folder.
# This should be removed in the future.
file(COPY .
  DESTINATION ${llvm_cmake_builddir}
  NO_SOURCE_PERMISSIONS
  FILES_MATCHING PATTERN *.cmake
  PATTERN CMakeFiles EXCLUDE
  PATTERN llvm-driver-template.cpp.in
  )

#
# Generate LLVMConfig.cmake for the install tree.
#

find_prefix_from_config(LLVM_CONFIG_CODE LLVM_INSTALL_PREFIX "${LLVM_INSTALL_PACKAGE_DIR}")

extend_path(LLVM_CONFIG_MAIN_INCLUDE_DIR "\${LLVM_INSTALL_PREFIX}" "${CMAKE_INSTALL_INCLUDEDIR}")
# This is the same as the above because the handwritten and generated headers
# are combined in one directory at install time.
set(LLVM_CONFIG_INCLUDE_DIR "${LLVM_CONFIG_MAIN_INCLUDE_DIR}")
set(LLVM_CONFIG_INCLUDE_DIRS
  "${LLVM_CONFIG_MAIN_INCLUDE_DIR}"
  "${LLVM_CONFIG_INCLUDE_DIR}"
  )
list(REMOVE_DUPLICATES LLVM_CONFIG_INCLUDE_DIRS)

extend_path(LLVM_CONFIG_LIBRARY_DIR "\${LLVM_INSTALL_PREFIX}" "lib\${LLVM_LIBDIR_SUFFIX}")
set(LLVM_CONFIG_LIBRARY_DIRS
  "${LLVM_CONFIG_LIBRARY_DIR}"
  # FIXME: Should there be other entries here?
  )
list(REMOVE_DUPLICATES LLVM_CONFIG_LIBRARY_DIRS)

set_property(GLOBAL PROPERTY LLVM_CONFIG_RUST_IMPORTED_TARGETS "")
set(LLVM_CONFIG_RUST_LIBRARY_DIR "${LLVM_CONFIG_LIBRARY_DIR}")
set(LLVM_CONFIG_RUST_CONFIGURATIONS "")
foreach(rust_port IN LISTS LLVM_CONFIG_RUST_SUPPORT_PORTS)
  if(LLVM_ENABLE_RUST_${rust_port})
    string(TOLOWER "${rust_port}" rust_library)
    llvm_record_rust_imported_library("llvm_${rust_library}")
  endif()
endforeach()
get_property(LLVM_CONFIG_RUST_IMPORTED_TARGETS GLOBAL
  PROPERTY LLVM_CONFIG_RUST_IMPORTED_TARGETS)

# Keep the Rust demangler's imported target available to installed consumers.
# Installed libraries live directly in the install library directory, so a
# plain IMPORTED_LOCATION works for every generator.
set(LLVM_RUSTDEMANGLE_LIBRARY_NAME "")
set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATION "")
set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATIONS "")
if(LLVM_ENABLE_RUST_RUSTDEMANGLE)
  get_property(LLVM_RUSTDEMANGLE_LIBRARY_NAME GLOBAL
    PROPERTY LLVM_RUST_llvm_rustdemangle_LIBRARY_NAME)
  if(NOT LLVM_RUSTDEMANGLE_LIBRARY_NAME)
    set(LLVM_RUSTDEMANGLE_LIBRARY_NAME
      "${CMAKE_STATIC_LIBRARY_PREFIX}llvm_rustdemangle${CMAKE_STATIC_LIBRARY_SUFFIX}")
  endif()
  set(LLVM_CONFIG_RUSTDEMANGLE_IMPORTED_LOCATION
    "${LLVM_CONFIG_LIBRARY_DIR}/${LLVM_RUSTDEMANGLE_LIBRARY_NAME}")
endif()

set(LLVM_RUST_DLANG_LIBRARY_NAME "")
set(LLVM_CONFIG_DLANG_IMPORTED_LOCATION "")
set(LLVM_CONFIG_DLANG_IMPORTED_LOCATIONS "")
if(LLVM_ENABLE_RUST_DLANG)
  get_property(LLVM_RUST_DLANG_LIBRARY_NAME GLOBAL
    PROPERTY LLVM_RUST_llvm_dlangdemangle_LIBRARY_NAME)
  if(NOT LLVM_RUST_DLANG_LIBRARY_NAME)
    set(LLVM_RUST_DLANG_LIBRARY_NAME
      "${CMAKE_STATIC_LIBRARY_PREFIX}llvm_dlangdemangle${CMAKE_STATIC_LIBRARY_SUFFIX}")
  endif()
  set(LLVM_CONFIG_DLANG_IMPORTED_LOCATION
    "${LLVM_CONFIG_LIBRARY_DIR}/${LLVM_RUST_DLANG_LIBRARY_NAME}")
endif()

set(LLVM_CONFIG_BINARY_DIR "\${LLVM_INSTALL_PREFIX}")
extend_path(LLVM_CONFIG_CMAKE_DIR "\${LLVM_INSTALL_PREFIX}" "${LLVM_INSTALL_PACKAGE_DIR}")
extend_path(LLVM_CONFIG_TOOLS_BINARY_DIR "\${LLVM_INSTALL_PREFIX}" "${LLVM_TOOLS_INSTALL_DIR}")

# Generate a default location for lit
if (LLVM_INSTALL_UTILS AND LLVM_BUILD_UTILS)
  if (CMAKE_HOST_WIN32 AND NOT CYGWIN)
    set(LLVM_CONFIG_DEFAULT_EXTERNAL_LIT "${LLVM_CONFIG_TOOLS_BINARY_DIR}/llvm-lit.py")
  else()
    set(LLVM_CONFIG_DEFAULT_EXTERNAL_LIT "${LLVM_CONFIG_TOOLS_BINARY_DIR}/llvm-lit")
  endif()
endif()

get_config_exports_includes(LLVM LLVM_CONFIG_INCLUDE_EXPORTS)
set(LLVM_CONFIG_EXPORTS "${LLVM_EXPORTS}")
configure_file(
  LLVMConfig.cmake.in
  ${CMAKE_CURRENT_BINARY_DIR}/CMakeFiles/LLVMConfig.cmake
  @ONLY)

# Generate LLVMConfigVersion.cmake for build and install tree.
configure_file(
  LLVMConfigVersion.cmake.in
  ${llvm_cmake_builddir}/LLVMConfigVersion.cmake
  @ONLY)

if (NOT LLVM_INSTALL_TOOLCHAIN_ONLY)
  install_distribution_exports(LLVM)

  install(FILES
    ${CMAKE_CURRENT_BINARY_DIR}/CMakeFiles/LLVMConfig.cmake
    ${llvm_cmake_builddir}/LLVMConfigVersion.cmake
    LLVM-Config.cmake
    DESTINATION ${LLVM_INSTALL_PACKAGE_DIR}
    COMPONENT cmake-exports)

  install(DIRECTORY .
    DESTINATION ${LLVM_INSTALL_PACKAGE_DIR}
    COMPONENT cmake-exports
    FILES_MATCHING PATTERN *.cmake
    PATTERN LLVMConfig.cmake EXCLUDE
    PATTERN LLVMConfigExtensions.cmake EXCLUDE
    PATTERN LLVMConfigVersion.cmake EXCLUDE
    PATTERN LLVM-Config.cmake EXCLUDE
    PATTERN GetHostTriple.cmake EXCLUDE
    PATTERN llvm-driver-template.cpp.in)

  if (NOT LLVM_ENABLE_IDE)
    # Add a dummy target so this can be used with LLVM_DISTRIBUTION_COMPONENTS
    add_custom_target(cmake-exports)
    add_llvm_install_targets(install-cmake-exports
                             COMPONENT cmake-exports)
  endif()
endif()
