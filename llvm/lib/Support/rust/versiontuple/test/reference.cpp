//===-- reference.cpp - C++ VersionTuple parse oracle ---------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// The production VersionTuple.cpp also defines getAsString and operator<<,
// which need LLVMSupport. The differential target deliberately does not link
// LLVMSupport, so this file carries the original parse implementation alone.
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/VersionTuple.h"

#include <cassert>

using namespace llvm;

static bool parseInt(StringRef &Input, unsigned &Value) {
  assert(Value == 0);
  if (Input.empty())
    return true;

  char Next = Input[0];
  Input = Input.substr(1);
  if (Next < '0' || Next > '9')
    return true;
  Value = (unsigned)(Next - '0');

  while (!Input.empty()) {
    Next = Input[0];
    if (Next < '0' || Next > '9')
      return false;
    Input = Input.substr(1);
    Value = Value * 10 + (unsigned)(Next - '0');
  }

  return false;
}

bool VersionTuple::tryParse(StringRef Input) {
  unsigned Major = 0, Minor = 0, Subminor = 0, Build = 0, Subbuild = 0;

  if (parseInt(Input, Major))
    return true;

  if (Input.empty()) {
    *this = VersionTuple(Major);
    return false;
  }

  if (Input[0] != '.')
    return true;
  Input = Input.substr(1);
  if (parseInt(Input, Minor))
    return true;

  if (Input.empty()) {
    *this = VersionTuple(Major, Minor);
    return false;
  }

  if (!Input.consume_front("."))
    return true;
  if (parseInt(Input, Subminor))
    return true;

  if (Input.empty()) {
    *this = VersionTuple(Major, Minor, Subminor);
    return false;
  }

  if (!Input.consume_front("."))
    return true;
  if (parseInt(Input, Build))
    return true;
  if (Build >= 1024 * 1024)
    return true;

  if (Input.empty()) {
    *this = VersionTuple(Major, Minor, Subminor, Build);
    return false;
  }

  if (!Input.consume_front("."))
    return true;
  if (parseInt(Input, Subbuild))
    return true;
  if (Subbuild >= 1024)
    return true;

  if (!Input.empty())
    return true;

  *this = VersionTuple(Major, Minor, Subminor, Build, Subbuild);
  return false;
}
