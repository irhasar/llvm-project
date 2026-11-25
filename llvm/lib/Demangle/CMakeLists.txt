add_llvm_component_library(LLVMDemangle
  Demangle.cpp
  ItaniumDemangle.cpp
  MicrosoftDemangle.cpp
  MicrosoftDemangleNodes.cpp
  RustDemangle.cpp
  DLangDemangle.cpp

  ADDITIONAL_HEADER_DIRS
  "${LLVM_MAIN_INCLUDE_DIR}/llvm/Demangle"

)

if(LLVM_ENABLE_RUST_RUSTDEMANGLE)
  include(AddRustLibrary)
  add_llvm_rust_staticlib(llvm_rustdemangle
    "${CMAKE_CURRENT_SOURCE_DIR}/rust/rustdemangle/Cargo.toml"
    INSTALL_COMPONENT LLVMDemangle)
  target_compile_definitions(LLVMDemangle PRIVATE LLVM_RUST_RUSTDEMANGLE)
  target_link_libraries(LLVMDemangle PRIVATE llvm_rustdemangle)

  add_executable(llvm-rust-rustdemangle-differential EXCLUDE_FROM_ALL
    rust/rustdemangle/test/differential.cpp
    rust/rustdemangle/test/RustDemangleCpp.cpp
    RustDemangle.cpp
    ItaniumDemangle.cpp)
  target_compile_definitions(llvm-rust-rustdemangle-differential PRIVATE
    LLVM_RUST_RUSTDEMANGLE
    RUSTDEMANGLE_CORPUS="${CMAKE_CURRENT_SOURCE_DIR}/rust/rustdemangle/test/corpus.txt")
  target_include_directories(llvm-rust-rustdemangle-differential PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${LLVM_MAIN_INCLUDE_DIR}
    ${LLVM_BINARY_DIR}/include)
  target_link_libraries(llvm-rust-rustdemangle-differential PRIVATE
    llvm_rustdemangle)

  add_custom_target(check-llvm-rust-rustdemangle
    COMMAND llvm-rust-rustdemangle-differential
    COMMAND llvm-rustdemangle-stable
    DEPENDS llvm-rust-rustdemangle-differential llvm-rustdemangle-stable
    USES_TERMINAL)
endif()

add_executable(llvm-rustdemangle-stable EXCLUDE_FROM_ALL
  rust/rustdemangle/test/stable.cpp)
target_link_libraries(llvm-rustdemangle-stable PRIVATE LLVMDemangle)
