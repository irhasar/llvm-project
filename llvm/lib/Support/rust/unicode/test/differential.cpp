//===-- differential.cpp - Unicode fold differential test -------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Unicode.h"
#include <cstdio>

extern "C" int llvm_unicode_fold_char_simple(int C);

int main() {
  for (int C = -1; C <= 0x110000; ++C) {
    int Expected = llvm::sys::unicode::foldCharSimple(C);
    int Actual = llvm_unicode_fold_char_simple(C);
    if (Expected != Actual) {
      std::fprintf(stderr, "fold mismatch at %x: C++ %x, Rust %x\n", C,
                   Expected, Actual);
      return 1;
    }
  }
  std::puts("unicode fold differential: all code points match");
  return 0;
}
