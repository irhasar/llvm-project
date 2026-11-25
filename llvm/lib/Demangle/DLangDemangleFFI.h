//===-- DLangDemangleFFI.h - FFI boundary for the Rust D demangler --------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#ifndef LLVM_LIB_DEMANGLE_DLANGDEMANGLEFFI_H
#define LLVM_LIB_DEMANGLE_DLANGDEMANGLEFFI_H

#include <cstdint>
#include <cstdlib>
#include <string_view>

extern "C" intptr_t llvm_dlangdemangle(const uint8_t *Input, uintptr_t InputLen,
                                       uint8_t **Output);

namespace llvm {
namespace detail {

inline char *dlangDemangleViaFFI(std::string_view MangledName) {
  uint8_t *Buffer = nullptr;
  intptr_t Need = llvm_dlangdemangle(
      reinterpret_cast<const uint8_t *>(MangledName.data()),
      MangledName.size(), &Buffer);
  if (Need < 0)
    return nullptr;
  if (Need == 0) {
    std::free(Buffer);
    return nullptr;
  }
  Buffer[Need] = '\0';
  return reinterpret_cast<char *>(Buffer);
}

} // namespace detail
} // namespace llvm

#endif
