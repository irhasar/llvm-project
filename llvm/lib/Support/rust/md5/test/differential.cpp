//===-- differential.cpp - Differential test for the Rust MD5 -------------===//
//
// Compares the Rust compression function and one-shot hash against a C++
// implementation of the same algorithm (the code LLVM used to ship), and
// reports throughput for both.
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <vector>

extern "C" void llvm_md5_hash(const uint8_t *Data, size_t Len, uint8_t *Out);

namespace {

using u32 = uint32_t;

inline u32 F(u32 x, u32 y, u32 z) { return z ^ (x & (y ^ z)); }
inline u32 G(u32 x, u32 y, u32 z) { return y ^ (z & (x ^ y)); }
inline u32 H(u32 x, u32 y, u32 z) { return x ^ y ^ z; }
inline u32 I(u32 x, u32 y, u32 z) { return y ^ (x | ~z); }

#define STEP(f, a, b, c, d, x, t, s)                                           \
  (a) += f((b), (c), (d)) + (x) + (t);                                         \
  (a) = (((a) << (s)) | ((a) >> (32 - (s))));                                  \
  (a) += (b);

u32 Load32(const uint8_t *P) {
  return u32(P[0]) | (u32(P[1]) << 8) | (u32(P[2]) << 16) |
         (u32(P[3]) << 24);
}

__attribute__((noinline)) void RefBody(u32 S[4], const uint8_t *P,
                                       size_t Blocks) {
  u32 a = S[0], b = S[1], c = S[2], d = S[3];
  while (Blocks--) {
    u32 m[16];
    for (int J = 0; J < 16; ++J)
      m[J] = Load32(P + J * 4);
    u32 sa = a, sb = b, sc = c, sd = d;

    // Round 1
    STEP(F, a, b, c, d, m[0], 0xd76aa478, 7)
    STEP(F, d, a, b, c, m[1], 0xe8c7b756, 12)
    STEP(F, c, d, a, b, m[2], 0x242070db, 17)
    STEP(F, b, c, d, a, m[3], 0xc1bdceee, 22)
    STEP(F, a, b, c, d, m[4], 0xf57c0faf, 7)
    STEP(F, d, a, b, c, m[5], 0x4787c62a, 12)
    STEP(F, c, d, a, b, m[6], 0xa8304613, 17)
    STEP(F, b, c, d, a, m[7], 0xfd469501, 22)
    STEP(F, a, b, c, d, m[8], 0x698098d8, 7)
    STEP(F, d, a, b, c, m[9], 0x8b44f7af, 12)
    STEP(F, c, d, a, b, m[10], 0xffff5bb1, 17)
    STEP(F, b, c, d, a, m[11], 0x895cd7be, 22)
    STEP(F, a, b, c, d, m[12], 0x6b901122, 7)
    STEP(F, d, a, b, c, m[13], 0xfd987193, 12)
    STEP(F, c, d, a, b, m[14], 0xa679438e, 17)
    STEP(F, b, c, d, a, m[15], 0x49b40821, 22)

    // Round 2
    STEP(G, a, b, c, d, m[1], 0xf61e2562, 5)
    STEP(G, d, a, b, c, m[6], 0xc040b340, 9)
    STEP(G, c, d, a, b, m[11], 0x265e5a51, 14)
    STEP(G, b, c, d, a, m[0], 0xe9b6c7aa, 20)
    STEP(G, a, b, c, d, m[5], 0xd62f105d, 5)
    STEP(G, d, a, b, c, m[10], 0x02441453, 9)
    STEP(G, c, d, a, b, m[15], 0xd8a1e681, 14)
    STEP(G, b, c, d, a, m[4], 0xe7d3fbc8, 20)
    STEP(G, a, b, c, d, m[9], 0x21e1cde6, 5)
    STEP(G, d, a, b, c, m[14], 0xc33707d6, 9)
    STEP(G, c, d, a, b, m[3], 0xf4d50d87, 14)
    STEP(G, b, c, d, a, m[8], 0x455a14ed, 20)
    STEP(G, a, b, c, d, m[13], 0xa9e3e905, 5)
    STEP(G, d, a, b, c, m[2], 0xfcefa3f8, 9)
    STEP(G, c, d, a, b, m[7], 0x676f02d9, 14)
    STEP(G, b, c, d, a, m[12], 0x8d2a4c8a, 20)

    // Round 3
    STEP(H, a, b, c, d, m[5], 0xfffa3942, 4)
    STEP(H, d, a, b, c, m[8], 0x8771f681, 11)
    STEP(H, c, d, a, b, m[11], 0x6d9d6122, 16)
    STEP(H, b, c, d, a, m[14], 0xfde5380c, 23)
    STEP(H, a, b, c, d, m[1], 0xa4beea44, 4)
    STEP(H, d, a, b, c, m[4], 0x4bdecfa9, 11)
    STEP(H, c, d, a, b, m[7], 0xf6bb4b60, 16)
    STEP(H, b, c, d, a, m[10], 0xbebfbc70, 23)
    STEP(H, a, b, c, d, m[13], 0x289b7ec6, 4)
    STEP(H, d, a, b, c, m[0], 0xeaa127fa, 11)
    STEP(H, c, d, a, b, m[3], 0xd4ef3085, 16)
    STEP(H, b, c, d, a, m[6], 0x04881d05, 23)
    STEP(H, a, b, c, d, m[9], 0xd9d4d039, 4)
    STEP(H, d, a, b, c, m[12], 0xe6db99e5, 11)
    STEP(H, c, d, a, b, m[15], 0x1fa27cf8, 16)
    STEP(H, b, c, d, a, m[2], 0xc4ac5665, 23)

    // Round 4
    STEP(I, a, b, c, d, m[0], 0xf4292244, 6)
    STEP(I, d, a, b, c, m[7], 0x432aff97, 10)
    STEP(I, c, d, a, b, m[14], 0xab9423a7, 15)
    STEP(I, b, c, d, a, m[5], 0xfc93a039, 21)
    STEP(I, a, b, c, d, m[12], 0x655b59c3, 6)
    STEP(I, d, a, b, c, m[3], 0x8f0ccc92, 10)
    STEP(I, c, d, a, b, m[10], 0xffeff47d, 15)
    STEP(I, b, c, d, a, m[1], 0x85845dd1, 21)
    STEP(I, a, b, c, d, m[8], 0x6fa87e4f, 6)
    STEP(I, d, a, b, c, m[15], 0xfe2ce6e0, 10)
    STEP(I, c, d, a, b, m[6], 0xa3014314, 15)
    STEP(I, b, c, d, a, m[13], 0x4e0811a1, 21)
    STEP(I, a, b, c, d, m[4], 0xf7537e82, 6)
    STEP(I, d, a, b, c, m[11], 0xbd3af235, 10)
    STEP(I, c, d, a, b, m[2], 0x2ad7d2bb, 15)
    STEP(I, b, c, d, a, m[9], 0xeb86d391, 21)

    a += sa;
    b += sb;
    c += sc;
    d += sd;
    P += 64;
  }
  S[0] = a;
  S[1] = b;
  S[2] = c;
  S[3] = d;
}

__attribute__((noinline)) void RefHash(const uint8_t *Data, size_t Len,
                                       uint8_t *Out) {
  u32 S[4] = {0x67452301u, 0xefcdab89u, 0x98badcfeu, 0x10325476u};
  RefBody(S, Data, Len / 64);

  uint8_t Tail[128] = {0};
  size_t Rem = Len % 64;
  std::memcpy(Tail, Data + Len - Rem, Rem);
  Tail[Rem] = 0x80;
  size_t Padded = Rem < 56 ? 64 : 128;
  uint64_t Bits = static_cast<uint64_t>(Len) * 8;
  for (int J = 0; J < 8; ++J)
    Tail[Padded - 8 + J] = static_cast<uint8_t>(Bits >> (8 * J));
  RefBody(S, Tail, Padded / 64);

  for (int J = 0; J < 4; ++J)
    for (int K = 0; K < 4; ++K)
      Out[J * 4 + K] = static_cast<uint8_t>(S[J] >> (8 * K));
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
    uint8_t Out[16];
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
      {"", "d41d8cd98f00b204e9800998ecf8427e"},
      {"a", "0cc175b9c0f1b6a831c399e269772661"},
      {"abc", "900150983cd24fb0d6963f7d28e17f72"},
      {"message digest", "f96b697d7cb7938d525a2f31aaf161d0"},
  };

