//===-- UnicodeCaseFoldRust.cpp - Rust Unicode case folding -----*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Compiler.h"

// Unicode.h pulls in the whole Support header set. The declaration is repeated
// here so this one-line wrapper does not dominate the component's compile time.
namespace llvm::sys::unicode {
LLVM_ABI int foldCharSimple(int C);
}

extern "C" int llvm_unicode_fold_char_simple(int C);

namespace llvm::sys::unicode {
int foldCharSimple(int C) { return llvm_unicode_fold_char_simple(C); }
}
