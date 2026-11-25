//===-- differential.cpp - Differential test for the Rust ConvertUTF -----===//
//
// Compares the Rust conversion routines against the C++ implementation in
// llvm/lib/Support/ConvertUTF.cpp and reports throughput for both.
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/ConvertUTF.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <random>
#include <string>
#include <vector>

using namespace llvm;

extern "C" {
unsigned llvm_convertutf_utf8_to_utf16(const UTF8 **, const UTF8 *, UTF16 **,
                                       UTF16 *, unsigned);
unsigned llvm_convertutf_utf8_to_utf32(const UTF8 **, const UTF8 *, UTF32 **,
                                       UTF32 *, unsigned);
unsigned llvm_convertutf_utf8_to_utf32_partial(const UTF8 **, const UTF8 *,
                                               UTF32 **, UTF32 *, unsigned);
unsigned llvm_convertutf_utf16_to_utf8(const UTF16 **, const UTF16 *, UTF8 **,
                                       UTF8 *, unsigned);
unsigned llvm_convertutf_utf32_to_utf8(const UTF32 **, const UTF32 *, UTF8 **,
                                       UTF8 *, unsigned);
unsigned llvm_convertutf_utf16_to_utf32(const UTF16 **, const UTF16 *, UTF32 **,
                                        UTF32 *, unsigned);
unsigned llvm_convertutf_utf32_to_utf16(const UTF32 **, const UTF32 *, UTF16 **,
                                        UTF16 *, unsigned);
unsigned char llvm_convertutf_is_legal_utf8_string(const UTF8 **, const UTF8 *);
unsigned char llvm_convertutf_is_legal_utf8_sequence(const UTF8 *, const UTF8 *);
unsigned llvm_convertutf_get_utf8_sequence_size(const UTF8 *, const UTF8 *);
unsigned llvm_convertutf_find_maximal_subpart(const UTF8 *, const UTF8 *);
unsigned llvm_convertutf_get_num_bytes_for_utf8(unsigned char);
}

