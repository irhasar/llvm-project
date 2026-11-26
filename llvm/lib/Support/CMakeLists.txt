include(GetLibraryName)

# Ensure that libSupport does not carry any static global initializer.
# libSupport can be embedded in use cases where we don't want to load all
# cl::opt unless we want to parse the command line.
# ManagedStatic can be used to enable lazy-initialization of globals.
# We don't use `add_flag_if_supported` as instead of compiling an empty file we
# check if the current platform is able to compile global std::mutex with this
# flag (Linux can, Darwin can't for example).
check_cxx_compiler_flag("-Werror=global-constructors" HAS_WERROR_GLOBAL_CTORS)
if (HAS_WERROR_GLOBAL_CTORS)
  SET(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -Werror=global-constructors")
  CHECK_CXX_SOURCE_COMPILES("
  #include <mutex>
  static std::mutex TestGlobalCtorDtor;
  static std::recursive_mutex TestGlobalCtorDtor2;
  int main() { (void)TestGlobalCtorDtor; (void)TestGlobalCtorDtor2; return 0;}
  " LLVM_HAS_NOGLOBAL_CTOR_MUTEX)
  if (NOT LLVM_HAS_NOGLOBAL_CTOR_MUTEX)
    string(REPLACE "-Werror=global-constructors" "" CMAKE_CXX_FLAGS ${CMAKE_CXX_FLAGS})
  endif()
endif()

if(LLVM_ENABLE_ZLIB)
  list(APPEND imported_libs ZLIB::ZLIB)
endif()

if(LLVM_ENABLE_ZSTD)
  if(TARGET zstd::libzstd_shared AND NOT LLVM_USE_STATIC_ZSTD)
    set(zstd_target zstd::libzstd_shared)
  else()
    set(zstd_target zstd::libzstd_static)
  endif()
endif()

if(LLVM_ENABLE_ZSTD)
  list(APPEND imported_libs ${zstd_target})
endif()

if( WIN32 )
  # libuuid required for FOLDERID_Profile usage in lib/Support/Windows/Path.inc.
  # advapi32 required for CryptAcquireContextW in lib/Support/Windows/Path.inc.
  # ntdll required for RtlGetLastNtStatus in lib/Support/ErrorHandling.cpp.
  set(system_libs ${system_libs} psapi shell32 ole32 uuid advapi32 ws2_32 ntdll)
  if( HAVE_WINDOWS_ICU )
    list(APPEND system_libs icu)
  endif()
elseif( CMAKE_HOST_UNIX )
  if( HAVE_LIBRT )
    set(system_libs ${system_libs} rt)
  endif()
  if( HAVE_LIBDL )
    set(system_libs ${system_libs} ${CMAKE_DL_LIBS})
  endif()
  if( HAVE_BACKTRACE AND NOT "${Backtrace_LIBRARIES}" STREQUAL "" )
    # On BSDs, CMake returns a fully qualified path to the backtrace library.
    # We need to remove the path and the 'lib' prefix, to make it look like a
    # regular short library name, suitable for appending to a -l link flag.
    get_filename_component(Backtrace_LIBFILE ${Backtrace_LIBRARIES} NAME_WE)
    STRING(REGEX REPLACE "^lib" "" Backtrace_LIBFILE ${Backtrace_LIBFILE})
    set(system_libs ${system_libs} ${Backtrace_LIBFILE})
  endif()
  set(system_libs ${system_libs} ${LLVM_ATOMIC_LIB})
  set(system_libs ${system_libs} ${LLVM_PTHREAD_LIB})
  if( UNIX AND NOT (BEOS OR HAIKU) )
    set(system_libs ${system_libs} m)
  endif()
  if( UNIX AND "${CMAKE_SYSTEM_NAME}" MATCHES "SunOS" )
    set(system_libs ${system_libs} kstat socket)
  endif()
  if( FUCHSIA )
    set(system_libs ${system_libs} zircon)
  endif()
  if ( HAIKU )
    add_compile_definitions(_BSD_SOURCE)
    set(system_libs ${system_libs} bsd network)
  endif()
endif( WIN32 )

set(WL "")
if(CMAKE_VERSION VERSION_GREATER_EQUAL "3.25"
   AND MSVC
   AND NOT CMAKE_GENERATOR MATCHES "Visual Studio")
  #IntelLLVM requires to pass linker flags with a wrapper
  set(WL "$<$<OR:$<LINK_LANG_AND_ID:C,IntelLLVM>,$<LINK_LANG_AND_ID:CXX,IntelLLVM>,$<LINK_LANG_AND_ID:Fortran,IntelLLVM>>:-Qoption,link,>")
endif()

# Delay load shell32.dll if possible to speed up process startup.
set (delayload_flags)
if (MSVC)
  # When linking with Swift, `swiftc.exe` is used as the linker drive rather
  # than invoking `link.exe` directly.  In such a case, the flags should be
  # marked as `-Xlinker` to pass them directly to the linker.  As a temporary
  # workaround simply elide the delay loading.
  set (delayload_flags $<$<NOT:$<LINK_LANGUAGE:Swift>>:delayimp ${WL}-delayload:shell32.dll ${WL}-delayload:ole32.dll>)
endif()

# Link Z3 if the user wants to build it.
if(LLVM_WITH_Z3)
  set(system_libs ${system_libs} ${Z3_LIBRARIES})
endif()

# Override the C runtime allocator on Windows and embed it into LLVM tools & libraries
if(LLVM_INTEGRATED_CRT_ALLOC)
  if (NOT CMAKE_MSVC_RUNTIME_LIBRARY OR CMAKE_MSVC_RUNTIME_LIBRARY MATCHES "DLL$")
    message(FATAL_ERROR "LLVM_INTEGRATED_CRT_ALLOC only works with CMAKE_MSVC_RUNTIME_LIBRARY set to MultiThreaded or MultiThreadedDebug.")
  endif()

  string(REGEX REPLACE "(/|\\\\)$" "" LLVM_INTEGRATED_CRT_ALLOC "${LLVM_INTEGRATED_CRT_ALLOC}")

  if(NOT EXISTS "${LLVM_INTEGRATED_CRT_ALLOC}")
    message(FATAL_ERROR "Cannot find the path to `git clone` for the CRT allocator! (${LLVM_INTEGRATED_CRT_ALLOC}). Currently, rpmalloc, snmalloc and mimalloc are supported.")
  endif()

  if((LLVM_INTEGRATED_CRT_ALLOC MATCHES "rpmalloc$") OR LLVM_ENABLE_RPMALLOC)
    add_compile_definitions(ENABLE_OVERRIDE ENABLE_PRELOAD)
    add_compile_definitions(LLVM_INTEGRATED_CRT_ALLOC="rpmalloc")
    set(ALLOCATOR_FILES "${LLVM_INTEGRATED_CRT_ALLOC}/rpmalloc/rpmalloc.c")
    list(APPEND delayload_flags "${WL}-INCLUDE:malloc")
  elseif(LLVM_INTEGRATED_CRT_ALLOC MATCHES "snmalloc$")
    add_compile_definitions(LLVM_INTEGRATED_CRT_ALLOC="snmalloc")
    set(ALLOCATOR_FILES "${LLVM_INTEGRATED_CRT_ALLOC}/src/snmalloc/override/new.cc")
    set(system_libs ${system_libs} "mincore.lib" "${WL}-INCLUDE:malloc")
  elseif(LLVM_INTEGRATED_CRT_ALLOC MATCHES "mimalloc$")
    add_compile_definitions(LLVM_INTEGRATED_CRT_ALLOC="mimalloc")
    set(MIMALLOC_LIB "${LLVM_INTEGRATED_CRT_ALLOC}/out/msvc-x64/Release/mimalloc-static.lib")
    if(NOT EXISTS "${MIMALLOC_LIB}")
	  message(FATAL_ERROR "Cannot find the mimalloc static library. To build it, first apply the patch from https://github.com/microsoft/mimalloc/issues/268 then build the Release x64 target through ${LLVM_INTEGRATED_CRT_ALLOC}\\ide\\vs2019\\mimalloc.sln")
    endif()
    set(system_libs ${system_libs} "${MIMALLOC_LIB}" "${WL}-INCLUDE:malloc")
  endif()
endif()

# FIXME: We are currently guarding AIX headers with _XOPEN_SOURCE=700.
# See llvm/CMakeLists.txt. However, we need _SC_NPROCESSORS_ONLN in
# unistd.h and it is guarded by _ALL_SOURCE, so we remove the _XOPEN_SOURCE
# guard here. We should remove the guards all together once AIX cleans up
# the system headers.
if (UNIX AND "${CMAKE_SYSTEM_NAME}" MATCHES "AIX")
  remove_definitions("-D_XOPEN_SOURCE=700")
endif()

add_subdirectory(BLAKE3)

# Targets that need the generated C++ fallback while the Rust option is on
# compile this copy, because the original source is marked header-only then.
set(UNICODE_CASE_FOLD_REFERENCE
  "${CMAKE_CURRENT_BINARY_DIR}/UnicodeCaseFoldReference.cpp")
configure_file("${CMAKE_CURRENT_SOURCE_DIR}/UnicodeCaseFold.cpp"
  "${UNICODE_CASE_FOLD_REFERENCE}" COPYONLY)

# Keep both sources in the component list for llvm_check_source_file_list, but
# compile only the selected one.
if(LLVM_ENABLE_RUST_UNICODE)
  set_source_files_properties(UnicodeCaseFold.cpp PROPERTIES HEADER_FILE_ONLY TRUE)
else()
  set_source_files_properties(UnicodeCaseFoldRust.cpp PROPERTIES HEADER_FILE_ONLY TRUE)
endif()

add_llvm_component_library(LLVMSupport
  ABIBreak.cpp
  AMDGPUMetadata.cpp
  APFixedPoint.cpp
  APFloat.cpp
  APInt.cpp
  APSInt.cpp
  ARMBuildAttributes.cpp
  AArch64AttributeParser.cpp
  AArch64BuildAttributes.cpp
  ARMAttributeParser.cpp
  ARMWinEH.cpp
  AllocToken.cpp
  Allocator.cpp
  AutoConvert.cpp
  Base64.cpp
  BalancedPartitioning.cpp
  BinaryStreamError.cpp
  BinaryStreamReader.cpp
  BinaryStreamRef.cpp
  BinaryStreamWriter.cpp
  BlockFrequency.cpp
  BranchProbability.cpp
  BuryPointer.cpp
  CachePruning.cpp
  Caching.cpp
  CHERICapabilityFormat.cpp
  circular_raw_ostream.cpp
  Chrono.cpp
  COM.cpp
  CodeGenCoverage.cpp
  CommandLine.cpp
  Compression.cpp
  CRC.cpp
  ConvertUTF.cpp
  ConvertEBCDIC.cpp
  ConvertUTFWrapper.cpp
  CrashRecoveryContext.cpp
  CSKYAttributes.cpp
  CSKYAttributeParser.cpp
  DataExtractor.cpp
  Debug.cpp
  DebugCounter.cpp
  DeltaAlgorithm.cpp
  DeltaTree.cpp
  DivisionByConstantInfo.cpp
  DAGDeltaAlgorithm.cpp
  DJB.cpp
  DXILABI.cpp
  DynamicAPInt.cpp
  ELFAttributes.cpp
  ELFAttrParserCompact.cpp
  ELFAttrParserExtended.cpp
  Error.cpp
  ErrorHandling.cpp
  ExponentialBackoff.cpp
  ExtensibleRTTI.cpp
  FileCollector.cpp
  FileUtilities.cpp
  FileOutputBuffer.cpp
  FloatingPointMode.cpp
  FoldingSet.cpp
  FormattedStream.cpp
  FormatVariadic.cpp
  GlobPattern.cpp
  GraphWriter.cpp
  HexagonAttributeParser.cpp
  HexagonAttributes.cpp
  InitLLVM.cpp
  InstructionCost.cpp
  IntEqClasses.cpp
  IntervalMap.cpp
  Jobserver.cpp
  JSON.cpp
  KnownBits.cpp
  KnownFPClass.cpp
  LEB128.cpp
  LineIterator.cpp
  Locale.cpp
  LockFileManager.cpp
  ManagedStatic.cpp
  MathExtras.cpp
  MemAlloc.cpp
  MemoryBuffer.cpp
  MemoryBufferRef.cpp
  ModRef.cpp
  MD5.cpp
  MSP430Attributes.cpp
  MSP430AttributeParser.cpp
  Mustache.cpp
  NativeFormatting.cpp
  OptimizedStructLayout.cpp
  Optional.cpp
  OptionStrCmp.cpp
  PGOOptions.cpp
  Parallel.cpp
  PluginLoader.cpp
  PrettyStackTrace.cpp
  RandomNumberGenerator.cpp
  IntegerInclusiveInterval.cpp
  Regex.cpp
  RewriteBuffer.cpp
  RewriteRope.cpp
  RISCVAttributes.cpp
  RISCVAttributeParser.cpp
  RISCVISAUtils.cpp
  ScaledNumber.cpp
  ScopedPrinter.cpp
  SHA1.cpp
  SHA256.cpp
  Signposts.cpp
  SipHash.cpp
  SlowDynamicAPInt.cpp
  SmallPtrSet.cpp
  SmallVector.cpp
  SourceMgr.cpp
  SpecialCaseList.cpp
  Statistic.cpp
  StringExtras.cpp
  StringMap.cpp
  StringSaver.cpp
  StringRef.cpp
  SuffixTreeNode.cpp
  SuffixTree.cpp
  SystemUtils.cpp
  TarWriter.cpp
  TextEncoding.cpp
  ThreadPool.cpp
  TimeProfiler.cpp
  Timer.cpp
  ToolOutputFile.cpp
  TrieRawHashMap.cpp
  Twine.cpp
  Unicode.cpp
  UnicodeCaseFold.cpp
  UnicodeCaseFoldRust.cpp
  UnicodeNameToCodepoint.cpp
  UnicodeNameToCodepointGenerated.cpp
  VersionTuple.cpp
  VirtualFileSystem.cpp
  VirtualOutputBackend.cpp
  VirtualOutputBackends.cpp
  VirtualOutputConfig.cpp
  VirtualOutputError.cpp
  VirtualOutputFile.cpp
  Win64EH.cpp
  WithColor.cpp
  YAMLParser.cpp
  YAMLTraits.cpp
  raw_os_ostream.cpp
  raw_ostream.cpp
  raw_ostream_proxy.cpp
  raw_socket_stream.cpp
  regcomp.c
  regerror.c
  regexec.c
  regfree.c
  regstrlcpy.c
  xxhash.cpp
  Z3Solver.cpp

  ${ALLOCATOR_FILES}
  $<TARGET_OBJECTS:LLVMSupportBlake3>

# System
  Atomic.cpp
  DynamicLibrary.cpp
  Errno.cpp
  Memory.cpp
  Path.cpp
  Process.cpp
  Program.cpp
  ProgramStack.cpp
  RWMutex.cpp
  Signals.cpp
  Threading.cpp
  Valgrind.cpp
  Watchdog.cpp
  zOSLibFunctions.cpp

  ADDITIONAL_HEADER_DIRS
  Unix
  Windows
  ${LLVM_MAIN_INCLUDE_DIR}/llvm/ADT
  ${LLVM_MAIN_INCLUDE_DIR}/llvm/Support
  ${Backtrace_INCLUDE_DIRS}

  PRECOMPILE_HEADERS
  [["llvm/Support/pch.h"]]

  LINK_LIBS
  ${system_libs} ${imported_libs} ${delayload_flags}

  LINK_COMPONENTS
  Demangle
  )

if(LLVM_ENABLE_RUST_SIPHASH)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_siphash
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/siphash/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_SIPHASH)
  target_link_libraries(LLVMSupport PRIVATE llvm_siphash)

  add_executable(llvm-rust-siphash-differential EXCLUDE_FROM_ALL
    rust/siphash/test/differential.cpp)
  target_include_directories(llvm-rust-siphash-differential PRIVATE
    ${LLVM_THIRD_PARTY_DIR}/siphash/include)
  target_link_libraries(llvm-rust-siphash-differential PRIVATE llvm_siphash)
  add_custom_target(check-llvm-rust-siphash
    COMMAND llvm-rust-siphash-differential
    DEPENDS llvm-rust-siphash-differential
    USES_TERMINAL)
endif()

add_executable(llvm-siphash-stable-hash EXCLUDE_FROM_ALL
  rust/siphash/test/stable-hash.cpp)
target_link_libraries(llvm-siphash-stable-hash PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_CRC)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_crc
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/crc/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_CRC)
  target_link_libraries(LLVMSupport PRIVATE llvm_crc)

  add_executable(llvm-rust-crc-differential EXCLUDE_FROM_ALL
    rust/crc/test/differential.cpp)
  target_link_libraries(llvm-rust-crc-differential PRIVATE llvm_crc)
  add_custom_target(check-llvm-rust-crc
    COMMAND llvm-rust-crc-differential
    DEPENDS llvm-rust-crc-differential
    USES_TERMINAL)
