//===-- UnicodeCaseFoldRust.cpp - Rust Unicode case folding -----*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Unicode.h"

extern "C" int llvm_unicode_fold_char_simple(int C);

int llvm::sys::unicode::foldCharSimple(int C) {
  return llvm_unicode_fold_char_simple(C);
}
