//===-- stable.cpp - Golden values for the public xxHash API ----*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/xxhash.h"

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

namespace {

int Failures = 0;

void Check64(const uint8_t *Data, size_t Len, uint64_t Expected,
             const char *What) {
  uint64_t Got = llvm::xxh3_64bits(Data, Len);
  if (Got != Expected) {
    std::fprintf(stderr, "%s: 64-bit got %016llx, expected %016llx\n", What,
                 (unsigned long long)Got, (unsigned long long)Expected);
    ++Failures;
  }
}

void Check128(const uint8_t *Data, size_t Len, uint64_t ExpectedLow,
              uint64_t ExpectedHigh, const char *What) {
  llvm::XXH128_hash_t Got = llvm::xxh3_128bits(Data, Len);
  if (Got.low64 != ExpectedLow || Got.high64 != ExpectedHigh) {
    std::fprintf(stderr,
                 "%s: 128-bit got %016llx%016llx, expected %016llx%016llx\n",
                 What, (unsigned long long)Got.high64,
                 (unsigned long long)Got.low64,
                 (unsigned long long)ExpectedHigh,
                 (unsigned long long)ExpectedLow);
    ++Failures;
  }
}

void CheckString(const char *Str, uint64_t H64, uint64_t Low, uint64_t High) {
  const uint8_t *Data = reinterpret_cast<const uint8_t *>(Str);
  size_t Len = std::strlen(Str);
  Check64(Data, Len, H64, Str);
  Check128(Data, Len, Low, High, Str);
}

} // namespace

int main() {
  CheckString("", 0x2d06800538d394c2ULL, 0x6001c324468d497fULL,
              0x99aa06d3014798d8ULL);
  CheckString("a", 0xe6c632b61e964e1fULL, 0xe6c632b61e964e1fULL,
              0xa96faf705af16834ULL);
  CheckString("abc", 0x78af5f94892f3950ULL, 0x78af5f94892f3950ULL,
              0x06b05ab6733a6185ULL);
  CheckString("message digest", 0x160d8e9329be94f9ULL, 0x0abfabecb8e3a424ULL,
              0x34ab715d95e3b649ULL);
  CheckString("The quick brown fox jumps over the lazy dog",
              0xce7d19a5418fb365ULL, 0x24a1cc2e3a8a7651ULL,
              0xddd650205ca3e7faULL);

  std::vector<uint8_t> Data(4096);
  for (size_t I = 0; I < Data.size(); ++I)
    Data[I] = static_cast<uint8_t>(I * 37 + 11);
  Check64(Data.data(), 768, 0x4f694eb6cca92fd8ULL, "pattern-768");
  Check128(Data.data(), 768, 0x4f694eb6cca92fd8ULL, 0x2aba07b0df8e5ba9ULL,
           "pattern-768");
  Check64(Data.data(), 1024, 0x7df7f049c0c1ad73ULL, "pattern-1024");
  Check128(Data.data(), 1024, 0x7df7f049c0c1ad73ULL, 0x02e7aa1347147456ULL,
           "pattern-1024");
  Check64(Data.data(), 4096, 0x95e9755b36d83075ULL, "pattern-4096");
  Check128(Data.data(), 4096, 0x95e9755b36d83075ULL, 0x02f5e20636c84421ULL,
           "pattern-4096");

  if (Failures) {
    std::fprintf(stderr, "%d xxHash stable checks failed\n", Failures);
    return 1;
  }
  std::printf("xxHash stable values OK\n");
  return 0;
}