endif()

add_executable(llvm-crc-stable EXCLUDE_FROM_ALL rust/crc/test/stable.cpp)
target_link_libraries(llvm-crc-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_MD5)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_md5
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/md5/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_MD5)
  target_link_libraries(LLVMSupport PRIVATE llvm_md5)

  add_executable(llvm-rust-md5-differential EXCLUDE_FROM_ALL
    rust/md5/test/differential.cpp)
  target_link_libraries(llvm-rust-md5-differential PRIVATE llvm_md5)
  add_custom_target(check-llvm-rust-md5
    COMMAND llvm-rust-md5-differential
    DEPENDS llvm-rust-md5-differential
    USES_TERMINAL)
endif()

add_executable(llvm-md5-stable EXCLUDE_FROM_ALL rust/md5/test/stable.cpp)
target_link_libraries(llvm-md5-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_SHA1)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_sha1
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/sha1/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_SHA1)
  target_link_libraries(LLVMSupport PRIVATE llvm_sha1)

  add_executable(llvm-rust-sha1-differential EXCLUDE_FROM_ALL
    rust/sha1/test/differential.cpp)
  target_link_libraries(llvm-rust-sha1-differential PRIVATE llvm_sha1)
  add_custom_target(check-llvm-rust-sha1
    COMMAND llvm-rust-sha1-differential
    DEPENDS llvm-rust-sha1-differential
    USES_TERMINAL)
