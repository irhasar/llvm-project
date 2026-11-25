//===- VersionTuple.cpp - Version Number Handling ---------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file implements the VersionTuple class, which represents a version in
// the form major[.minor[.subminor]].
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/VersionTuple.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/raw_ostream.h"
#include <cassert>
#include <cstdint>

using namespace llvm;

std::string VersionTuple::getAsString() const {
  std::string Result;
  {
    llvm::raw_string_ostream Out(Result);
    Out << *this;
  }
  return Result;
}

raw_ostream &llvm::operator<<(raw_ostream &Out, const VersionTuple &V) {
  Out << V.getMajor();
  if (std::optional<unsigned> Minor = V.getMinor())
    Out << '.' << *Minor;
  if (std::optional<unsigned> Subminor = V.getSubminor())
    Out << '.' << *Subminor;
  if (std::optional<unsigned> Build = V.getBuild())
    Out << '.' << *Build;
  if (std::optional<unsigned> Subbuild = V.getSubbuild())
    Out << '.' << *Subbuild;
  return Out;
}

#if defined(LLVM_RUST_VERSIONTUPLE)
struct VersionTupleFields {
  uint32_t Major;
  uint32_t Minor;
  uint32_t Subminor;
  uint32_t Build;
  uint32_t Subbuild;
  uint32_t Flags;
};

extern "C" uint8_t llvm_versiontuple_try_parse(const uint8_t *Data, size_t Len,
                                                VersionTupleFields *Out);

bool VersionTuple::tryParse(StringRef Input) {
  VersionTupleFields Fields;
  if (llvm_versiontuple_try_parse(
          reinterpret_cast<const uint8_t *>(Input.data()), Input.size(),
          &Fields))
    return true;

  if (Fields.Flags & 8)
    *this = VersionTuple(Fields.Major, Fields.Minor, Fields.Subminor,
                         Fields.Build, Fields.Subbuild);
  else if (Fields.Flags & 4)
    *this = VersionTuple(Fields.Major, Fields.Minor, Fields.Subminor,
                         Fields.Build);
  else if (Fields.Flags & 2)
    *this = VersionTuple(Fields.Major, Fields.Minor, Fields.Subminor);
  else if (Fields.Flags & 1)
    *this = VersionTuple(Fields.Major, Fields.Minor);
  else
    *this = VersionTuple(Fields.Major);
  return false;
}
#else

static bool parseInt(StringRef &input, unsigned &value) {
  assert(value == 0);
  if (input.empty())
    return true;

  char next = input[0];
  input = input.substr(1);
  if (next < '0' || next > '9')
    return true;
  value = (unsigned)(next - '0');

  while (!input.empty()) {
    next = input[0];
    if (next < '0' || next > '9')
      return false;
    input = input.substr(1);
    value = value * 10 + (unsigned)(next - '0');
  }

  return false;
}

bool VersionTuple::tryParse(StringRef input) {
  unsigned major = 0, minor = 0, subminor = 0, build = 0, subbuild = 0;

  // Parse the major version, [0-9]+
  if (parseInt(input, major))
    return true;

  if (input.empty()) {
    *this = VersionTuple(major);
    return false;
  }

  // If we're not done, parse the minor version, \.[0-9]+
  if (input[0] != '.')
    return true;
  input = input.substr(1);
  if (parseInt(input, minor))
    return true;

  if (input.empty()) {
    *this = VersionTuple(major, minor);
    return false;
  }

  // If we're not done, parse the subminor version, \.[0-9]+
  if (!input.consume_front("."))
    return true;
  if (parseInt(input, subminor))
    return true;

  if (input.empty()) {
    *this = VersionTuple(major, minor, subminor);
    return false;
  }

  // If we're not done, parse the build version, \.[0-9]+
  if (!input.consume_front("."))
    return true;
  if (parseInt(input, build))
    return true;
  if (build >= 1024 * 1024)
    return true;

  if (input.empty()) {
    *this = VersionTuple(major, minor, subminor, build);
    return false;
  }

  // And the subbuild version, \.[0-9]+
  if (!input.consume_front("."))
    return true;
  if (parseInt(input, subbuild))
    return true;
  if (subbuild >= 1024)
    return true;

  // If we have characters left over, it's an error.
  if (!input.empty())
    return true;

  *this = VersionTuple(major, minor, subminor, build, subbuild);
  return false;
}
#endif

VersionTuple VersionTuple::withMajorReplaced(unsigned NewMajor) const {
  if (HasSubbuild)
    return VersionTuple(NewMajor, Minor, Subminor, Build, Subbuild);
  if (HasBuild)
    return VersionTuple(NewMajor, Minor, Subminor, Build);
  if (HasSubminor)
    return VersionTuple(NewMajor, Minor, Subminor);
  if (HasMinor)
    return VersionTuple(NewMajor, Minor);
  return VersionTuple(NewMajor);
}
