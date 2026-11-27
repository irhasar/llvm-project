//===-- StringExtrasRust.cpp - Rust StringExtras case conversion -*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/Compiler.h"

#include <cstddef>
#include <cstdint>
#include <string>

namespace llvm {
LLVM_ABI std::string convertToSnakeFromCamelCase(StringRef Input);
LLVM_ABI std::string convertToCamelFromSnakeCase(StringRef Input,
                                                 bool CapitalizeFirst);
} // namespace llvm

extern "C" size_t llvm_stringextras_snake(const char *Src, size_t Len,
                                          char *Dst);
extern "C" size_t llvm_stringextras_camel(const char *Src, size_t Len,
                                          char *Dst, uint8_t CapitalizeFirst);

#if defined(__GNUC__) || defined(__clang__)
// A Rust archive built with debug assertions or overflow checks pulls in
// core's panic routines, which reference this personality routine. The crate
// uses panic=abort, so unwinding never reaches it. Keep the definition weak so
// a real Rust runtime still wins; a second weak definition from another Rust
// port is harmless.
extern "C" __attribute__((weak)) void rust_eh_personality() {}
#endif

std::string llvm::convertToSnakeFromCamelCase(StringRef Input) {
  if (Input.empty())
    return "";

  std::string Result;
  Result.resize(Input.size() * 2);
  size_t Written =
      llvm_stringextras_snake(Input.data(), Input.size(), Result.data());
  Result.resize(Written);
  return Result;
}

std::string llvm::convertToCamelFromSnakeCase(StringRef Input,
                                              bool CapitalizeFirst) {
  if (Input.empty())
    return "";

  std::string Result;
  Result.resize(Input.size());
  size_t Written = llvm_stringextras_camel(Input.data(), Input.size(),
                                           Result.data(),
                                           CapitalizeFirst ? 1 : 0);
  Result.resize(Written);
  return Result;
}