endif()

add_executable(llvm-sha1-stable EXCLUDE_FROM_ALL rust/sha1/test/stable.cpp)
target_link_libraries(llvm-sha1-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_SHA256)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_sha256
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/sha256/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_SHA256)
  target_link_libraries(LLVMSupport PRIVATE llvm_sha256)

  add_executable(llvm-rust-sha256-differential EXCLUDE_FROM_ALL
    rust/sha256/test/differential.cpp)
  target_link_libraries(llvm-rust-sha256-differential PRIVATE llvm_sha256)
  add_custom_target(check-llvm-rust-sha256
    COMMAND llvm-rust-sha256-differential
    DEPENDS llvm-rust-sha256-differential
    USES_TERMINAL)
endif()

add_executable(llvm-sha256-stable EXCLUDE_FROM_ALL rust/sha256/test/stable.cpp)
target_link_libraries(llvm-sha256-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_BASE64)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_base64
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/base64/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_BASE64)
  target_link_libraries(LLVMSupport PRIVATE llvm_base64)

  add_executable(llvm-rust-base64-differential EXCLUDE_FROM_ALL
    rust/base64/test/differential.cpp)
  target_link_libraries(llvm-rust-base64-differential PRIVATE LLVMSupport
    llvm_base64)
  add_custom_target(check-llvm-rust-base64
    COMMAND llvm-rust-base64-differential
    DEPENDS llvm-rust-base64-differential
    USES_TERMINAL)
