//===-- DLangDemangleCpp.cpp - Renamed C++ reference demangler ------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Build the original C++ implementation under a second symbol so the
// differential harness can compare it with the public Rust-backed
// llvm::dlangDemangle in the same executable.
//
//===----------------------------------------------------------------------===//

#include <string>

#undef LLVM_RUST_DLANG
#define dlangDemangle dlangDemangleCpp
#include "../../../DLangDemangle.cpp"

namespace llvm {
bool dlangDemangleCppBytes(std::string_view MangledName,
                           std::string &Result) {
  if (MangledName.empty() || !starts_with(MangledName, "_D"))
    return false;

  OutputBuffer Demangled;
  if (MangledName == "_Dmain") {
    Demangled << "D main";
  } else {
    Demangler D(MangledName);
    const char *M = D.parseMangle(&Demangled);
    if (M == nullptr || *M != '\0') {
      std::free(Demangled.getBuffer());
      return false;
    }
  }

  if (Demangled.getCurrentPosition() == 0) {
    std::free(Demangled.getBuffer());
    return false;
  }

  Result.assign(Demangled.getBuffer(), Demangled.getCurrentPosition());
  std::free(Demangled.getBuffer());
  return true;
}
} // namespace llvm
