if(NOT DEFINED SRC OR NOT DEFINED STAMP)
  message(FATAL_ERROR "RustArchiveStamp.cmake requires SRC and STAMP")
endif()

if(NOT EXISTS "${SRC}")
  message(FATAL_ERROR "Rust archive does not exist: ${SRC}")
endif()

file(SHA256 "${SRC}" hash)
set(old "")
if(EXISTS "${STAMP}")
  file(READ "${STAMP}" old)
  string(STRIP "${old}" old)
endif()

if(NOT old STREQUAL hash)
  file(WRITE "${STAMP}" "${hash}\n")
endif()
