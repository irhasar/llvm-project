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
set(crate_members)
set(remove)
foreach(member IN LISTS members)
  if(member MATCHES "^${CRATE}[.-]")
    list(APPEND crate_members "${member}")
  elseif(member)
    list(APPEND remove "${member}")
  endif()
endforeach()

if(NOT crate_members)
  message(FATAL_ERROR "No archive members for crate ${CRATE}: ${ARCHIVE}")
endif()

# Removing the runtime members is only safe when the retained object does not
# need them. If nm is unavailable, or if the crate references any symbol we
# cannot account for, keep the complete archive: an oversized optional archive
# is preferable to a link failure with overflow checks or debug assertions on.
set(prune TRUE)
if(NOT DEFINED NM OR NOT NM)
  set(prune FALSE)
else()
  # nm exits nonzero when an archive member has no symbols; that is harmless,
  # so rely on the output rather than the exit status.
  execute_process(
    COMMAND "${NM}" --format=posix -A -u "${ARCHIVE}"
    OUTPUT_VARIABLE undefined
    ERROR_QUIET)
  if(undefined STREQUAL "")
    set(prune FALSE)
  else()
    string(REPLACE "\r\n" "\n" undefined "${undefined}")
    string(REPLACE "\n" ";" undefined "${undefined}")
    set(allowed_undefined
      abort bcmp memcmp memcpy memmove memset rust_eh_personality)
    set(parsed FALSE)
    foreach(line IN LISTS undefined)
      if(line MATCHES "^.*\\[([^]]+)\\]: ([^ ]+) U *$")
        set(parsed TRUE)
        set(member "${CMAKE_MATCH_1}")
        set(symbol "${CMAKE_MATCH_2}")
        if(member IN_LIST crate_members AND NOT symbol IN_LIST allowed_undefined)
          set(prune FALSE)
        endif()
      endif()
    endforeach()
    if(NOT parsed)
      set(prune FALSE)
    endif()
  endif()
endif()

if(prune AND remove)
  execute_process(
    COMMAND "${AR}" d "${ARCHIVE}" ${remove}
    RESULT_VARIABLE result)
  if(NOT result EQUAL 0)
    message(FATAL_ERROR "Could not prune Rust archive: ${ARCHIVE}")
  endif()
elseif(NOT prune)
  message(STATUS "Keeping complete Rust archive ${ARCHIVE}; the retained object needs runtime members")
endif()
