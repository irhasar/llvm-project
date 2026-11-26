if(NOT DEFINED SRC OR NOT DEFINED DST)
  message(FATAL_ERROR "RustEnsureStaged.cmake requires SRC and DST")
endif()

if(NOT EXISTS "${SRC}")
  message(FATAL_ERROR "Rust archive does not exist: ${SRC}")
endif()

set(copy FALSE)
if(NOT EXISTS "${DST}")
  set(copy TRUE)
else()
  # An interrupted copy leaves a short file. Replace it through a temporary
  # file so the next build cannot link a partial archive.
  file(SIZE "${SRC}" src_size)
  file(SIZE "${DST}" dst_size)
  if(NOT src_size EQUAL dst_size)
    set(copy TRUE)
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