  int Failures = 0;
  for (const auto &V : Vectors) {
    uint8_t Out[16];
    llvm_md5_hash(reinterpret_cast<const uint8_t *>(V.Input),
                  std::strlen(V.Input), Out);
    char Hex[33];
    for (int J = 0; J < 16; ++J)
      std::snprintf(Hex + J * 2, 3, "%02x", Out[J]);
    if (std::strcmp(Hex, V.Hex) != 0) {
      std::fprintf(stderr, "vector mismatch for \"%s\": %s\n", V.Input, Hex);
      ++Failures;
    }
  }

  for (size_t Len = 0; Len <= 700; ++Len) {
    uint8_t A[16], B[16];
    llvm_md5_hash(Data.data(), Len, A);
    RefHash(Data.data(), Len, B);
    if (std::memcmp(A, B, 16) != 0) {
      std::fprintf(stderr, "mismatch at len=%zu\n", Len);
      ++Failures;
    }
  }

  if (Failures)
    return 1;
  std::printf("published vectors: 4 checked\n");
  std::printf("randomized differential: 701 cases\n");

  const size_t Lengths[] = {16, 64, 256, 1024, 4096, 65536};
  std::printf("\n%-10s", "input");
  for (size_t L : Lengths)
    std::printf("%10zu", L);
  std::printf("\n");

  std::vector<double> Cpp, Rust;
  for (size_t L : Lengths)
    Cpp.push_back(Bench(RefHash, Data, L));
  for (size_t L : Lengths)
    Rust.push_back(Bench(llvm_md5_hash, Data, L));

  std::printf("%-10s", "C++");
  for (double V : Cpp)
    std::printf("%10.2f", V);
  std::printf("\n%-10s", "Rust");
  for (double V : Rust)
    std::printf("%10.2f", V);
  std::printf("\n%-10s", "speedup");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", Cpp[I] / Rust[I]);
  std::printf("\n");

  std::printf("\nOK: Rust MD5 is bit-identical to the C++ reference\n");
  return 0;
}
