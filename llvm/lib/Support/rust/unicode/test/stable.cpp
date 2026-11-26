//===-- stable.cpp - Unicode fold golden test -------------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Unicode.h"
#include <cstdio>

static int check(int Input, int Expected) {
  int Actual = llvm::sys::unicode::foldCharSimple(Input);
  if (Actual != Expected) {
    std::fprintf(stderr, "fold(%x) = %x, expected %x\n", Input, Actual,
                 Expected);
    return 1;
  }
  return 0;
}

int main() {
  int Failures = 0;
  Failures += check(-1, -1);
  Failures += check(0x40, 0x40);
  Failures += check('A', 'a');
  Failures += check(0x00b5, 0x03bc);
  Failures += check(0x0130, 0x0130);
  Failures += check(0x0178, 0x00ff);
  Failures += check(0x1e900, 0x1e922);
  Failures += check(0x1e921, 0x1e943);
  Failures += check(0x110000, 0x110000);
  if (Failures)
    return 1;
  std::puts("unicode fold golden: all cases match");
  return 0;
}
