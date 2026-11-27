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

  // The common short case fits in a stack buffer; this avoids zero-filling a
  // 2 * size std::string before Rust overwrites it.
  constexpr size_t StackSize = 128;
  if (Input.size() <= StackSize / 2) {
    char Buffer[StackSize];
    size_t Written =
        llvm_stringextras_snake(Input.data(), Input.size(), Buffer);
    return std::string(Buffer, Written);
  }

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

  // A branchless ASCII lower-to-upper conversion avoids a mispredicted branch
  // on the first byte, which is the only change most snake_case inputs need.
  auto UpperFirst = [](char &C) {
    unsigned char Byte = static_cast<unsigned char>(C);
    bool Lower = static_cast<unsigned>(Byte - 'a') < 26;
    Byte -= static_cast<unsigned char>(Lower) * ('a' - 'A');
    C = static_cast<char>(Byte);
  };

  if (Input.size() == 1) {
    char C = Input[0];
    if (CapitalizeFirst)
      UpperFirst(C);
    return std::string(1, C);
  }

  std::string Result(Input.data(), Input.size());
  // Most snake_case names have no underscore; copying the input and fixing the
  // first byte avoids entering the Rust converter entirely.
  if (Input.find('_') == StringRef::npos) {
    if (CapitalizeFirst)
      UpperFirst(Result[0]);
    return Result;
  }
  size_t Written = llvm_stringextras_camel(Input.data(), Input.size(),
                                           Result.data(),
                                           CapitalizeFirst ? 1 : 0);
  Result.resize(Written);
  return Result;
}