endif()

add_executable(llvm-base64-stable EXCLUDE_FROM_ALL rust/base64/test/stable.cpp)
target_link_libraries(llvm-base64-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_CONVERTUTF)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_convertutf
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/convertutf/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_CONVERTUTF)
  target_link_libraries(LLVMSupport PRIVATE llvm_convertutf)

  add_executable(llvm-rust-convertutf-differential EXCLUDE_FROM_ALL
    rust/convertutf/test/differential.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/ConvertUTF.cpp")
  target_include_directories(llvm-rust-convertutf-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-convertutf-differential PRIVATE
    llvm_convertutf)
  add_custom_target(check-llvm-rust-convertutf
    COMMAND llvm-rust-convertutf-differential
    COMMAND llvm-convertutf-stable
    DEPENDS llvm-rust-convertutf-differential llvm-convertutf-stable
    USES_TERMINAL)
endif()

add_executable(llvm-convertutf-stable EXCLUDE_FROM_ALL
  rust/convertutf/test/stable.cpp)
target_link_libraries(llvm-convertutf-stable PRIVATE LLVMSupport)

# The standalone EBCDIC tests avoid linking LLVMSupport on 64-bit ELF by
# providing the one SmallVector out-of-line symbol they can reach. Other
# targets use the real support library.
if(UNIX AND NOT APPLE AND CMAKE_SIZEOF_VOID_P EQUAL 8)
  set(LLVM_EBCDIC_STANDALONE_SOURCES
    rust/convertebcdic/test/SmallVectorStub.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/ConvertEBCDIC.cpp")
  set(LLVM_EBCDIC_TEST_SUPPORT_LIBS)
else()
  set(LLVM_EBCDIC_STANDALONE_SOURCES)
  set(LLVM_EBCDIC_TEST_SUPPORT_LIBS LLVMSupport)
endif()

if(LLVM_ENABLE_RUST_CONVERTEBCDIC)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_convertebcdic
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/convertebcdic/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_CONVERTEBCDIC)
  target_link_libraries(LLVMSupport PRIVATE llvm_convertebcdic)

  add_executable(llvm-rust-convertebcdic-differential EXCLUDE_FROM_ALL
    rust/convertebcdic/test/differential.cpp
    rust/convertebcdic/test/ConvertEBCDICCpp.cpp
    ${LLVM_EBCDIC_STANDALONE_SOURCES})
  target_compile_definitions(llvm-rust-convertebcdic-differential PRIVATE
    LLVM_RUST_CONVERTEBCDIC)
  target_include_directories(llvm-rust-convertebcdic-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-convertebcdic-differential PRIVATE
    llvm_convertebcdic ${LLVM_EBCDIC_TEST_SUPPORT_LIBS})
  add_custom_target(check-llvm-rust-convertebcdic
    COMMAND llvm-rust-convertebcdic-differential
    DEPENDS llvm-rust-convertebcdic-differential
    USES_TERMINAL)
endif()

add_executable(llvm-convertebcdic-stable EXCLUDE_FROM_ALL
  rust/convertebcdic/test/stable.cpp
  ${LLVM_EBCDIC_STANDALONE_SOURCES})
target_include_directories(llvm-convertebcdic-stable PRIVATE
  ${LLVM_MAIN_INCLUDE_DIR}
  ${LLVM_BINARY_DIR}/include)
target_link_libraries(llvm-convertebcdic-stable PRIVATE
  ${LLVM_EBCDIC_TEST_SUPPORT_LIBS})
if(LLVM_ENABLE_RUST_CONVERTEBCDIC)
  target_compile_definitions(llvm-convertebcdic-stable PRIVATE
    LLVM_RUST_CONVERTEBCDIC)
  target_link_libraries(llvm-convertebcdic-stable PRIVATE llvm_convertebcdic)
endif()

if(LLVM_ENABLE_RUST_XXHASH)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_xxhash
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/xxhash/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_XXHASH)
  target_link_libraries(LLVMSupport PRIVATE llvm_xxhash)

  add_executable(llvm-rust-xxhash-differential EXCLUDE_FROM_ALL
    rust/xxhash/test/differential.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/xxhash.cpp")
  target_include_directories(llvm-rust-xxhash-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-xxhash-differential PRIVATE llvm_xxhash)
  add_custom_target(check-llvm-rust-xxhash
    COMMAND llvm-rust-xxhash-differential
    COMMAND llvm-xxhash-stable
    DEPENDS llvm-rust-xxhash-differential llvm-xxhash-stable
    USES_TERMINAL)
endif()

add_executable(llvm-xxhash-stable EXCLUDE_FROM_ALL
  rust/xxhash/test/stable.cpp)
target_link_libraries(llvm-xxhash-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_DJB)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_djb
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/djb/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_DJB)
  target_link_libraries(LLVMSupport PRIVATE llvm_djb)

  add_executable(llvm-rust-djb-differential EXCLUDE_FROM_ALL
    rust/djb/test/differential.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/DJB.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/ConvertUTF.cpp"
    "${UNICODE_CASE_FOLD_REFERENCE}"
    "${CMAKE_CURRENT_SOURCE_DIR}/ABIBreak.cpp")
  target_include_directories(llvm-rust-djb-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-djb-differential PRIVATE llvm_djb)
  add_custom_target(check-llvm-rust-djb
    COMMAND llvm-rust-djb-differential
    COMMAND llvm-djb-stable
    DEPENDS llvm-rust-djb-differential llvm-djb-stable
    USES_TERMINAL)
