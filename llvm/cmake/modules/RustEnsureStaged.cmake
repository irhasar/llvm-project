if(NOT DEFINED SRC OR NOT DEFINED DST)
  message(FATAL_ERROR "RustEnsureStaged.cmake requires SRC and DST")
endif()

set(copy FALSE)
if(NOT EXISTS "${DST}")
  if(NOT EXISTS "${SRC}")
    message(FATAL_ERROR "Rust archive does not exist: ${SRC}")
  endif()
  set(copy TRUE)
elseif(EXISTS "${SRC}")
  # An interrupted copy leaves a short file. Replace it through a temporary
  # file so the next build cannot link a partial archive.
  file(SIZE "${SRC}" src_size)
  file(SIZE "${DST}" dst_size)
  if(NOT src_size EQUAL dst_size)
    set(copy TRUE)
  endif()
else()
  # The Cargo output is gone; the staged archive is only usable if it matches
  # the hash recorded before that output disappeared.
  if(NOT DEFINED EXPECTED OR NOT EXISTS "${EXPECTED}")
    message(FATAL_ERROR "Cannot verify staged Rust archive without ${EXPECTED}")
  endif()
  file(READ "${EXPECTED}" expected_hash)
  string(STRIP "${expected_hash}" expected_hash)
  file(SHA256 "${DST}" dst_hash)
  if(NOT expected_hash STREQUAL dst_hash)
    message(FATAL_ERROR "Staged Rust archive does not match its recorded hash: ${DST}")
  endif()
endif()

if(copy)
  set(tmp "${DST}.tmp")
  file(REMOVE "${tmp}")
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E copy "${SRC}" "${tmp}"
    RESULT_VARIABLE result)
  if(NOT result EQUAL 0)
    message(FATAL_ERROR "Could not stage Rust archive: ${DST}")
  endif()
  file(RENAME "${tmp}" "${DST}")
endif()