namespace {

[[noreturn]] void fail(const char *What, size_t Case, size_t Cap,
                       unsigned Flags) {
  std::printf("FAIL: %s case=%zu cap=%zu flags=%u\n", What, Case, Cap, Flags);
  std::exit(1);
}

template <typename T>
bool same(const std::vector<T> &A, const std::vector<T> &B, size_t N) {
  return std::memcmp(A.data(), B.data(), N * sizeof(T)) == 0;
}

// ---------------------------------------------------------------- UTF-8 data

void appendUTF8(std::vector<UTF8> &Out, UTF32 CP) {
  if (CP < 0x80) {
    Out.push_back(CP);
  } else if (CP < 0x800) {
    Out.push_back(0xC0 | (CP >> 6));
    Out.push_back(0x80 | (CP & 0x3F));
  } else if (CP < 0x10000) {
    Out.push_back(0xE0 | (CP >> 12));
    Out.push_back(0x80 | ((CP >> 6) & 0x3F));
    Out.push_back(0x80 | (CP & 0x3F));
  } else {
    Out.push_back(0xF0 | (CP >> 18));
    Out.push_back(0x80 | ((CP >> 12) & 0x3F));
    Out.push_back(0x80 | ((CP >> 6) & 0x3F));
    Out.push_back(0x80 | (CP & 0x3F));
  }
}

std::vector<UTF8> validUTF8(std::mt19937 &Rng, size_t MaxLen) {
  std::vector<UTF8> Out;
  std::uniform_int_distribution<int> Len(0, int(MaxLen));
  std::uniform_int_distribution<int> Kind(0, 3);
  size_t N = Len(Rng);
  for (size_t I = 0; I < N; ++I) {
    UTF32 CP;
    switch (Kind(Rng)) {
    case 0:
      CP = Rng() % 0x80;
      break;
    case 1:
      CP = 0x80 + Rng() % 0x780;
      break;
    case 2:
      CP = 0x800 + Rng() % 0xD000;
      break;
    default:
      CP = 0x10000 + Rng() % 0x100000;
      break;
    }
    appendUTF8(Out, CP);
  }
  return Out;
}

std::vector<UTF8> malformedUTF8(std::mt19937 &Rng, size_t MaxLen) {
  std::vector<UTF8> Out = validUTF8(Rng, MaxLen);
  static const UTF8 Bad[][4] = {
      {0x80}, {0xBF}, {0xC0, 0x80}, {0xC1, 0xBF}, {0xE0, 0x80, 0x80},
      {0xED, 0xA0, 0x80}, {0xF0, 0x80, 0x80, 0x80},
      {0xF4, 0x90, 0x80, 0x80}, {0xF5, 0x80, 0x80, 0x80},
      {0xE2, 0x82}, {0xF0, 0x9F, 0x98}, {0xFF},
  };
  static const size_t BadLen[] = {1, 1, 2, 2, 3, 3, 4, 4, 4, 2, 3, 1};
  std::uniform_int_distribution<int> Pos(0, int(Out.size()));
  std::uniform_int_distribution<int> Which(0, 11);
  int N = int(Rng() % 4);
  for (int I = 0; I < N; ++I) {
    size_t P = Pos(Rng);
    int W = Which(Rng);
    Out.insert(Out.begin() + P, Bad[W], Bad[W] + BadLen[W]);
  }
  return Out;
}

// --------------------------------------------------------------- comparison

void compareUTF8toUTF16(const std::vector<UTF8> &Src, size_t Case) {
  for (unsigned Flags = 0; Flags <= 1; ++Flags) {
    for (size_t Cap = 0; Cap <= Src.size() * 2 + 2; ++Cap) {
      std::vector<UTF16> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF8 *SA = Src.data(), *SB = Src.data();
      UTF16 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF8toUTF16(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf8_to_utf16(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf8->utf16", Case, Cap, Flags);
    }
  }
}

void compareUTF8toUTF32(const std::vector<UTF8> &Src, size_t Case) {
  for (unsigned Flags = 0; Flags <= 1; ++Flags) {
    for (size_t Cap = 0; Cap <= Src.size() + 2; ++Cap) {
      std::vector<UTF32> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF8 *SA = Src.data(), *SB = Src.data();
      UTF32 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF8toUTF32(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf8_to_utf32(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf8->utf32", Case, Cap, Flags);

      SA = Src.data();
      SB = Src.data();
      TA = A.data();
      TB = B.data();
      RA = ConvertUTF8toUTF32Partial(&SA, Src.data() + Src.size(), &TA,
                                     A.data() + Cap, (ConversionFlags)Flags);
      RB = llvm_convertutf_utf8_to_utf32_partial(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      PA = size_t(TA - A.data());
      PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf8->utf32 partial", Case, Cap, Flags);
    }
  }
}

void compareUTF16(const std::vector<UTF16> &Src, size_t Case) {
  for (unsigned Flags = 0; Flags <= 1; ++Flags) {
    for (size_t Cap = 0; Cap <= Src.size() * 3 + 2; ++Cap) {
      std::vector<UTF8> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF16 *SA = Src.data(), *SB = Src.data();
      UTF8 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF16toUTF8(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf16_to_utf8(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf16->utf8", Case, Cap, Flags);
    }
    for (size_t Cap = 0; Cap <= Src.size() + 2; ++Cap) {
      std::vector<UTF32> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF16 *SA = Src.data(), *SB = Src.data();
      UTF32 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF16toUTF32(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf16_to_utf32(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf16->utf32", Case, Cap, Flags);
    }
  }
}

void compareUTF32(const std::vector<UTF32> &Src, size_t Case) {
  for (unsigned Flags = 0; Flags <= 1; ++Flags) {
    for (size_t Cap = 0; Cap <= Src.size() * 4 + 2; ++Cap) {
      std::vector<UTF8> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF32 *SA = Src.data(), *SB = Src.data();
      UTF8 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF32toUTF8(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf32_to_utf8(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf32->utf8", Case, Cap, Flags);
    }
    for (size_t Cap = 0; Cap <= Src.size() * 2 + 2; ++Cap) {
      std::vector<UTF16> A(Cap + 8, 0xCD), B(Cap + 8, 0xCD);
      const UTF32 *SA = Src.data(), *SB = Src.data();
      UTF16 *TA = A.data(), *TB = B.data();
      ConversionResult RA = ConvertUTF32toUTF16(
          &SA, Src.data() + Src.size(), &TA, A.data() + Cap,
          (ConversionFlags)Flags);
      unsigned RB = llvm_convertutf_utf32_to_utf16(
          &SB, Src.data() + Src.size(), &TB, B.data() + Cap, Flags);
      size_t PA = size_t(TA - A.data()), PB = size_t(TB - B.data());
      if (unsigned(RA) != RB || size_t(SA - Src.data()) != size_t(SB - Src.data()) ||
          PA != PB || !same(A, B, std::min(Cap, PA)))
        fail("utf32->utf16", Case, Cap, Flags);
    }
  }
}

void compareOneSequence(const UTF8 *P, const UTF8 *E, size_t Tag) {
  unsigned A = isLegalUTF8Sequence(P, E);
  unsigned B = llvm_convertutf_is_legal_utf8_sequence(P, E);
  if (A != B)
    fail("exhaustive legal", Tag, 0, 0);
  A = getUTF8SequenceSize(P, E);
  B = llvm_convertutf_get_utf8_sequence_size(P, E);
  if (A != B)
    fail("exhaustive size", Tag, 0, 0);
  if (!A) {
    A = findMaximalSubpartOfIllFormedUTF8Sequence(P, E);
    B = llvm_convertutf_find_maximal_subpart(P, E);
    if (A != B)
      fail("exhaustive maximal", Tag, 0, 0);
  }
}

void exhaustiveHelpers() {
  UTF8 B[4];
  for (int N = 1; N <= 2; ++N) {
    for (int A = 0; A < 256; ++A) {
      for (int C = 0; C < 256; ++C) {
        if (N == 1 && C)
          continue;
        B[0] = UTF8(A);
        B[1] = UTF8(C);
        compareOneSequence(B, B + N, size_t(N << 16 | A << 8 | C));
      }
    }
  }
  for (int A = 0xE0; A <= 0xEF; ++A) {
    for (int C = 0; C < 256; ++C) {
      for (int D = 0; D < 256; ++D) {
        B[0] = UTF8(A);
        B[1] = UTF8(C);
        B[2] = UTF8(D);
        compareOneSequence(B, B + 3, size_t(0x3000000 | A << 16 | C << 8 | D));
      }
    }
  }
  for (int A = 0xF0; A <= 0xF4; ++A) {
    for (int C = 0; C < 256; ++C) {
      for (int D = 0; D < 256; D += 17) {
        for (int F = 0; F < 256; F += 17) {
          B[0] = UTF8(A);
          B[1] = UTF8(C);
          B[2] = UTF8(D);
          B[3] = UTF8(F);
          compareOneSequence(B, B + 4,
                             size_t(0x4000000 | A << 16 | C << 8 | D ^ F));
        }
      }
    }
  }
}

void compareHelpers(const std::vector<UTF8> &Src, size_t Case) {
  for (size_t I = 0; I <= Src.size(); ++I) {
    for (size_t J = I; J <= Src.size(); ++J) {
      const UTF8 *P = Src.data() + I, *E = Src.data() + J;
      if (I == J)
        continue;
      unsigned A = isLegalUTF8Sequence(P, E);
      unsigned B = llvm_convertutf_is_legal_utf8_sequence(P, E);
      if (A != B)
        fail("isLegalUTF8Sequence", Case, I * 1000 + J, 0);
      A = getUTF8SequenceSize(P, E);
      B = llvm_convertutf_get_utf8_sequence_size(P, E);
      if (A != B)
        fail("getUTF8SequenceSize", Case, I * 1000 + J, 0);
      if (!isLegalUTF8Sequence(P, E)) {
        A = findMaximalSubpartOfIllFormedUTF8Sequence(P, E);
        B = llvm_convertutf_find_maximal_subpart(P, E);
        if (A != B)
          fail("findMaximalSubpart", Case, I * 1000 + J, 0);
      }
    }
    const UTF8 *A = Src.data() + I, *B = Src.data() + I;
    unsigned RA = isLegalUTF8String(&A, Src.data() + Src.size());
    unsigned RB = llvm_convertutf_is_legal_utf8_string(&B, Src.data() + Src.size());
    if (RA != RB || (A - Src.data()) != (B - Src.data()))
      fail("isLegalUTF8String", Case, I, 0);
  }
  for (unsigned I = 0; I < 256; ++I) {
    unsigned A = getNumBytesForUTF8(UTF8(I));
    unsigned B = llvm_convertutf_get_num_bytes_for_utf8(UTF8(I));
    if (A != B)
      fail("getNumBytesForUTF8", Case, I, 0);
  }
}

// --------------------------------------------------------------- benchmarks

using Clock = std::chrono::steady_clock;

template <typename F>
double timeBest(F &&Fn, int Reps = 7) {
  double Best = 1e30;
  for (int R = 0; R < Reps; ++R) {
    auto T0 = Clock::now();
    Fn();
    auto T1 = Clock::now();
    Best = std::min(Best, std::chrono::duration<double, std::nano>(T1 - T0).count());
  }
  return Best;
}

std::vector<UTF8> asciiPattern(size_t N) { return std::vector<UTF8>(N, 'a'); }

std::vector<UTF8> mixedPattern(size_t N) {
  std::vector<UTF8> Out;
  while (Out.size() + 10 <= N) {
    appendUTF8(Out, 'a');
    appendUTF8(Out, 0xE9);
    appendUTF8(Out, 0x20AC);
    appendUTF8(Out, 0x1F600);
  }
  return Out;
}

std::vector<UTF16> asciiUTF16(size_t N) { return std::vector<UTF16>(N, 'a'); }

std::vector<UTF16> mixedUTF16(size_t N) {
  std::vector<UTF16> Out;
  while (Out.size() + 5 <= N) {
    Out.push_back('a');
    Out.push_back(0xE9);
    Out.push_back(0x20AC);
    Out.push_back(0xD83D);
    Out.push_back(0xDE00);
  }
  return Out;
}

std::vector<UTF32> asciiUTF32(size_t N) { return std::vector<UTF32>(N, 'a'); }

std::vector<UTF32> mixedUTF32(size_t N) {
  std::vector<UTF32> Out;
  while (Out.size() + 4 <= N) {
    Out.push_back('a');
    Out.push_back(0xE9);
    Out.push_back(0x20AC);
    Out.push_back(0x1F600);
  }
  return Out;
}

template <typename SrcT, typename DstT, typename CppFn, typename RustFn>
void benchOne(const char *Name, const std::vector<SrcT> &Src, size_t DstCount,
              CppFn Cpp, RustFn Rust) {
  std::vector<DstT> Dst(DstCount + 8);
  const SrcT *S = Src.data();
  const SrcT *SE = S + Src.size();
  DstT *D = Dst.data();
  DstT *DE = D + DstCount;
  volatile size_t Sink = 0;
  double C = 1e30, R = 1e30;
  for (int Rep = 0; Rep < 15; ++Rep) {
    auto T0 = Clock::now();
    {
      const SrcT *P = S;
      DstT *Q = D;
      Cpp(&P, SE, &Q, DE);
      Sink += size_t(Q - D);
    }
    auto T1 = Clock::now();
    C = std::min(C, std::chrono::duration<double, std::nano>(T1 - T0).count());

    T0 = Clock::now();
    {
      const SrcT *P = S;
      DstT *Q = D;
      Rust(&P, SE, &Q, DE);
      Sink += size_t(Q - D);
    }
    T1 = Clock::now();
    R = std::min(R, std::chrono::duration<double, std::nano>(T1 - T0).count());
  }
  std::printf("%-20s %8zu %10.1f %10.1f %7.2fx\n", Name, Src.size(), C, R,
              C / R);
}

void benchmark() {
  const size_t Sizes[] = {16, 64, 256, 1024, 4096, 65536};
  std::printf("\n%-20s %8s %10s %10s %8s\n", "case", "units", "C++ ns",
              "Rust ns", "speedup");

  for (size_t N : Sizes) {
    auto A8 = asciiPattern(N);
    auto M8 = mixedPattern(N);
    auto A16 = asciiUTF16(N);
    auto M16 = mixedUTF16(N);
    auto A32 = asciiUTF32(N);
    auto M32 = mixedUTF32(N);

    benchOne<UTF8, UTF16>("u8->u16 ascii", A8, A8.size() + 4,
      [](const UTF8 **S, const UTF8 *SE, UTF16 **T, UTF16 *TE) {
        ConvertUTF8toUTF16(S, SE, T, TE, strictConversion);
      },
      [](const UTF8 **S, const UTF8 *SE, UTF16 **T, UTF16 *TE) {
        llvm_convertutf_utf8_to_utf16(S, SE, T, TE, 0);
      });
    benchOne<UTF8, UTF16>("u8->u16 mixed", M8, M8.size() + 4,
      [](const UTF8 **S, const UTF8 *SE, UTF16 **T, UTF16 *TE) {
        ConvertUTF8toUTF16(S, SE, T, TE, strictConversion);
      },
      [](const UTF8 **S, const UTF8 *SE, UTF16 **T, UTF16 *TE) {
        llvm_convertutf_utf8_to_utf16(S, SE, T, TE, 0);
      });

    benchOne<UTF8, UTF32>("u8->u32 ascii", A8, A8.size() + 4,
      [](const UTF8 **S, const UTF8 *SE, UTF32 **T, UTF32 *TE) {
        ConvertUTF8toUTF32(S, SE, T, TE, strictConversion);
      },
      [](const UTF8 **S, const UTF8 *SE, UTF32 **T, UTF32 *TE) {
        llvm_convertutf_utf8_to_utf32(S, SE, T, TE, 0);
      });
    benchOne<UTF8, UTF32>("u8->u32 mixed", M8, M8.size() + 4,
      [](const UTF8 **S, const UTF8 *SE, UTF32 **T, UTF32 *TE) {
        ConvertUTF8toUTF32(S, SE, T, TE, strictConversion);
      },
      [](const UTF8 **S, const UTF8 *SE, UTF32 **T, UTF32 *TE) {
        llvm_convertutf_utf8_to_utf32(S, SE, T, TE, 0);
      });

    benchOne<UTF16, UTF8>("u16->u8 ascii", A16, A16.size() * 3 + 4,
      [](const UTF16 **S, const UTF16 *SE, UTF8 **T, UTF8 *TE) {
        ConvertUTF16toUTF8(S, SE, T, TE, strictConversion);
      },
      [](const UTF16 **S, const UTF16 *SE, UTF8 **T, UTF8 *TE) {
        llvm_convertutf_utf16_to_utf8(S, SE, T, TE, 0);
      });
    benchOne<UTF16, UTF8>("u16->u8 mixed", M16, M16.size() * 3 + 4,
      [](const UTF16 **S, const UTF16 *SE, UTF8 **T, UTF8 *TE) {
        ConvertUTF16toUTF8(S, SE, T, TE, strictConversion);
      },
      [](const UTF16 **S, const UTF16 *SE, UTF8 **T, UTF8 *TE) {
        llvm_convertutf_utf16_to_utf8(S, SE, T, TE, 0);
      });

    benchOne<UTF32, UTF8>("u32->u8 ascii", A32, A32.size() * 4 + 4,
      [](const UTF32 **S, const UTF32 *SE, UTF8 **T, UTF8 *TE) {
        ConvertUTF32toUTF8(S, SE, T, TE, strictConversion);
      },
      [](const UTF32 **S, const UTF32 *SE, UTF8 **T, UTF8 *TE) {
        llvm_convertutf_utf32_to_utf8(S, SE, T, TE, 0);
      });
    benchOne<UTF32, UTF8>("u32->u8 mixed", M32, M32.size() * 4 + 4,
      [](const UTF32 **S, const UTF32 *SE, UTF8 **T, UTF8 *TE) {
        ConvertUTF32toUTF8(S, SE, T, TE, strictConversion);
      },
      [](const UTF32 **S, const UTF32 *SE, UTF8 **T, UTF8 *TE) {
        llvm_convertutf_utf32_to_utf8(S, SE, T, TE, 0);
      });

    benchOne<UTF16, UTF32>("u16->u32 ascii", A16, A16.size() + 4,
      [](const UTF16 **S, const UTF16 *SE, UTF32 **T, UTF32 *TE) {
        ConvertUTF16toUTF32(S, SE, T, TE, strictConversion);
      },
      [](const UTF16 **S, const UTF16 *SE, UTF32 **T, UTF32 *TE) {
        llvm_convertutf_utf16_to_utf32(S, SE, T, TE, 0);
      });
    benchOne<UTF16, UTF32>("u16->u32 mixed", M16, M16.size() + 4,
      [](const UTF16 **S, const UTF16 *SE, UTF32 **T, UTF32 *TE) {
        ConvertUTF16toUTF32(S, SE, T, TE, strictConversion);
      },
      [](const UTF16 **S, const UTF16 *SE, UTF32 **T, UTF32 *TE) {
        llvm_convertutf_utf16_to_utf32(S, SE, T, TE, 0);
      });

    benchOne<UTF32, UTF16>("u32->u16 ascii", A32, A32.size() * 2 + 4,
      [](const UTF32 **S, const UTF32 *SE, UTF16 **T, UTF16 *TE) {
        ConvertUTF32toUTF16(S, SE, T, TE, strictConversion);
      },
      [](const UTF32 **S, const UTF32 *SE, UTF16 **T, UTF16 *TE) {
        llvm_convertutf_utf32_to_utf16(S, SE, T, TE, 0);
      });
    benchOne<UTF32, UTF16>("u32->u16 mixed", M32, M32.size() * 2 + 4,
      [](const UTF32 **S, const UTF32 *SE, UTF16 **T, UTF16 *TE) {
        ConvertUTF32toUTF16(S, SE, T, TE, strictConversion);
      },
      [](const UTF32 **S, const UTF32 *SE, UTF16 **T, UTF16 *TE) {
        llvm_convertutf_utf32_to_utf16(S, SE, T, TE, 0);
      });

    const UTF8 *SA = A8.data();
    const UTF8 *MA = M8.data();
    const UTF8 *SEA = SA + A8.size();
    const UTF8 *SEM = MA + M8.size();
    volatile size_t Sink = 0;
    double C = 1e30, R = 1e30;
    for (int Rep = 0; Rep < 15; ++Rep) {
      auto T0 = Clock::now();
      {
        const UTF8 *P = SA;
        Sink += isLegalUTF8String(&P, SEA);
      }
      auto T1 = Clock::now();
      C = std::min(C, std::chrono::duration<double, std::nano>(T1 - T0).count());
      T0 = Clock::now();
      {
        const UTF8 *P = SA;
        Sink += llvm_convertutf_is_legal_utf8_string(&P, SEA);
      }
      T1 = Clock::now();
      R = std::min(R, std::chrono::duration<double, std::nano>(T1 - T0).count());
    }
    std::printf("%-20s %8zu %10.1f %10.1f %7.2fx\n", "valid ascii", A8.size(),
                C, R, C / R);

    C = 1e30;
    R = 1e30;
    for (int Rep = 0; Rep < 15; ++Rep) {
      auto T0 = Clock::now();
      {
        const UTF8 *P = MA;
        Sink += isLegalUTF8String(&P, SEM);
      }
      auto T1 = Clock::now();
      C = std::min(C, std::chrono::duration<double, std::nano>(T1 - T0).count());
      T0 = Clock::now();
      {
        const UTF8 *P = MA;
        Sink += llvm_convertutf_is_legal_utf8_string(&P, SEM);
      }
      T1 = Clock::now();
      R = std::min(R, std::chrono::duration<double, std::nano>(T1 - T0).count());
    }
    std::printf("%-20s %8zu %10.1f %10.1f %7.2fx\n", "valid mixed", M8.size(),
                C, R, C / R);
  }
}

} // namespace

int main() {
  exhaustiveHelpers();
  std::mt19937 Rng(0xC0FFEE);
  size_t Case = 0;

  for (int I = 0; I < 300; ++I) {
    auto V = validUTF8(Rng, 120);
    compareUTF8toUTF16(V, Case);
    compareUTF8toUTF32(V, Case);
    compareHelpers(V, Case);
    ++Case;
  }
  for (int I = 0; I < 300; ++I) {
    auto V = malformedUTF8(Rng, 80);
    compareUTF8toUTF16(V, Case);
    compareUTF8toUTF32(V, Case);
    compareHelpers(V, Case);
    ++Case;
  }

  for (int I = 0; I < 200; ++I) {
    size_t N = Rng() % 100;
    std::vector<UTF16> V(N);
    for (auto &C : V) {
      unsigned K = Rng() % 8;
      if (K < 5)
        C = UTF16(Rng() % 0x80);
      else if (K < 7)
        C = UTF16(0x80 + Rng() % 0xD780);
      else
        C = UTF16(0xD800 + Rng() % 0x800);
    }
    compareUTF16(V, Case++);
  }
  for (int I = 0; I < 200; ++I) {
    size_t N = Rng() % 80;
    std::vector<UTF32> V(N);
    for (auto &C : V) {
      unsigned K = Rng() % 8;
      if (K < 5)
        C = Rng() % 0x80;
      else if (K < 7)
        C = 0x80 + Rng() % 0x10FF80;
      else
        C = 0xD800 + Rng() % 0x800;
    }
    compareUTF32(V, Case++);
  }

  std::printf("differential: %zu cases OK\n", Case);
  benchmark();
  return 0;
}