endif()

add_executable(llvm-djb-stable EXCLUDE_FROM_ALL
  rust/djb/test/stable.cpp)
target_link_libraries(llvm-djb-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_VERSIONTUPLE)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_versiontuple
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/versiontuple/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_VERSIONTUPLE)
  target_link_libraries(LLVMSupport PRIVATE llvm_versiontuple)

  add_executable(llvm-rust-versiontuple-differential EXCLUDE_FROM_ALL
    rust/versiontuple/test/differential.cpp
    rust/versiontuple/test/reference.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/ABIBreak.cpp")
  target_include_directories(llvm-rust-versiontuple-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-versiontuple-differential PRIVATE LLVMSupport)
  add_custom_target(check-llvm-rust-versiontuple
    COMMAND llvm-rust-versiontuple-differential
    COMMAND llvm-versiontuple-stable
    DEPENDS llvm-rust-versiontuple-differential llvm-versiontuple-stable
    USES_TERMINAL)
endif()

add_executable(llvm-versiontuple-stable EXCLUDE_FROM_ALL
  rust/versiontuple/test/stable.cpp)
target_link_libraries(llvm-versiontuple-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_OPTIONSTRCMP)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_optionstrcmp
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/optionstrcmp/Cargo.toml")
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_OPTIONSTRCMP)
  target_link_libraries(LLVMSupport PRIVATE llvm_optionstrcmp)

  add_executable(llvm-rust-optionstrcmp-differential EXCLUDE_FROM_ALL
    rust/optionstrcmp/test/differential.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/OptionStrCmp.cpp"
    rust/optionstrcmp/test/reference_stringref.cpp
    "${CMAKE_CURRENT_SOURCE_DIR}/ABIBreak.cpp")
  target_include_directories(llvm-rust-optionstrcmp-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-optionstrcmp-differential PRIVATE llvm_optionstrcmp)
  add_custom_target(check-llvm-rust-optionstrcmp
    COMMAND llvm-rust-optionstrcmp-differential
    COMMAND llvm-optionstrcmp-stable
    DEPENDS llvm-rust-optionstrcmp-differential llvm-optionstrcmp-stable
    USES_TERMINAL)
