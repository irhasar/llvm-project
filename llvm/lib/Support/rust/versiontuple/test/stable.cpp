//===-- stable.cpp - VersionTuple golden vectors ----------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/VersionTuple.h"

#include <cstdio>

using namespace llvm;

namespace {

struct Case {
  const char *Text;
  unsigned Major;
  unsigned Minor;
  unsigned Subminor;
  unsigned Build;
  unsigned Subbuild;
};

bool hasMinor(const VersionTuple &V) { return V.getMinor().has_value(); }
bool hasSubminor(const VersionTuple &V) { return V.getSubminor().has_value(); }
bool hasBuild(const VersionTuple &V) { return V.getBuild().has_value(); }
bool hasSubbuild(const VersionTuple &V) { return V.getSubbuild().has_value(); }

} // namespace

int main() {
  const Case Cases[] = {
      {"0", 0, 0, 0, 0, 0},
      {"1", 1, 0, 0, 0, 0},
      {"17.2", 17, 2, 0, 0, 0},
      {"17.2.3", 17, 2, 3, 0, 0},
      {"17.2.3.4", 17, 2, 3, 4, 0},
      {"17.2.3.4.5", 17, 2, 3, 4, 5},
      {"0.0.0.0.0", 0, 0, 0, 0, 0},
      {"1.2.3.1048575.1023", 1, 2, 3, 1048575, 1023},
      {"4294967295.4294967295.4294967295.1048575.1023", 4294967295u,
       2147483647u, 2147483647u, 1048575, 1023},
  };

  for (const Case &C : Cases) {
    VersionTuple V;
    if (V.tryParse(C.Text)) {
      std::fprintf(stderr, "unexpected parse failure: %s\n", C.Text);
      return 1;
    }
    if (V.asTuple() != std::tuple(C.Major, C.Minor, C.Subminor, C.Build,
                                  C.Subbuild)) {
      std::fprintf(stderr, "wrong value: %s\n", C.Text);
      return 1;
    }
  }

  // Component presence must survive the round trip exactly.
  {
    VersionTuple V;
    if (V.tryParse("1.2") || !hasMinor(V) || hasSubminor(V) || hasBuild(V) ||
        hasSubbuild(V))
      return 1;
  }
  {
    VersionTuple V;
    if (V.tryParse("1.2.3") || !hasMinor(V) || !hasSubminor(V) ||
        hasBuild(V) || hasSubbuild(V))
      return 1;
  }
  {
    VersionTuple V;
    if (V.tryParse("1.2.3.4") || !hasMinor(V) || !hasSubminor(V) ||
        !hasBuild(V) || hasSubbuild(V))
      return 1;
  }
  {
    VersionTuple V;
    if (V.tryParse("1.2.3.4.5") || !hasMinor(V) || !hasSubminor(V) ||
        !hasBuild(V) || !hasSubbuild(V))
      return 1;
  }

  const char *Bad[] = {"", ".", "1.", "1..2", "1.2.", "1.2.3.",
                       "1.2.3.4.", "1.2.3.4.5.6", "1.2.3.4x", "a",
                       "1.2.3.1048576", "1.2.3.4.1024"};
  for (const char *Text : Bad) {
    VersionTuple V(9, 9, 9);
    if (!V.tryParse(Text) || V.asTuple() != std::tuple(9u, 9u, 9u, 0u, 0u)) {
      std::fprintf(stderr, "unexpected parse success or mutation: %s\n", Text);
      return 1;
    }
  }

  // Embedded NUL is not a terminator for StringRef.
  const char WithNul[] = {'1', '.', '2', '\0', 'x'};
  VersionTuple V;
  if (!V.tryParse(StringRef(WithNul, sizeof(WithNul))))
    return 1;

  std::printf("OK: VersionTuple golden vectors\n");
}
