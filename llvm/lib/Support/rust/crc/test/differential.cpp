//===-- differential.cpp - Differential test for the Rust CRC-32 ----------===//
//
// Compares the Rust implementation against two C++ references: the
// byte-at-a-time loop LLVM currently uses and a slicing-by-8 implementation.
// Also reports throughput for each so regressions show up in CI logs.
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" uint32_t llvm_crc32_update(uint32_t CRC, const uint8_t *Data,
                                      size_t Len);

namespace {

constexpr uint32_t Poly = 0xEDB88320u;

constexpr std::array<std::array<uint32_t, 256>, 8> MakeTables() {
  std::array<std::array<uint32_t, 256>, 8> T{};
  for (uint32_t I = 0; I < 256; ++I) {
    uint32_t C = I;
    for (int J = 0; J < 8; ++J)
      C = (C & 1) ? (C >> 1) ^ Poly : C >> 1;
    T[0][I] = C;
  }
  for (int K = 1; K < 8; ++K)
    for (uint32_t I = 0; I < 256; ++I)
      T[K][I] = (T[K - 1][I] >> 8) ^ T[0][T[K - 1][I] & 0xff];
  return T;
}

constexpr auto T = MakeTables();

uint32_t Load32(const uint8_t *P) {
  return uint32_t(P[0]) | (uint32_t(P[1]) << 8) | (uint32_t(P[2]) << 16) |
         (uint32_t(P[3]) << 24);
}

__attribute__((noinline)) uint32_t ReferenceByte(uint32_t CRC,
                                                 const uint8_t *Data,
                                                 size_t Len) {
  uint32_t C = ~CRC;
  for (size_t I = 0; I < Len; ++I)
    C = T[0][(C ^ Data[I]) & 0xff] ^ (C >> 8);
  return ~C;
}

__attribute__((noinline)) uint32_t ReferenceSlice8(uint32_t CRC,
                                                   const uint8_t *Data,
                                                   size_t Len) {
  uint32_t C = ~CRC;
  size_t I = 0;
  for (; I + 8 <= Len; I += 8) {
    uint32_t X = C ^ Load32(Data + I);
    uint32_t Y = Load32(Data + I + 4);
    C = T[7][X & 0xff] ^ T[6][(X >> 8) & 0xff] ^ T[5][(X >> 16) & 0xff] ^
        T[4][X >> 24] ^ T[3][Y & 0xff] ^ T[2][(Y >> 8) & 0xff] ^
        T[1][(Y >> 16) & 0xff] ^ T[0][Y >> 24];
  }
  for (; I < Len; ++I)
    C = T[0][(C ^ Data[I]) & 0xff] ^ (C >> 8);
  return ~C;
}

uint64_t State = 0x243F6A8885A308D3ULL;

uint64_t Next() {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

double Bench(uint32_t (*Fn)(uint32_t, const uint8_t *, size_t),
             const std::vector<uint8_t> &Data, size_t Len) {
  size_t Iters = std::max<size_t>(1, 200'000'000 / (Len + 16));
  if (Iters > 4'000'000)
    Iters = 4'000'000;
  double Best = 1e30;
  for (int Rep = 0; Rep < 5; ++Rep) {
    volatile uint32_t Sink;
    auto Start = std::chrono::steady_clock::now();
    uint32_t C = 0;
    for (size_t I = 0; I < Iters; ++I)
      C = Fn(C, Data.data(), Len);
    auto End = std::chrono::steady_clock::now();
    Sink = C;
    (void)Sink;
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

  if (llvm_crc32_update(0, reinterpret_cast<const uint8_t *>("123456789"),
                        9) != 0xCBF43926u) {
    std::fprintf(stderr, "check value mismatch\n");
    return 1;
  }

  int Failures = 0;
  for (size_t Len = 0; Len <= 700; ++Len) {
    uint32_t Seed = static_cast<uint32_t>(Next());
    uint32_t Expected = ReferenceByte(Seed, Data.data(), Len);
    uint32_t Got = llvm_crc32_update(Seed, Data.data(), Len);
    if (Got != Expected) {
      std::fprintf(stderr, "mismatch at len=%zu: %08x vs %08x\n", Len, Got,
                   Expected);
      ++Failures;
    }
  }

  for (size_t Split = 0; Split <= 128; ++Split) {
    uint32_t Whole = ReferenceSlice8(0, Data.data(), 128);
    uint32_t First = llvm_crc32_update(0, Data.data(), Split);
    uint32_t Second = llvm_crc32_update(First, Data.data() + Split, 128 - Split);
    if (Whole != Second) {
      std::fprintf(stderr, "incremental mismatch at split=%zu\n", Split);
      ++Failures;
    }
  }

  if (Failures)
    return 1;
  std::printf("published vectors: check value 0xCBF43926\n");
  std::printf("randomized differential: %d cases\n", 701 + 129);

  const size_t Lengths[] = {16, 64, 256, 1024, 4096, 65536};
  std::printf("\n%-10s", "input");
  for (size_t L : Lengths)
    std::printf("%10zu", L);
  std::printf("\n");

  std::printf("%-10s", "C++ byte");
  std::vector<double> ByteRow, SliceRow, RustRow;
  for (size_t L : Lengths)
    ByteRow.push_back(Bench(ReferenceByte, Data, L));
  for (size_t L : Lengths)
    SliceRow.push_back(Bench(ReferenceSlice8, Data, L));
  for (size_t L : Lengths)
    RustRow.push_back(Bench(llvm_crc32_update, Data, L));
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%10.2f", ByteRow[I]);
  std::printf("\n%-10s", "C++ slice8");
  for (double V : SliceRow)
    std::printf("%10.2f", V);
  std::printf("\n%-10s", "Rust");
  for (double V : RustRow)
    std::printf("%10.2f", V);
  std::printf("\n%-10s", "speedup/byte");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", ByteRow[I] / RustRow[I]);
  std::printf("\n%-10s", "speedup/slice8");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", SliceRow[I] / RustRow[I]);
  std::printf("\n");

  std::printf("\nOK: Rust CRC-32 is bit-identical to the C++ reference\n");
  return 0;
}