endif()

add_executable(llvm-optionstrcmp-stable EXCLUDE_FROM_ALL
  rust/optionstrcmp/test/stable.cpp)
target_link_libraries(llvm-optionstrcmp-stable PRIVATE LLVMSupport)

if(LLVM_ENABLE_RUST_NATIVEFORMATTING)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_nativeformatting
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/nativeformatting/Cargo.toml"
    STRIP_DEBUG)
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_NATIVEFORMATTING)
  target_link_libraries(LLVMSupport PRIVATE llvm_nativeformatting)

  add_executable(llvm-rust-nativeformatting-differential EXCLUDE_FROM_ALL
    rust/nativeformatting/test/differential.cpp)
  target_include_directories(llvm-rust-nativeformatting-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-nativeformatting-differential PRIVATE
    llvm_nativeformatting)
  add_custom_target(check-llvm-rust-nativeformatting
    COMMAND llvm-rust-nativeformatting-differential
    COMMAND llvm-nativeformatting-stable
    DEPENDS llvm-rust-nativeformatting-differential
            llvm-nativeformatting-stable
    USES_TERMINAL)
endif()

if(LLVM_ENABLE_RUST_UNICODE)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_unicode
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/unicode/Cargo.toml"
    STRIP_DEBUG PRUNE_ARCHIVE)
  target_compile_definitions(LLVMSupport PRIVATE LLVM_RUST_UNICODE)
  target_link_libraries(LLVMSupport PRIVATE llvm_unicode)

  add_executable(llvm-rust-unicode-differential EXCLUDE_FROM_ALL
    rust/unicode/test/differential.cpp
    "${UNICODE_CASE_FOLD_REFERENCE}")
  target_include_directories(llvm-rust-unicode-differential PRIVATE
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-unicode-differential PRIVATE llvm_unicode)

  add_custom_target(check-llvm-rust-unicode
    COMMAND llvm-rust-unicode-differential
    COMMAND llvm-unicode-stable
    DEPENDS llvm-rust-unicode-differential llvm-unicode-stable
    USES_TERMINAL)

  # Overflow checks make the crate reference core's panic routines.  The
  # pruning script must keep those providers instead of producing an archive
  # that only links with the default release flags.
  if(NOT MSVC)
    set(unicode_overflow_dir
      "${CMAKE_CURRENT_BINARY_DIR}/rust-unicode-overflow")
    set(unicode_overflow_archive
      "${unicode_overflow_dir}/release/${CMAKE_STATIC_LIBRARY_PREFIX}llvm_unicode${CMAKE_STATIC_LIBRARY_SUFFIX}")
    set(unicode_overflow_staged
      "${unicode_overflow_dir}/release/libllvm_unicode.pruned.a")
    set(unicode_prune_script
      "${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/modules/RustPruneArchive.cmake")
    file(GLOB_RECURSE unicode_overflow_sources CONFIGURE_DEPENDS
      "${CMAKE_CURRENT_SOURCE_DIR}/rust/unicode/src/*.rs"
      "${CMAKE_CURRENT_SOURCE_DIR}/rust/unicode/Cargo.lock")
    list(APPEND unicode_overflow_sources
      "${CMAKE_CURRENT_SOURCE_DIR}/rust/unicode/Cargo.toml")
    # Keep the test rule honest when LLVM_RUST_FLAGS changes, especially with
    # Unix Makefiles, which do not rerun a command just because its recipe did.
    set(unicode_overflow_flags
      "${CMAKE_CURRENT_BINARY_DIR}/rust-unicode-overflow-flags.txt")
    file(GENERATE OUTPUT "${unicode_overflow_flags}"
      CONTENT "${LLVM_RUST_FLAGS}\n")
    list(APPEND unicode_overflow_sources "${unicode_overflow_flags}")
    add_custom_command(OUTPUT "${unicode_overflow_archive}"
      COMMAND "${CMAKE_COMMAND}" -E env
              "RUSTFLAGS=${LLVM_RUST_FLAGS} -Coverflow-checks=yes"
              "${LLVM_RUST_CARGO_EXECUTABLE}" build --release --locked
              --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/rust/unicode/Cargo.toml"
              --target-dir "${unicode_overflow_dir}"
      DEPENDS ${unicode_overflow_sources}
      COMMENT "Building Unicode crate with overflow checks")
    add_custom_command(OUTPUT "${unicode_overflow_staged}"
      COMMAND "${CMAKE_COMMAND}" -E copy_if_different
              "${unicode_overflow_archive}" "${unicode_overflow_staged}"
      COMMAND "${CMAKE_COMMAND}"
              "-DARCHIVE=${unicode_overflow_staged}" "-DCRATE=llvm_unicode"
              "-DAR=${CMAKE_AR}" "-DNM=${CMAKE_NM}" -P "${unicode_prune_script}"
      DEPENDS "${unicode_overflow_archive}" "${unicode_prune_script}"
      COMMENT "Pruning overflow-checked Unicode archive")
    add_custom_target(llvm-rust-unicode-overflow-archive
      DEPENDS "${unicode_overflow_staged}")
    add_executable(llvm-rust-unicode-overflow-stable EXCLUDE_FROM_ALL
      rust/unicode/test/stable.cpp
      UnicodeCaseFoldRust.cpp)
    add_dependencies(llvm-rust-unicode-overflow-stable
      llvm-rust-unicode-overflow-archive)
    target_include_directories(llvm-rust-unicode-overflow-stable PRIVATE
      ${LLVM_MAIN_INCLUDE_DIR}
      ${LLVM_BINARY_DIR}/include)
    target_link_libraries(llvm-rust-unicode-overflow-stable PRIVATE
      "${unicode_overflow_staged}" ${CMAKE_DL_LIBS})
    if(UNIX)
      target_link_libraries(llvm-rust-unicode-overflow-stable PRIVATE m pthread)
    endif()
    add_custom_target(check-llvm-rust-unicode-overflow
      COMMAND llvm-rust-unicode-overflow-stable
      DEPENDS llvm-rust-unicode-overflow-stable
      USES_TERMINAL)
  endif()
