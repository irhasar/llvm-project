//===-- differential.cpp - DJB Rust vs C++ comparison ---------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/DJB.h"

#include <chrono>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

extern "C" uint32_t llvm_djb_case_folding_hash(const uint8_t *Data, size_t Len,
                                               uint32_t H);

namespace {

uint32_t rustHash(llvm::StringRef Input, uint32_t H = 5381) {
  return llvm_djb_case_folding_hash(
      reinterpret_cast<const uint8_t *>(Input.data()), Input.size(), H);
}

uint32_t cppHash(llvm::StringRef Input, uint32_t H = 5381) {
  return llvm::caseFoldingDjbHash(Input, H);
}

bool check(llvm::StringRef Input) {
  const uint32_t Rust = rustHash(Input);
  const uint32_t Cpp = cppHash(Input);
  if (Rust == Cpp)
    return true;
  std::fprintf(stderr, "mismatch len=%zu rust=%u cpp=%u bytes:", Input.size(),
               Rust, Cpp);
  for (unsigned char C : Input.bytes())
    std::fprintf(stderr, " %02x", C);
  std::fputc('\n', stderr);
  return false;
}

std::string utf8(uint32_t C) {
  std::string Out;
  if (C < 0x80) {
    Out.push_back(static_cast<char>(C));
  } else if (C < 0x800) {
    Out.push_back(static_cast<char>(0xc0 | (C >> 6)));
    Out.push_back(static_cast<char>(0x80 | (C & 0x3f)));
  } else if (C < 0x10000) {
    Out.push_back(static_cast<char>(0xe0 | (C >> 12)));
    Out.push_back(static_cast<char>(0x80 | ((C >> 6) & 0x3f)));
    Out.push_back(static_cast<char>(0x80 | (C & 0x3f)));
  } else {
    Out.push_back(static_cast<char>(0xf0 | (C >> 18)));
    Out.push_back(static_cast<char>(0x80 | ((C >> 12) & 0x3f)));
    Out.push_back(static_cast<char>(0x80 | ((C >> 6) & 0x3f)));
    Out.push_back(static_cast<char>(0x80 | (C & 0x3f)));
  }
  return Out;
}

void benchmark() {
  std::vector<std::string> ascii;
  for (int I = 0; I < 1000; ++I)
    ascii.push_back("DW_AT_name_" + std::to_string(I) + "_abcdefghijklmnop");
  std::vector<std::string> unicode;
  for (uint32_t C = 0x00c0; C <= 0x017f; ++C)
    unicode.push_back(utf8(C));
  for (uint32_t C = 0x0391; C <= 0x03c9; ++C)
    unicode.push_back(utf8(C));

  volatile uint32_t Sink = 0;
  for (int Rep = 0; Rep < 2000; ++Rep)
    for (const auto &S : ascii)
      Sink += rustHash(S);
  auto Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : ascii)
      Sink += rustHash(S);
  auto End = std::chrono::steady_clock::now();
  double RustAscii = std::chrono::duration<double, std::nano>(End - Start).count() /
                     (20000.0 * ascii.size());

  for (int Rep = 0; Rep < 2000; ++Rep)
    for (const auto &S : ascii)
      Sink += cppHash(S);
  Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : ascii)
      Sink += cppHash(S);
  End = std::chrono::steady_clock::now();
  double CppAscii = std::chrono::duration<double, std::nano>(End - Start).count() /
                    (20000.0 * ascii.size());

  for (int Rep = 0; Rep < 2000; ++Rep)
    for (const auto &S : unicode)
      Sink += rustHash(S);
  Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : unicode)
      Sink += rustHash(S);
  End = std::chrono::steady_clock::now();
  double RustUnicode = std::chrono::duration<double, std::nano>(End - Start).count() /
                       (20000.0 * unicode.size());

  for (int Rep = 0; Rep < 2000; ++Rep)
    for (const auto &S : unicode)
      Sink += cppHash(S);
  Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : unicode)
      Sink += cppHash(S);
  End = std::chrono::steady_clock::now();
  double CppUnicode = std::chrono::duration<double, std::nano>(End - Start).count() /
                      (20000.0 * unicode.size());

  std::printf("benchmark: ascii c++ %.2f ns rust %.2f ns speedup %.2fx\n",
              CppAscii, RustAscii, CppAscii / RustAscii);
  std::printf("benchmark: unicode c++ %.2f ns rust %.2f ns speedup %.2fx\n",
              CppUnicode, RustUnicode, CppUnicode / RustUnicode);
}

} // namespace

int main() {
  std::vector<std::string> Corpus = {
      "", "a", "abc", "Hello", "hello", "ABC", "abc",
      "\xc3\x84", "\xc3\xa4", "\xce\x91", "\xce\xb1",
      "\xc4\xb0", "\xc4\xb1", "\xe2\x84\xaa", "\xc5\xbf",
      "DW_AT_name", "DW_AT_name_extra", "a\0b", "a\0B",
  };
  for (unsigned C = 0; C < 256; ++C)
    Corpus.emplace_back(1, static_cast<char>(C));

  std::size_t Checks = 0;
  for (const auto &S : Corpus) {
    if (!check(S))
      return 1;
    ++Checks;
  }

  // Every legal scalar value exercises the folding table.
  for (uint32_t C = 0; C <= 0x10ffff; ++C) {
    if (C >= 0xd800 && C <= 0xdfff)
      continue;
    if (!check(utf8(C)))
      return 1;
    ++Checks;
  }

  // Random valid UTF-8, including multi-code-point names. The C++ reference
  // writes past its one-code-point output buffer when a valid code point is
  // followed by a truncated sequence, so malformed multi-byte fuzz is not a
  // meaningful differential input; the single-byte cases above cover the
  // replacement path.
  uint64_t State = 0x9e3779b97f4a7c15ULL;
  for (unsigned I = 0; I < 200000; ++I) {
    State = State * 6364136223846793005ULL + 1442695040888963407ULL;
    unsigned Len = static_cast<unsigned>(State >> 59) & 7;
    std::string Input;
    for (unsigned J = 0; J < Len; ++J) {
      State = State * 6364136223846793005ULL + 1442695040888963407ULL;
      uint32_t C = static_cast<uint32_t>(State % 0x110000);
      if (C >= 0xd800 && C <= 0xdfff)
        C = 0xfffd;
      Input += utf8(C);
    }
    if (!check(Input))
      return 1;
    ++Checks;
  }

  std::printf("corpus: %zu inputs, 0 mismatches\n", Checks);
  benchmark();
  std::printf("OK: Rust caseFoldingDjbHash matches C++\n");
}
