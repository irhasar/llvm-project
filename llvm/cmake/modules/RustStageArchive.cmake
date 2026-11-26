if(NOT DEFINED SRC OR NOT DEFINED DST OR NOT DEFINED STAMP)
  message(FATAL_ERROR "RustStageArchive.cmake requires SRC, DST and STAMP")
endif()

if(NOT EXISTS "${SRC}")
  message(FATAL_ERROR "Rust archive does not exist: ${SRC}")
endif()

set(copy FALSE)
if(NOT EXISTS "${DST}")
  set(copy TRUE)
else()
  file(SHA256 "${SRC}" src_hash)
  file(SHA256 "${DST}" dst_hash)
  if(NOT src_hash STREQUAL dst_hash)
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
  # Rename only after the copy finishes, so an interrupted copy cannot leave a
  # partial destination that the next build would accept.
  file(RENAME "${tmp}" "${DST}")
endif()

file(TOUCH "${STAMP}")