endif()

add_executable(llvm-unicode-stable EXCLUDE_FROM_ALL
  rust/unicode/test/stable.cpp)
target_link_libraries(llvm-unicode-stable PRIVATE LLVMSupport)

add_custom_target(check-llvm-unicode
  COMMAND llvm-unicode-stable
  DEPENDS llvm-unicode-stable
  USES_TERMINAL)

add_executable(llvm-nativeformatting-stable EXCLUDE_FROM_ALL
  rust/nativeformatting/test/stable.cpp)
target_link_libraries(llvm-nativeformatting-stable PRIVATE LLVMSupport)

# Link ICU library if it is an external library.
if(ICU_FOUND AND NOT HAVE_WINDOWS_ICU)
  target_link_libraries(LLVMSupport
  PRIVATE
  ${ICU_LIBRARIES}
  )
endif()

set(llvm_system_libs ${system_libs})

# This block is only needed for llvm-config. When we deprecate llvm-config and
# move to using CMake export, this block can be removed.
if(LLVM_ENABLE_ZLIB)
  # CMAKE_BUILD_TYPE is only meaningful to single-configuration generators.
  if(CMAKE_BUILD_TYPE)
    string(TOUPPER ${CMAKE_BUILD_TYPE} build_type)
    get_property(zlib_library TARGET ZLIB::ZLIB PROPERTY LOCATION_${build_type})
  endif()
  if(NOT zlib_library)
    get_property(zlib_library TARGET ZLIB::ZLIB PROPERTY LOCATION)
  endif()
  get_library_name(${zlib_library} zlib_library)
  set(llvm_system_libs ${llvm_system_libs} "${zlib_library}")
