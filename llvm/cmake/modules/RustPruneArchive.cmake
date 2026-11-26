if(NOT DEFINED ARCHIVE OR NOT DEFINED CRATE OR NOT DEFINED AR)
  message(FATAL_ERROR "RustPruneArchive.cmake requires ARCHIVE, CRATE and AR")
endif()

if(NOT EXISTS "${ARCHIVE}")
  message(FATAL_ERROR "Rust archive does not exist: ${ARCHIVE}")
endif()

execute_process(
  COMMAND "${AR}" t "${ARCHIVE}"
  OUTPUT_VARIABLE members
  RESULT_VARIABLE result)
if(NOT result EQUAL 0)
  message(FATAL_ERROR "Could not list Rust archive: ${ARCHIVE}")
endif()

string(REPLACE "\r\n" "\n" members "${members}")
string(REPLACE "\n" ";" members "${members}")
set(remove)
foreach(member IN LISTS members)
  if(member AND NOT member MATCHES "^${CRATE}[.-]")
    list(APPEND remove "${member}")
  endif()
endforeach()

if(remove)
  execute_process(
    COMMAND "${AR}" d "${ARCHIVE}" ${remove}
    RESULT_VARIABLE result)
  if(NOT result EQUAL 0)
    message(FATAL_ERROR "Could not prune Rust archive: ${ARCHIVE}")
  endif()
endif()
