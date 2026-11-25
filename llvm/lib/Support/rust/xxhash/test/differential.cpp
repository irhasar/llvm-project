//===-- differential.cpp - C++ vs Rust xxHash differential ------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Compares the Rust XXH3 64-bit and 128-bit entry points, and their explicit
// scalar variants, with the C++ reference implementation in
// llvm/lib/Support/xxhash.cpp, then reports throughput for both dispatched
// entry points. The reference is compiled as a separate translation unit
// without LLVM_RUST_XXHASH, so this test never compares Rust with itself.
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/xxhash.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" uint64_t llvm_xxhash_xxh3_64bits(const uint8_t *Data, size_t Len);
extern "C" void llvm_xxhash_xxh3_128bits(const uint8_t *Data, size_t Len,
                                         uint64_t *Out);
extern "C" uint64_t llvm_xxhash_xxh3_64bits_scalar(const uint8_t *Data,
                                                   size_t Len);
extern "C" void llvm_xxhash_xxh3_128bits_scalar(const uint8_t *Data, size_t Len,
                                                uint64_t *Out);

using Clock = std::chrono::steady_clock;

namespace {

volatile uint64_t Sink;

int Failures = 0;

void Fail(const char *What, size_t Len) {
  if (Failures < 20)
    std::fprintf(stderr, "mismatch: %s len=%zu\n", What, Len);
  ++Failures;
}

bool Check(const uint8_t *Data, size_t Len) {
  uint64_t Cpp64 = llvm::xxh3_64bits(Data, Len);
  uint64_t Rust64 = llvm_xxhash_xxh3_64bits(Data, Len);
  uint64_t Rust64Scalar = llvm_xxhash_xxh3_64bits_scalar(Data, Len);
  llvm::XXH128_hash_t Cpp128 = llvm::xxh3_128bits(Data, Len);
  uint64_t Rust128[2] = {0, 0};
  uint64_t Rust128Scalar[2] = {0, 0};
  llvm_xxhash_xxh3_128bits(Data, Len, Rust128);
  llvm_xxhash_xxh3_128bits_scalar(Data, Len, Rust128Scalar);
  bool Ok = true;
  if (Cpp64 != Rust64) {
    Fail("64-bit", Len);
    Ok = false;
  }
  // The scalar entry point bypasses AVX2/NEON dispatch, so the fallback is
  // covered on hosts where the vector path is selected.
  if (Cpp64 != Rust64Scalar) {
    Fail("64-bit scalar", Len);
    Ok = false;
  }
  if (Cpp128.low64 != Rust128[0] || Cpp128.high64 != Rust128[1]) {
    Fail("128-bit", Len);
    Ok = false;
  }
  if (Cpp128.low64 != Rust128Scalar[0] || Cpp128.high64 != Rust128Scalar[1]) {
    Fail("128-bit scalar", Len);
    Ok = false;
  }
  return Ok;
}

template <typename Fn> double Time(Fn &&F, size_t Iters) {
  double Best = 1e30;
  for (int Rep = 0; Rep < 3; ++Rep) {
    auto Start = Clock::now();
    for (size_t I = 0; I < Iters; ++I)
      Sink += F();
    auto End = Clock::now();
    double Ns = std::chrono::duration<double, std::nano>(End - Start).count();
    Best = std::min(Best, Ns / Iters);
  }
  return Best;
}

struct BenchCase {
  size_t Len;
  size_t Iters;
};

} // namespace

int main(int argc, char **argv) {
  std::vector<uint8_t> Data(1 << 20);
  uint64_t State = 0x243F6A8885A308D3ULL;
  auto Next = [&]() {
    State ^= State << 13;
    State ^= State >> 7;
    State ^= State << 17;
    return State;
  };
  for (auto &Byte : Data)
    Byte = static_cast<uint8_t>(Next());

  for (size_t Len = 0; Len <= 2048; ++Len)
    Check(Data.data(), Len);
  for (size_t Trial = 0; Trial < 20000; ++Trial)
    Check(Data.data(), Next() % 4096);
  for (size_t Len : {4096u, 65536u, 1u << 20})
    Check(Data.data(), Len);

  if (Failures) {
    std::fprintf(stderr, "%d mismatches\n", Failures);
    return 1;
  }
  if (argc > 1 && std::strcmp(argv[1], "--check-only") == 0) {
    std::printf("OK: Rust xxHash is bit-identical to the C++ reference\n");
    return 0;
  }

  const BenchCase Cases[] = {{12, 20000000}, {48, 10000000}, {192, 5000000},
                             {768, 2000000}, {4096, 300000},
                             {65536, 30000}, {1u << 20, 2000}};
  std::printf("%8s %10s %10s %7s %10s %10s %7s\n", "size", "cpp64", "rust64",
              "x64", "cpp128", "rust128", "x128");
  for (const BenchCase &Case : Cases) {
    const uint8_t *Ptr = Data.data();
    // Verify the exact timed inputs before timing them.
    Check(Ptr, Case.Len);
    double Cpp64 = Time([&] { return llvm::xxh3_64bits(Ptr, Case.Len); },
                        Case.Iters);
    double Rust64 =
        Time([&] { return llvm_xxhash_xxh3_64bits(Ptr, Case.Len); }, Case.Iters);
    uint64_t Out[2];
    double Cpp128 = Time(
        [&] {
          llvm::XXH128_hash_t H = llvm::xxh3_128bits(Ptr, Case.Len);
          return H.low64 + H.high64;
        },
        Case.Iters);
    double Rust128 = Time(
        [&] {
          llvm_xxhash_xxh3_128bits(Ptr, Case.Len, Out);
          return Out[0] + Out[1];
        },
        Case.Iters);
    std::printf("%8zu %10.1f %10.1f %6.2fx %10.1f %10.1f %6.2fx\n", Case.Len,
                Cpp64, Rust64, Cpp64 / Rust64, Cpp128, Rust128,
                Cpp128 / Rust128);
  }

  if (Failures) {
    std::fprintf(stderr, "%d mismatches\n", Failures);
    return 1;
  }
  std::printf("OK: Rust xxHash is bit-identical to the C++ reference\n");
  return 0;
}