endif()

if(LLVM_ENABLE_ZSTD)
  # CMAKE_BUILD_TYPE is only meaningful to single-configuration generators.
  if(CMAKE_BUILD_TYPE)
    string(TOUPPER ${CMAKE_BUILD_TYPE} build_type)
    get_property(zstd_library TARGET ${zstd_target} PROPERTY LOCATION_${build_type})
  endif()
  if(NOT zstd_library)
    get_property(zstd_library TARGET ${zstd_target} PROPERTY LOCATION)
  endif()
  if (zstd_target STREQUAL zstd::libzstd_shared)
    get_library_name(${zstd_library} zstd_library)
    set(llvm_system_libs ${llvm_system_libs} "${zstd_library}")
  else()
    set(llvm_system_libs ${llvm_system_libs} "${zstd_STATIC_LIBRARY}")
  endif()
endif()

set_property(TARGET LLVMSupport PROPERTY LLVM_SYSTEM_LIBS "${llvm_system_libs}")

# The Rust ports are static archives installed next to the other LLVM
# libraries, so downstream links need them in llvm-config --system-libs.
foreach(rust_port SIPHASH CRC MD5 SHA1 SHA256 BASE64 CONVERTUTF CONVERTEBCDIC
                  XXHASH RUSTDEMANGLE DLANG NATIVEFORMATTING UNICODE)
  if(LLVM_ENABLE_RUST_${rust_port})
    if(rust_port STREQUAL "DLANG")
      set(rust_port_lower "dlangdemangle")
    else()
      string(TOLOWER ${rust_port} rust_port_lower)
    endif()
    set_property(TARGET LLVMSupport APPEND PROPERTY LLVM_SYSTEM_LIBS
                 "llvm_${rust_port_lower}")
  endif()
endforeach()


if(LLVM_INTEGRATED_CRT_ALLOC)
  if(LLVM_INTEGRATED_CRT_ALLOC MATCHES "snmalloc$")
    set_property(TARGET LLVMSupport PROPERTY CXX_STANDARD 17)
    add_compile_definitions(_SILENCE_CXX17_ITERATOR_BASE_CLASS_DEPRECATION_WARNING)
    if ("${CMAKE_CXX_COMPILER_ID}" MATCHES "Clang" AND
        "${CMAKE_SYSTEM_PROCESSOR}" MATCHES "x86_64")
      set_property(TARGET LLVMSupport PROPERTY COMPILE_FLAGS "-mcx16")
    endif()
  endif()
endif()

if(LLVM_WITH_Z3)
  target_include_directories(LLVMSupport SYSTEM
    PRIVATE
    ${Z3_INCLUDE_DIR}
    )
endif()

target_include_directories(LLVMSupport
  PRIVATE
  ${LLVM_THIRD_PARTY_DIR}/siphash/include
  ${LLVM_SOURCE_DIR}/../libc
)

if(NOT MSVC)
  target_compile_options(LLVMSupport PRIVATE "-Wno-c99-extensions") # _Complex warnings.
endif()

# SupportLSP depends on Support and therefore must be included afterwards.
