//===-- differential.cpp - OptionStrCmp Rust vs C++ ------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/ArrayRef.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/OptionStrCmp.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

using namespace llvm;

namespace {

extern "C" int llvm_optionstrcmp_name(const uint8_t *A, size_t ALen,
                                      const uint8_t *B, size_t BLen,
                                      uint8_t FallbackCaseSensitive);

int rustName(StringRef A, StringRef B, bool Fallback) {
  return llvm_optionstrcmp_name(
      reinterpret_cast<const uint8_t *>(A.data()), A.size(),
      reinterpret_cast<const uint8_t *>(B.data()), B.size(), Fallback ? 1 : 0);
}

bool check(StringRef A, StringRef B, bool Fallback) {
  int Cpp = StrCmpOptionName(A, B, Fallback);
  int Rust = rustName(A, B, Fallback);
  if (Cpp != Rust) {
    std::fprintf(stderr, "mismatch fallback=%d cpp=%d rust=%d\n", Fallback, Cpp,
                 Rust);
    return false;
  }
  return true;
}

bool checkPrefixes(ArrayRef<StringRef> A, ArrayRef<StringRef> B) {
  int Cpp = StrCmpOptionPrefixes(A, B);
  int Rust = 0;
  for (size_t I = 0, E = std::min(A.size(), B.size()); I != E; ++I)
    if (int Cmp = rustName(A[I], B[I], true)) {
      Rust = Cmp;
      break;
    }
  if (Cpp != Rust) {
    std::fprintf(stderr, "prefix mismatch cpp=%d rust=%d\n", Cpp, Rust);
    return false;
  }
  return true;
}

void benchmark(const std::vector<std::string> &Names) {
  volatile long Sink = 0;
  auto Run = [&](auto Compare) {
    auto Start = std::chrono::steady_clock::now();
    for (int R = 0; R < 2000; ++R)
      for (const auto &A : Names)
        for (const auto &B : Names)
          Sink += Compare(StringRef(A), StringRef(B), true);
    auto End = std::chrono::steady_clock::now();
    return std::chrono::duration<double, std::nano>(End - Start).count();
  };

  auto Cpp1 = Run([](StringRef A, StringRef B, bool F) {
    return StrCmpOptionName(A, B, F);
  });
  auto Rust1 = Run([](StringRef A, StringRef B, bool F) {
    return rustName(A, B, F);
  });
  auto Cpp2 = Run([](StringRef A, StringRef B, bool F) {
    return StrCmpOptionName(A, B, F);
  });
  auto Rust2 = Run([](StringRef A, StringRef B, bool F) {
    return rustName(A, B, F);
  });
  double Count = 2000.0 * Names.size() * Names.size();
  double Cpp = std::min(Cpp1, Cpp2) / Count;
  double Rust = std::min(Rust1, Rust2) / Count;
  std::printf("benchmark: c++ %.2f ns rust %.2f ns speedup %.2fx\n", Cpp, Rust,
              Cpp / Rust);
}

} // namespace

int main() {
  std::vector<std::string> Names;
  for (int I = 0; I < 200; ++I)
    Names.push_back("--" + std::string(1, static_cast<char>('a' + I % 26)) +
                    "-option-" + std::to_string(I));
  Names.push_back("--help");
  Names.push_back("--HELP");
  Names.push_back("--version");
  Names.push_back("--target=x86_64");
  Names.push_back("");

  std::size_t Checks = 0;
  for (const auto &A : Names)
    for (const auto &B : Names)
      for (bool Fallback : {false, true}) {
        if (!check(A, B, Fallback))
          return 1;
        ++Checks;
      }

  // Every pair of single bytes, including high bytes.
  for (unsigned A = 0; A != 256; ++A)
    for (unsigned B = 0; B != 256; ++B) {
      char ABuf = static_cast<char>(A);
      char BBuf = static_cast<char>(B);
      if (!check(StringRef(&ABuf, 1), StringRef(&BBuf, 1), true))
        return 1;
      ++Checks;
    }

  std::vector<StringRef> PrefixA = {StringRef("--"), StringRef("debug"),
                                    StringRef("")};
  std::vector<StringRef> PrefixB = {StringRef("--"), StringRef("DEBUG"),
                                    StringRef("")};
  if (!checkPrefixes(PrefixA, PrefixB) ||
      !checkPrefixes(ArrayRef<StringRef>(PrefixA).take_front(1),
                     ArrayRef<StringRef>(PrefixB).take_front(2)))
    return 1;
  Checks += 2;

  uint64_t State = 0x9e3779b97f4a7c15ULL;
  for (unsigned I = 0; I < 300000; ++I) {
    State = State * 6364136223846793005ULL + 1442695040888963407ULL;
    unsigned ALen = static_cast<unsigned>(State >> 60);
    std::string A;
    for (unsigned J = 0; J < ALen; ++J) {
      State = State * 6364136223846793005ULL + 1442695040888963407ULL;
      static const char Alphabet[] = "abcdefghijklmnopqrstuvwxyz"
                                     "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_=.";
      A.push_back(Alphabet[(State >> 56) % (sizeof(Alphabet) - 1)]);
    }
    State = State * 6364136223846793005ULL + 1442695040888963407ULL;
    unsigned BLen = static_cast<unsigned>(State >> 60);
    std::string B;
    for (unsigned J = 0; J < BLen; ++J) {
      State = State * 6364136223846793005ULL + 1442695040888963407ULL;
      static const char Alphabet[] = "abcdefghijklmnopqrstuvwxyz"
                                     "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_=.";
      B.push_back(Alphabet[(State >> 56) % (sizeof(Alphabet) - 1)]);
    }
    if (!check(A, B, (I & 1) != 0))
      return 1;
    ++Checks;
  }

  std::printf("corpus: %zu inputs, 0 mismatches\n", Checks);
  benchmark(Names);
  std::printf("OK: Rust StrCmpOptionName matches C++\n");
}
