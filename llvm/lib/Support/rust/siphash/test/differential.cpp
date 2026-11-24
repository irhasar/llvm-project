//===-- differential.cpp - Rust vs C++ SipHash differential test ---------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Compares the Rust port (llvm_siphash_2_4_{64,128}) against the C++ reference
// implementation in third-party/siphash on published vectors and randomized
// inputs. Also reports rough throughput for both, to gate the migration on
// performance as well as correctness.
//
//===----------------------------------------------------------------------===//

#include "siphash/SipHash.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" {
void llvm_siphash_2_4_64(const uint8_t *Data, size_t Len, const uint8_t *Key,
                         uint8_t *Out);
void llvm_siphash_2_4_128(const uint8_t *Data, size_t Len, const uint8_t *Key,
                          uint8_t *Out);
}

namespace {

uint64_t NextRandom(uint64_t &State) {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

__attribute__((noinline)) void Reference64(const uint8_t *Data, size_t Len,
                                           const uint8_t *Key, uint8_t *Out) {
  siphash<2, 4>(Data, Len, *reinterpret_cast<const uint8_t(*)[16]>(Key),
                *reinterpret_cast<uint8_t(*)[8]>(Out));
}

__attribute__((noinline)) void Reference128(const uint8_t *Data, size_t Len,
                                            const uint8_t *Key, uint8_t *Out) {
  siphash<2, 4>(Data, Len, *reinterpret_cast<const uint8_t(*)[16]>(Key),
                *reinterpret_cast<uint8_t(*)[16]>(Out));
}

// Published SipHash-2-4 vectors: key = 00..0f, input = 00 01 02 ...
const uint64_t KnownVectors[] = {
    0x726fdb47dd0e0e31ULL, 0x74f839c593dc67fdULL, 0x0d6c8009d9a94f5aULL,
    0x85676696d7fb7e2dULL, 0xcf2794e0277187b7ULL, 0x18765564cd99a68dULL,
    0xcbc9466e58fee3ceULL, 0xab0200f58b01d137ULL, 0x93f5f5799a932462ULL,
    0x9e0082df0ba9e4b0ULL, 0x7a5dbbc594ddb9f3ULL, 0xf4b32f46226bada7ULL,
    0x751e8fbc860ee5fbULL, 0x14ea5627c0843d90ULL, 0xf723ca908e7af2eeULL,
    0xa129ca6149be45e5ULL,
};

int Failures = 0;

void ReportMismatch(const char *What, size_t Len, const uint8_t *Data,
                    const uint8_t *Key, const uint8_t *Expected,
                    const uint8_t *Actual, size_t OutLen) {
  std::fprintf(stderr, "MISMATCH (%s, len=%zu)\n  key=", What, Len);
  for (size_t I = 0; I < 16; ++I)
    std::fprintf(stderr, "%02x", Key[I]);
  std::fprintf(stderr, "\n  data=");
  for (size_t I = 0; I < Len && I < 32; ++I)
    std::fprintf(stderr, "%02x", Data[I]);
  std::fprintf(stderr, "\n  expected=");
  for (size_t I = 0; I < OutLen; ++I)
    std::fprintf(stderr, "%02x", Expected[I]);
  std::fprintf(stderr, "\n  actual  =");
  for (size_t I = 0; I < OutLen; ++I)
    std::fprintf(stderr, "%02x", Actual[I]);
  std::fprintf(stderr, "\n");
  ++Failures;
}

void CheckPublishedVectors() {
  uint8_t Key[16];
  uint8_t Input[16];
  for (int I = 0; I < 16; ++I) {
    Key[I] = static_cast<uint8_t>(I);
    Input[I] = static_cast<uint8_t>(I);
  }

  for (size_t Len = 0; Len < sizeof(KnownVectors) / sizeof(*KnownVectors);
       ++Len) {
    uint8_t Expected[8];
    uint8_t Reference[8];
    uint8_t Rust[8];
    uint64_t Bits = KnownVectors[Len];
    for (int I = 0; I < 8; ++I)
      Expected[I] = static_cast<uint8_t>(Bits >> (8 * I));

    Reference64(Input, Len, Key, Reference);
    llvm_siphash_2_4_64(Input, Len, Key, Rust);

    if (std::memcmp(Reference, Expected, 8) != 0) {
      std::fprintf(stderr,
                   "reference C++ implementation disagrees with published "
                   "vector at len=%zu\n",
                   Len);
      ++Failures;
    }
    if (std::memcmp(Rust, Expected, 8) != 0)
      ReportMismatch("published-64", Len, Input, Key, Expected, Rust, 8);
  }
  std::printf("published vectors: %zu checked\n",
              sizeof(KnownVectors) / sizeof(*KnownVectors));
}

void CheckRandomized() {
  constexpr size_t MaxLen = 512;
  std::vector<uint8_t> Data(MaxLen);
  uint64_t State = 0x9e3779b97f4a7c15ULL;
  size_t Cases = 0;

  for (size_t Len = 0; Len <= MaxLen; ++Len) {
    for (int Iter = 0; Iter < 512; ++Iter) {
      uint8_t Key[16];
      for (size_t I = 0; I < sizeof(Key); I += 8) {
        uint64_t Word = NextRandom(State);
        std::memcpy(Key + I, &Word, 8);
      }
      for (size_t I = 0; I < Len; ++I)
        Data[I] = static_cast<uint8_t>(NextRandom(State));

      uint8_t Ref64[8], Rust64[8];
      Reference64(Data.data(), Len, Key, Ref64);
      llvm_siphash_2_4_64(Data.data(), Len, Key, Rust64);
      if (std::memcmp(Ref64, Rust64, 8) != 0)
        ReportMismatch("random-64", Len, Data.data(), Key, Ref64, Rust64, 8);

      uint8_t Ref128[16], Rust128[16];
      Reference128(Data.data(), Len, Key, Ref128);
      llvm_siphash_2_4_128(Data.data(), Len, Key, Rust128);
      if (std::memcmp(Ref128, Rust128, 16) != 0)
        ReportMismatch("random-128", Len, Data.data(), Key, Ref128, Rust128, 16);

      ++Cases;
    }
  }
  std::printf("randomized differential: %zu cases\n", Cases);
}

volatile uint64_t Sink;

template <typename Fn>
double BenchNsPerOp(Fn Hash, const std::vector<uint8_t> &Data,
                    const uint8_t *Key, size_t Iterations) {
  auto Start = std::chrono::steady_clock::now();
  for (size_t I = 0; I < Iterations; ++I) {
    uint8_t Out[8];
    Hash(Data.data(), Data.size(), Key, Out);
    Sink += Out[0];
  }
  auto End = std::chrono::steady_clock::now();
  double Ns =
      std::chrono::duration_cast<std::chrono::nanoseconds>(End - Start).count();
  return Ns / static_cast<double>(Iterations);
}

void Benchmark() {
  uint8_t Key[16];
  for (int I = 0; I < 16; ++I)
    Key[I] = static_cast<uint8_t>(I * 7);

  std::printf("\n%-10s %14s %14s %8s\n", "input", "C++ ns/op", "Rust ns/op",
              "ratio");
  for (size_t Len : {size_t{8}, size_t{16}, size_t{32}, size_t{128}, size_t{1024}}) {
    std::vector<uint8_t> Data(Len, 0x5a);
    constexpr size_t Iterations = 2000000;
    double Cpp = BenchNsPerOp(Reference64, Data, Key, Iterations);
    double Rust = BenchNsPerOp(llvm_siphash_2_4_64, Data, Key, Iterations);
    std::printf("%-10zu %14.3f %14.3f %7.2fx\n", Len, Cpp, Rust, Cpp / Rust);
  }
}

} // namespace

int main() {
  CheckPublishedVectors();
  CheckRandomized();
  Benchmark();
  if (Failures) {
    std::fprintf(stderr, "FAILED: %d mismatches\n", Failures);
    return 1;
  }
  std::printf("OK: Rust SipHash is bit-identical to the C++ reference\n");
  return 0;
}
