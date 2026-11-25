//===-- stable.cpp - DJB golden vectors -----------------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/DJB.h"

#include <cstdio>
#include <cstring>

namespace {

struct Case {
  const char *Text;
  uint32_t Expected;
};

} // namespace

int main() {
  const Case Cases[] = {
      {"", 5381},
      {"a", 177670},
      {"abc", 193485963},
      {"Hello", 261238937},
      {"hello", 261238937},
      {"ABC", 193485963},
      {"\xc3\x84", 5866508},
      {"\xc3\xa4", 5866508},
      {"\xce\x91", 5866884},
      {"\xce\xb1", 5866884},
      {"\xc4\xb0", 177678},
      {"\xc4\xb1", 177678},
      {"\xe2\x84\xaa", 177680},
      {"\xc5\xbf", 177688},
      {"DW_AT_name", 3698432468u},
      {"DW_AT_name_extra", 3753652599u},
  };

  for (const Case &C : Cases) {
    llvm::StringRef Text(C.Text);
    uint32_t Got = llvm::caseFoldingDjbHash(Text);
    if (Got != C.Expected) {
      std::fprintf(stderr, "caseFoldingDjbHash(%s) = %u, expected %u\n", C.Text,
                   Got, C.Expected);
      return 1;
    }
  }

  // Embedded NULs are part of the StringRef, not terminators.
  const char WithNul[] = {'a', '\0', 'b'};
  uint32_t Got = llvm::caseFoldingDjbHash(llvm::StringRef(WithNul, 3));
  uint32_t Expected = 5381;
  for (char C : WithNul)
    Expected = (Expected << 5) + Expected + static_cast<unsigned char>(C);
  if (Got != Expected) {
    std::fprintf(stderr, "embedded NUL: got %u, expected %u\n", Got, Expected);
    return 1;
  }

  std::printf("OK: DJB golden vectors\n");
}
