//===-- stable.cpp - OptionStrCmp golden vectors ----------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/ArrayRef.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/OptionStrCmp.h"

#include <cstdio>

using namespace llvm;

namespace {

bool checkName(StringRef A, StringRef B, bool Fallback, int Expected) {
  int Actual = StrCmpOptionName(A, B, Fallback);
  if (Actual != Expected) {
    std::fprintf(stderr, "name mismatch: %d != %d\n", Actual, Expected);
    return false;
  }
  return true;
}

bool checkPrefixes(ArrayRef<StringRef> A, ArrayRef<StringRef> B,
                   int Expected) {
  int Actual = StrCmpOptionPrefixes(A, B);
  if (Actual != Expected) {
    std::fprintf(stderr, "prefix mismatch: %d != %d\n", Actual, Expected);
    return false;
  }
  return true;
}

} // namespace

int main() {
  if (!checkName("help", "HELP", true, 1) ||
      !checkName("help", "HELP", false, 0) ||
      !checkName("HELP", "help", true, -1) ||
      !checkName("abc", "ABD", true, -1) ||
      !checkName("ABD", "abc", true, 1) ||
      !checkName("abc", "abcd", true, 1) ||
      !checkName("abcd", "abc", true, -1) ||
      !checkName("", "a", true, 1) ||
      !checkName("a", "", true, -1) ||
      !checkName("", "", true, 0))
    return 1;

  const char HighA = static_cast<char>(0xff);
  const char HighB = static_cast<char>(0x7f);
  if (!checkName(StringRef(&HighA, 1), StringRef(&HighB, 1), true, 1))
    return 1;

  StringRef A1[] = {StringRef("--"), StringRef("debug"), StringRef("")};
  StringRef B1[] = {StringRef("--"), StringRef("DEBUG"), StringRef("")};
  if (!checkPrefixes(A1, B1, 1))
    return 1;

  StringRef A2[] = {StringRef("--")};
  StringRef B2[] = {StringRef("--"), StringRef("debug")};
  if (!checkPrefixes(A2, B2, 0))
    return 1;

  ArrayRef<StringRef> EmptyA;
  ArrayRef<StringRef> EmptyB;
  if (!checkPrefixes(EmptyA, EmptyB, 0))
    return 1;

  std::printf("OK: OptionStrCmp golden vectors\n");
}
