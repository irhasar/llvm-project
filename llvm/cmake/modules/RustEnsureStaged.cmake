if(NOT DEFINED SRC OR NOT DEFINED DST)
  message(FATAL_ERROR "RustEnsureStaged.cmake requires SRC and DST")
endif()

if(NOT EXISTS "${DST}")
  if(NOT EXISTS "${SRC}")
    message(FATAL_ERROR "Rust archive does not exist: ${SRC}")
  endif()
  execute_process(
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${SRC}" "${DST}"
    RESULT_VARIABLE result)
  if(NOT result EQUAL 0)
    message(FATAL_ERROR "Could not stage Rust archive: ${DST}")
  endif()
endif()
