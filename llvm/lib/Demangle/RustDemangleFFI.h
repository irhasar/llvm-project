//===--- RustDemangleFFI.h - Rust demangler FFI declarations ----*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Private declarations shared by the Rust demangler wrapper and its tests.
// The return type must match Rust's isize: intptr_t, not int64_t.
//
//===----------------------------------------------------------------------===//

#ifndef LLVM_LIB_DEMANGLE_RUSTDEMANGLEFFI_H
#define LLVM_LIB_DEMANGLE_RUSTDEMANGLEFFI_H

#include <cstddef>
#include <cstdint>
#include <string_view>

extern "C" std::intptr_t llvm_rustdemangle_demangle(
    const std::uint8_t *Input, std::size_t InputLen, std::uint8_t **Output);

namespace llvm {
namespace detail {

// Keep the benchmarked Rust path identical to llvm::rustDemangle, including
// the prefix check and the terminator write.
inline char *rustDemangleViaFFI(std::string_view MangledName) {
  if (MangledName.size() < 2 || MangledName[0] != '_' || MangledName[1] != 'R')
    return nullptr;

  std::uint8_t *Buffer = nullptr;
  std::intptr_t Need = llvm_rustdemangle_demangle(
      reinterpret_cast<const std::uint8_t *>(MangledName.data()),
      MangledName.size(), &Buffer);
  if (Need < 0)
    return nullptr;

  char *Result = reinterpret_cast<char *>(Buffer);
  Result[static_cast<std::size_t>(Need)] = '\0';
  return Result;
}

} // namespace detail
} // namespace llvm

#endif
