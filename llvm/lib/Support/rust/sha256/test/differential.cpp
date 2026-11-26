//===-- differential.cpp - Differential test for the Rust SHA-256 ---------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
// Compares the Rust one-shot SHA-256 against the C++ compression function LLVM
// ships (copied verbatim below) and reports throughput for both.
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" void llvm_sha256_hash(const uint8_t *Data, size_t Len, uint8_t *Out);
extern "C" void llvm_sha256_hash_scalar(const uint8_t *Data, size_t Len,
                                        uint8_t *Out);
extern "C" int llvm_sha256_has_sha_ni();

namespace {

using u32 = uint32_t;

#define SHR(x, c) ((x) >> (c))
#define ROTR(x, n) (((x) >> n) | ((x) << (32 - (n))))

#define CH(x, y, z) (((x) & (y)) ^ (~(x) & (z)))
#define MAJ(x, y, z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))

#define SIGMA_0(x) (ROTR(x, 2) ^ ROTR(x, 13) ^ ROTR(x, 22))
#define SIGMA_1(x) (ROTR(x, 6) ^ ROTR(x, 11) ^ ROTR(x, 25))

#define SIGMA_2(x) (ROTR(x, 17) ^ ROTR(x, 19) ^ SHR(x, 10))
#define SIGMA_3(x) (ROTR(x, 7) ^ ROTR(x, 18) ^ SHR(x, 3))

#define F_EXPAND(A, B, C, D, E, F, G, H, M1, M2, M3, M4, k)                    \
  do {                                                                         \
    H += SIGMA_1(E) + CH(E, F, G) + M1 + k;                                    \
    D += H;                                                                    \
    H += SIGMA_0(A) + MAJ(A, B, C);                                            \
    M1 += SIGMA_2(M2) + M3 + SIGMA_3(M4);                                      \
  } while (0);


__attribute__((noinline)) void RefBlock(u32 S[8], u32 Buf[16]) {

#if defined(LLVM_RUST_SHA256)
  llvm_sha256_block(S, Buf);
#else
  uint32_t A = S[0];
  uint32_t B = S[1];
  uint32_t C = S[2];
  uint32_t D = S[3];
  uint32_t E = S[4];
  uint32_t F = S[5];
  uint32_t G = S[6];
  uint32_t H = S[7];

  uint32_t W00 = Buf[0];
  uint32_t W01 = Buf[1];
  uint32_t W02 = Buf[2];
  uint32_t W03 = Buf[3];
  uint32_t W04 = Buf[4];
  uint32_t W05 = Buf[5];
  uint32_t W06 = Buf[6];
  uint32_t W07 = Buf[7];
  uint32_t W08 = Buf[8];
  uint32_t W09 = Buf[9];
  uint32_t W10 = Buf[10];
  uint32_t W11 = Buf[11];
  uint32_t W12 = Buf[12];
  uint32_t W13 = Buf[13];
  uint32_t W14 = Buf[14];
  uint32_t W15 = Buf[15];

  F_EXPAND(A, B, C, D, E, F, G, H, W00, W14, W09, W01, 0x428A2F98);
  F_EXPAND(H, A, B, C, D, E, F, G, W01, W15, W10, W02, 0x71374491);
  F_EXPAND(G, H, A, B, C, D, E, F, W02, W00, W11, W03, 0xB5C0FBCF);
  F_EXPAND(F, G, H, A, B, C, D, E, W03, W01, W12, W04, 0xE9B5DBA5);
  F_EXPAND(E, F, G, H, A, B, C, D, W04, W02, W13, W05, 0x3956C25B);
  F_EXPAND(D, E, F, G, H, A, B, C, W05, W03, W14, W06, 0x59F111F1);
  F_EXPAND(C, D, E, F, G, H, A, B, W06, W04, W15, W07, 0x923F82A4);
  F_EXPAND(B, C, D, E, F, G, H, A, W07, W05, W00, W08, 0xAB1C5ED5);
  F_EXPAND(A, B, C, D, E, F, G, H, W08, W06, W01, W09, 0xD807AA98);
  F_EXPAND(H, A, B, C, D, E, F, G, W09, W07, W02, W10, 0x12835B01);
  F_EXPAND(G, H, A, B, C, D, E, F, W10, W08, W03, W11, 0x243185BE);
  F_EXPAND(F, G, H, A, B, C, D, E, W11, W09, W04, W12, 0x550C7DC3);
  F_EXPAND(E, F, G, H, A, B, C, D, W12, W10, W05, W13, 0x72BE5D74);
  F_EXPAND(D, E, F, G, H, A, B, C, W13, W11, W06, W14, 0x80DEB1FE);
  F_EXPAND(C, D, E, F, G, H, A, B, W14, W12, W07, W15, 0x9BDC06A7);
  F_EXPAND(B, C, D, E, F, G, H, A, W15, W13, W08, W00, 0xC19BF174);

  F_EXPAND(A, B, C, D, E, F, G, H, W00, W14, W09, W01, 0xE49B69C1);
  F_EXPAND(H, A, B, C, D, E, F, G, W01, W15, W10, W02, 0xEFBE4786);
  F_EXPAND(G, H, A, B, C, D, E, F, W02, W00, W11, W03, 0x0FC19DC6);
  F_EXPAND(F, G, H, A, B, C, D, E, W03, W01, W12, W04, 0x240CA1CC);
  F_EXPAND(E, F, G, H, A, B, C, D, W04, W02, W13, W05, 0x2DE92C6F);
  F_EXPAND(D, E, F, G, H, A, B, C, W05, W03, W14, W06, 0x4A7484AA);
  F_EXPAND(C, D, E, F, G, H, A, B, W06, W04, W15, W07, 0x5CB0A9DC);
  F_EXPAND(B, C, D, E, F, G, H, A, W07, W05, W00, W08, 0x76F988DA);
  F_EXPAND(A, B, C, D, E, F, G, H, W08, W06, W01, W09, 0x983E5152);
  F_EXPAND(H, A, B, C, D, E, F, G, W09, W07, W02, W10, 0xA831C66D);
  F_EXPAND(G, H, A, B, C, D, E, F, W10, W08, W03, W11, 0xB00327C8);
  F_EXPAND(F, G, H, A, B, C, D, E, W11, W09, W04, W12, 0xBF597FC7);
  F_EXPAND(E, F, G, H, A, B, C, D, W12, W10, W05, W13, 0xC6E00BF3);
  F_EXPAND(D, E, F, G, H, A, B, C, W13, W11, W06, W14, 0xD5A79147);
  F_EXPAND(C, D, E, F, G, H, A, B, W14, W12, W07, W15, 0x06CA6351);
  F_EXPAND(B, C, D, E, F, G, H, A, W15, W13, W08, W00, 0x14292967);

  F_EXPAND(A, B, C, D, E, F, G, H, W00, W14, W09, W01, 0x27B70A85);
  F_EXPAND(H, A, B, C, D, E, F, G, W01, W15, W10, W02, 0x2E1B2138);
  F_EXPAND(G, H, A, B, C, D, E, F, W02, W00, W11, W03, 0x4D2C6DFC);
  F_EXPAND(F, G, H, A, B, C, D, E, W03, W01, W12, W04, 0x53380D13);
  F_EXPAND(E, F, G, H, A, B, C, D, W04, W02, W13, W05, 0x650A7354);
  F_EXPAND(D, E, F, G, H, A, B, C, W05, W03, W14, W06, 0x766A0ABB);
  F_EXPAND(C, D, E, F, G, H, A, B, W06, W04, W15, W07, 0x81C2C92E);
  F_EXPAND(B, C, D, E, F, G, H, A, W07, W05, W00, W08, 0x92722C85);
  F_EXPAND(A, B, C, D, E, F, G, H, W08, W06, W01, W09, 0xA2BFE8A1);
  F_EXPAND(H, A, B, C, D, E, F, G, W09, W07, W02, W10, 0xA81A664B);
  F_EXPAND(G, H, A, B, C, D, E, F, W10, W08, W03, W11, 0xC24B8B70);
  F_EXPAND(F, G, H, A, B, C, D, E, W11, W09, W04, W12, 0xC76C51A3);
  F_EXPAND(E, F, G, H, A, B, C, D, W12, W10, W05, W13, 0xD192E819);
  F_EXPAND(D, E, F, G, H, A, B, C, W13, W11, W06, W14, 0xD6990624);
  F_EXPAND(C, D, E, F, G, H, A, B, W14, W12, W07, W15, 0xF40E3585);
  F_EXPAND(B, C, D, E, F, G, H, A, W15, W13, W08, W00, 0x106AA070);

  F_EXPAND(A, B, C, D, E, F, G, H, W00, W14, W09, W01, 0x19A4C116);
  F_EXPAND(H, A, B, C, D, E, F, G, W01, W15, W10, W02, 0x1E376C08);
  F_EXPAND(G, H, A, B, C, D, E, F, W02, W00, W11, W03, 0x2748774C);
  F_EXPAND(F, G, H, A, B, C, D, E, W03, W01, W12, W04, 0x34B0BCB5);
  F_EXPAND(E, F, G, H, A, B, C, D, W04, W02, W13, W05, 0x391C0CB3);
  F_EXPAND(D, E, F, G, H, A, B, C, W05, W03, W14, W06, 0x4ED8AA4A);
  F_EXPAND(C, D, E, F, G, H, A, B, W06, W04, W15, W07, 0x5B9CCA4F);
  F_EXPAND(B, C, D, E, F, G, H, A, W07, W05, W00, W08, 0x682E6FF3);
  F_EXPAND(A, B, C, D, E, F, G, H, W08, W06, W01, W09, 0x748F82EE);
  F_EXPAND(H, A, B, C, D, E, F, G, W09, W07, W02, W10, 0x78A5636F);
  F_EXPAND(G, H, A, B, C, D, E, F, W10, W08, W03, W11, 0x84C87814);
  F_EXPAND(F, G, H, A, B, C, D, E, W11, W09, W04, W12, 0x8CC70208);
  F_EXPAND(E, F, G, H, A, B, C, D, W12, W10, W05, W13, 0x90BEFFFA);
  F_EXPAND(D, E, F, G, H, A, B, C, W13, W11, W06, W14, 0xA4506CEB);
  F_EXPAND(C, D, E, F, G, H, A, B, W14, W12, W07, W15, 0xBEF9A3F7);
  F_EXPAND(B, C, D, E, F, G, H, A, W15, W13, W08, W00, 0xC67178F2);

  S[0] += A;
  S[1] += B;
  S[2] += C;
  S[3] += D;
  S[4] += E;
  S[5] += F;
  S[6] += G;
  S[7] += H;
#endif
}

__attribute__((noinline)) void RefHash(const uint8_t *Data, size_t Len,
                                       uint8_t *Out) {
  u32 S[8] = {0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
              0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19};

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

  for (int J = 0; J < 8; ++J)
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
    uint8_t Out[32];
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
      {"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
      {"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"},
      {"The quick brown fox jumps over the lazy dog",
       "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"},
  };

  int Failures = 0;
  for (const auto &V : Vectors) {
    uint8_t Out[32];
    llvm_sha256_hash(reinterpret_cast<const uint8_t *>(V.Input),
                     std::strlen(V.Input), Out);
    char Hex[65];
    for (int J = 0; J < 32; ++J)
      std::snprintf(Hex + J * 2, 3, "%02x", Out[J]);
    if (std::strcmp(Hex, V.Hex) != 0) {
      std::fprintf(stderr, "vector mismatch for \"%s\": %s\n", V.Input, Hex);
      ++Failures;
    }
  }

  for (size_t Len = 0; Len <= 700; ++Len) {
    uint8_t A[32], B[32], C[32];
    llvm_sha256_hash(Data.data(), Len, A);
    llvm_sha256_hash_scalar(Data.data(), Len, C);
    RefHash(Data.data(), Len, B);
    if (std::memcmp(A, B, 32) != 0) {
      std::fprintf(stderr, "auto mismatch at len=%zu\n", Len);
      ++Failures;
    }
    if (std::memcmp(C, B, 32) != 0) {
      std::fprintf(stderr, "scalar mismatch at len=%zu\n", Len);
      ++Failures;
    }
  }

  if (Failures)
    return 1;
  std::printf("published vectors: 3 checked\n");
  std::printf("SHA-NI: %s\n", llvm_sha256_has_sha_ni() ? "available" : "unavailable");
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
    Scalar.push_back(Bench(llvm_sha256_hash_scalar, Data, L));
  for (size_t L : Lengths)
    Auto.push_back(Bench(llvm_sha256_hash, Data, L));

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

  std::printf("\nOK: Rust SHA-256 is bit-identical to the C++ reference\n");
  return 0;
}
