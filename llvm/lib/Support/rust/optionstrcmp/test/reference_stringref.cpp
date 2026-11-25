//===-- reference_stringref.cpp - StringRef oracle ------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// The differential target does not link LLVMSupport, so provide the one
// StringRef entry point that OptionStrCmp.cpp uses.
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringRef.h"

using namespace llvm;

int StringRef::compare_insensitive(StringRef RHS) const {
  size_t Min = size() < RHS.size() ? size() : RHS.size();
  for (size_t I = 0; I != Min; ++I) {
    unsigned char Left = static_cast<unsigned char>(data()[I]);
    unsigned char Right = static_cast<unsigned char>(RHS.data()[I]);
    if (Left >= 'A' && Left <= 'Z')
      Left += 'a' - 'A';
    if (Right >= 'A' && Right <= 'Z')
      Right += 'a' - 'A';
    if (Left != Right)
      return Left < Right ? -1 : 1;
  }
  if (size() == RHS.size())
    return 0;
  return size() < RHS.size() ? -1 : 1;
}
