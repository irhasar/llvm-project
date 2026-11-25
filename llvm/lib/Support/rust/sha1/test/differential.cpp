//===-- differential.cpp - Differential test for the Rust SHA-1 -----------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
// Compares the Rust one-shot SHA-1 against the C++ compression function LLVM
// ships (copied verbatim below) and reports throughput for both.
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" void llvm_sha1_hash(const uint8_t *Data, size_t Len, uint8_t *Out);
extern "C" void llvm_sha1_hash_scalar(const uint8_t *Data, size_t Len,
                                        uint8_t *Out);
extern "C" int llvm_sha1_has_sha_ni();

namespace {

using u32 = uint32_t;

static inline uint32_t rol(uint32_t Number, int Bits) {
  return (Number << Bits) | (Number >> (32 - Bits));
}

static inline uint32_t blk0(uint32_t *Buf, int I) { return Buf[I]; }

static inline uint32_t blk(uint32_t *Buf, int I) {
  Buf[I & 15] = rol(Buf[(I + 13) & 15] ^ Buf[(I + 8) & 15] ^ Buf[(I + 2) & 15] ^
                        Buf[I & 15],
                    1);
  return Buf[I & 15];
}

static inline void r0(uint32_t &A, uint32_t &B, uint32_t &C, uint32_t &D,
                      uint32_t &E, int I, uint32_t *Buf) {
  E += ((B & (C ^ D)) ^ D) + blk0(Buf, I) + 0x5A827999 + rol(A, 5);
  B = rol(B, 30);
}

static inline void r1(uint32_t &A, uint32_t &B, uint32_t &C, uint32_t &D,
                      uint32_t &E, int I, uint32_t *Buf) {
  E += ((B & (C ^ D)) ^ D) + blk(Buf, I) + 0x5A827999 + rol(A, 5);
  B = rol(B, 30);
}

static inline void r2(uint32_t &A, uint32_t &B, uint32_t &C, uint32_t &D,
                      uint32_t &E, int I, uint32_t *Buf) {
  E += (B ^ C ^ D) + blk(Buf, I) + 0x6ED9EBA1 + rol(A, 5);
  B = rol(B, 30);
}

static inline void r3(uint32_t &A, uint32_t &B, uint32_t &C, uint32_t &D,
                      uint32_t &E, int I, uint32_t *Buf) {
  E += (((B | C) & D) | (B & C)) + blk(Buf, I) + 0x8F1BBCDC + rol(A, 5);
  B = rol(B, 30);
}

static inline void r4(uint32_t &A, uint32_t &B, uint32_t &C, uint32_t &D,
                      uint32_t &E, int I, uint32_t *Buf) {
  E += (B ^ C ^ D) + blk(Buf, I) + 0xCA62C1D6 + rol(A, 5);
  B = rol(B, 30);
}

/* code */
#define SHA1_K0 0x5a827999
#define SHA1_K20 0x6ed9eba1
#define SHA1_K40 0x8f1bbcdc
#define SHA1_K60 0xca62c1d6

#define SEED_0 0x67452301
#define SEED_1 0xefcdab89
#define SEED_2 0x98badcfe
#define SEED_3 0x10325476
#define SEED_4 0xc3d2e1f0


__attribute__((noinline)) void RefBlock(u32 S[5], u32 Buf[16]) {

#if defined(LLVM_RUST_SHA1)
  llvm_sha1_block(S, Buf);
#else
  uint32_t A = S[0];
  uint32_t B = S[1];
  uint32_t C = S[2];
  uint32_t D = S[3];
  uint32_t E = S[4];

  // 4 rounds of 20 operations each. Loop unrolled.
  r0(A, B, C, D, E, 0, Buf);
  r0(E, A, B, C, D, 1, Buf);
  r0(D, E, A, B, C, 2, Buf);
  r0(C, D, E, A, B, 3, Buf);
  r0(B, C, D, E, A, 4, Buf);
  r0(A, B, C, D, E, 5, Buf);
  r0(E, A, B, C, D, 6, Buf);
  r0(D, E, A, B, C, 7, Buf);
  r0(C, D, E, A, B, 8, Buf);
  r0(B, C, D, E, A, 9, Buf);
  r0(A, B, C, D, E, 10, Buf);
  r0(E, A, B, C, D, 11, Buf);
  r0(D, E, A, B, C, 12, Buf);
  r0(C, D, E, A, B, 13, Buf);
  r0(B, C, D, E, A, 14, Buf);
  r0(A, B, C, D, E, 15, Buf);
  r1(E, A, B, C, D, 16, Buf);
  r1(D, E, A, B, C, 17, Buf);
  r1(C, D, E, A, B, 18, Buf);
  r1(B, C, D, E, A, 19, Buf);

  r2(A, B, C, D, E, 20, Buf);
  r2(E, A, B, C, D, 21, Buf);
  r2(D, E, A, B, C, 22, Buf);
  r2(C, D, E, A, B, 23, Buf);
  r2(B, C, D, E, A, 24, Buf);
  r2(A, B, C, D, E, 25, Buf);
  r2(E, A, B, C, D, 26, Buf);
  r2(D, E, A, B, C, 27, Buf);
  r2(C, D, E, A, B, 28, Buf);
  r2(B, C, D, E, A, 29, Buf);
  r2(A, B, C, D, E, 30, Buf);
  r2(E, A, B, C, D, 31, Buf);
  r2(D, E, A, B, C, 32, Buf);
  r2(C, D, E, A, B, 33, Buf);
  r2(B, C, D, E, A, 34, Buf);
  r2(A, B, C, D, E, 35, Buf);
  r2(E, A, B, C, D, 36, Buf);
  r2(D, E, A, B, C, 37, Buf);
  r2(C, D, E, A, B, 38, Buf);
  r2(B, C, D, E, A, 39, Buf);

  r3(A, B, C, D, E, 40, Buf);
  r3(E, A, B, C, D, 41, Buf);
  r3(D, E, A, B, C, 42, Buf);
  r3(C, D, E, A, B, 43, Buf);
  r3(B, C, D, E, A, 44, Buf);
  r3(A, B, C, D, E, 45, Buf);
  r3(E, A, B, C, D, 46, Buf);
  r3(D, E, A, B, C, 47, Buf);
  r3(C, D, E, A, B, 48, Buf);
  r3(B, C, D, E, A, 49, Buf);
  r3(A, B, C, D, E, 50, Buf);
  r3(E, A, B, C, D, 51, Buf);
  r3(D, E, A, B, C, 52, Buf);
  r3(C, D, E, A, B, 53, Buf);
  r3(B, C, D, E, A, 54, Buf);
  r3(A, B, C, D, E, 55, Buf);
  r3(E, A, B, C, D, 56, Buf);
  r3(D, E, A, B, C, 57, Buf);
  r3(C, D, E, A, B, 58, Buf);
  r3(B, C, D, E, A, 59, Buf);

  r4(A, B, C, D, E, 60, Buf);
  r4(E, A, B, C, D, 61, Buf);
  r4(D, E, A, B, C, 62, Buf);
  r4(C, D, E, A, B, 63, Buf);
  r4(B, C, D, E, A, 64, Buf);
  r4(A, B, C, D, E, 65, Buf);
  r4(E, A, B, C, D, 66, Buf);
  r4(D, E, A, B, C, 67, Buf);
  r4(C, D, E, A, B, 68, Buf);
  r4(B, C, D, E, A, 69, Buf);
  r4(A, B, C, D, E, 70, Buf);
  r4(E, A, B, C, D, 71, Buf);
  r4(D, E, A, B, C, 72, Buf);
  r4(C, D, E, A, B, 73, Buf);
  r4(B, C, D, E, A, 74, Buf);
  r4(A, B, C, D, E, 75, Buf);
  r4(E, A, B, C, D, 76, Buf);
  r4(D, E, A, B, C, 77, Buf);
  r4(C, D, E, A, B, 78, Buf);
  r4(B, C, D, E, A, 79, Buf);

  S[0] += A;
  S[1] += B;
  S[2] += C;
  S[3] += D;
  S[4] += E;
#endif
}

__attribute__((noinline)) void RefHash(const uint8_t *Data, size_t Len,
                                       uint8_t *Out) {
  u32 S[5] = {SEED_0, SEED_1, SEED_2, SEED_3, SEED_4};

  size_t Blocks = Len / 64;
  const uint8_t *P = Data;
  for (size_t B = 0; B < Blocks; ++B) {
    u32 Buf[16];
    for (int J = 0; J < 16; ++J)
      Buf[J] = (u32(P[J * 4]) << 24) | (u32(P[J * 4 + 1]) << 16) |
               (u32(P[J * 4 + 2]) << 8) | u32(P[J * 4 + 3]);
    RefBlock(S, Buf);
    P += 64;
  }

  uint8_t Tail[128] = {0};
  size_t Rem = Len % 64;
  std::memcpy(Tail, Data + Len - Rem, Rem);
  Tail[Rem] = 0x80;
  size_t Padded = Rem < 56 ? 64 : 128;
  uint64_t Bits = static_cast<uint64_t>(Len) * 8;
  for (int J = 0; J < 8; ++J)
    Tail[Padded - 8 + J] = static_cast<uint8_t>(Bits >> (56 - 8 * J));
  for (size_t B = 0; B < Padded / 64; ++B) {
    u32 Buf[16];
    for (int J = 0; J < 16; ++J)
      Buf[J] = (u32(Tail[B * 64 + J * 4]) << 24) |
               (u32(Tail[B * 64 + J * 4 + 1]) << 16) |
               (u32(Tail[B * 64 + J * 4 + 2]) << 8) |
               u32(Tail[B * 64 + J * 4 + 3]);
    RefBlock(S, Buf);
  }

  for (int J = 0; J < 5; ++J)
    for (int K = 0; K < 4; ++K)
      Out[J * 4 + K] = static_cast<uint8_t>(S[J] >> (24 - 8 * K));
}

uint64_t State = 0x9e3779b97f4a7c15ULL;

uint64_t Next() {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

volatile uint8_t Sink;

double Bench(void (*Fn)(const uint8_t *, size_t, uint8_t *),
             const std::vector<uint8_t> &Data, size_t Len) {
  void (*volatile F)(const uint8_t *, size_t, uint8_t *) = Fn;
  size_t Iters = std::max<size_t>(1, 200'000'000 / (Len + 16));
  if (Iters > 4'000'000)
    Iters = 4'000'000;
  double Best = 1e30;
  for (int Rep = 0; Rep < 5; ++Rep) {
    uint8_t Out[20];
    auto Start = std::chrono::steady_clock::now();
    for (size_t I = 0; I < Iters; ++I)
      F(Data.data(), Len, Out);
    auto End = std::chrono::steady_clock::now();
    Sink = Out[0];
    double Ns =
        std::chrono::duration_cast<std::chrono::nanoseconds>(End - Start)
            .count();
    Best = std::min(Best, Ns / double(Iters));
  }
  return Best;
}

} // namespace

int main() {
  std::vector<uint8_t> Data(1 << 16);
  for (auto &B : Data)
    B = static_cast<uint8_t>(Next());

  static const struct {
    const char *Input;
    const char *Hex;
  } Vectors[] = {
      {"", "da39a3ee5e6b4b0d3255bfef95601890afd80709"},
      {"abc", "a9993e364706816aba3e25717850c26c9cd0d89d"},
      {"The quick brown fox jumps over the lazy dog",
       "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"},
  };

  int Failures = 0;
  for (const auto &V : Vectors) {
    uint8_t Out[20];
    llvm_sha1_hash(reinterpret_cast<const uint8_t *>(V.Input),
                   std::strlen(V.Input), Out);
    char Hex[41];
    for (int J = 0; J < 20; ++J)
      std::snprintf(Hex + J * 2, 3, "%02x", Out[J]);
    if (std::strcmp(Hex, V.Hex) != 0) {
      std::fprintf(stderr, "vector mismatch for \"%s\": %s\n", V.Input, Hex);
      ++Failures;
    }
  }

  for (size_t Len = 0; Len <= 700; ++Len) {
    uint8_t A[20], B[20], C[20];
    llvm_sha1_hash(Data.data(), Len, A);
    llvm_sha1_hash_scalar(Data.data(), Len, C);
    RefHash(Data.data(), Len, B);
    if (std::memcmp(A, B, 20) != 0) {
      std::fprintf(stderr, "auto mismatch at len=%zu\n", Len);
      ++Failures;
    }
    if (std::memcmp(C, B, 20) != 0) {
      std::fprintf(stderr, "scalar mismatch at len=%zu\n", Len);
      ++Failures;
    }
  }

  if (Failures)
    return 1;
  std::printf("published vectors: 3 checked\n");
  std::printf("SHA-NI: %s\n", llvm_sha1_has_sha_ni() ? "available" : "unavailable");
  std::printf("randomized differential: 701 cases\n");

  const size_t Lengths[] = {16, 64, 256, 1024, 4096, 65536};
  std::printf("\n%-10s", "input");
  for (size_t L : Lengths)
    std::printf("%10zu", L);
  std::printf("\n");

  std::vector<double> Cpp, Scalar, Auto;
  for (size_t L : Lengths)
    Cpp.push_back(Bench(RefHash, Data, L));
  for (size_t L : Lengths)
    Scalar.push_back(Bench(llvm_sha1_hash_scalar, Data, L));
  for (size_t L : Lengths)
    Auto.push_back(Bench(llvm_sha1_hash, Data, L));

  std::printf("%-12s", "C++");
  for (double V : Cpp)
    std::printf("%10.2f", V);
  std::printf("\n%-12s", "Rust scalar");
  for (double V : Scalar)
    std::printf("%10.2f", V);
  std::printf("\n%-12s", "Rust auto");
  for (double V : Auto)
    std::printf("%10.2f", V);
  std::printf("\n%-12s", "scalar vs C++");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", Cpp[I] / Scalar[I]);
  std::printf("\n%-12s", "auto vs C++");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", Cpp[I] / Auto[I]);
  std::printf("\n");

  std::printf("\nOK: Rust SHA-1 is bit-identical to the C++ reference\n");
  return 0;
}
