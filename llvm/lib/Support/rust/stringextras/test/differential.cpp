//===-- differential.cpp - StringExtras Rust vs C++ ----------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringExtras.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

using namespace llvm;

namespace stringextras_test {
std::string referenceSnake(StringRef Input);
std::string referenceCamel(StringRef Input, bool CapitalizeFirst);
} // namespace stringextras_test

namespace {

bool checkSnake(StringRef Input) {
  std::string Cpp = stringextras_test::referenceSnake(Input);
  std::string Rust = convertToSnakeFromCamelCase(Input);
  if (Cpp != Rust) {
    std::fprintf(stderr, "snake mismatch for '%.*s'\n",
                 static_cast<int>(Input.size()), Input.data());
    return false;
  }
  return true;
}

bool checkCamel(StringRef Input, bool CapitalizeFirst) {
  std::string Cpp = stringextras_test::referenceCamel(Input, CapitalizeFirst);
  std::string Rust = convertToCamelFromSnakeCase(Input, CapitalizeFirst);
  if (Cpp != Rust) {
    std::fprintf(stderr, "camel mismatch for '%.*s' capitalize=%d\n",
                 static_cast<int>(Input.size()), Input.data(),
                 CapitalizeFirst ? 1 : 0);
    return false;
  }
  return true;
}

void benchmark(const std::vector<std::string> &Corpus) {
  volatile size_t Sink = 0;
  constexpr int Rounds = 64;

  auto RunSnake = [&](auto Convert) {
    auto Start = std::chrono::steady_clock::now();
    for (int R = 0; R < Rounds; ++R)
      for (const std::string &Input : Corpus)
        Sink += Convert(StringRef(Input)).size();
    auto End = std::chrono::steady_clock::now();
    return std::chrono::duration<double, std::nano>(End - Start).count();
  };
  auto RunCamel = [&](auto Convert) {
    auto Start = std::chrono::steady_clock::now();
    for (int R = 0; R < Rounds; ++R)
      for (const std::string &Input : Corpus)
        Sink += Convert(StringRef(Input), true).size();
    auto End = std::chrono::steady_clock::now();
    return std::chrono::duration<double, std::nano>(End - Start).count();
  };

  auto CppSnake = std::min(RunSnake([](StringRef S) {
                    return stringextras_test::referenceSnake(S);
                  }),
                  RunSnake([](StringRef S) {
                    return stringextras_test::referenceSnake(S);
                  }));
  auto RustSnake = std::min(RunSnake([](StringRef S) {
                     return convertToSnakeFromCamelCase(S);
                   }),
                   RunSnake([](StringRef S) {
                     return convertToSnakeFromCamelCase(S);
                   }));
  auto CppCamel = std::min(RunCamel([](StringRef S, bool C) {
                    return stringextras_test::referenceCamel(S, C);
                  }),
                  RunCamel([](StringRef S, bool C) {
                    return stringextras_test::referenceCamel(S, C);
                  }));
  auto RustCamel = std::min(RunCamel([](StringRef S, bool C) {
                     return convertToCamelFromSnakeCase(S, C);
                   }),
                   RunCamel([](StringRef S, bool C) {
                     return convertToCamelFromSnakeCase(S, C);
                   }));

  double Count = static_cast<double>(Rounds) * Corpus.size();
  CppSnake /= Count;
  RustSnake /= Count;
  CppCamel /= Count;
  RustCamel /= Count;
  std::printf("benchmark snake: c++ %.2f ns rust %.2f ns speedup %.2fx\n",
              CppSnake, RustSnake, CppSnake / RustSnake);
  std::printf("benchmark camel: c++ %.2f ns rust %.2f ns speedup %.2fx\n",
              CppCamel, RustCamel, CppCamel / RustCamel);
}

} // namespace

int main() {
  std::vector<std::string> Corpus = {"",
                                     "A",
                                     "aB",
                                     "HTTPServer",
                                     "ABCDef",
                                     "foo123Bar",
                                     "already_snake",
                                     "XMLHTTPRequest",
                                     "a__b",
                                     "a_",
                                     "_foo",
                                     "foo_bar_baz",
                                     "OPName",
                                     "ABCDefGhijKLMnop"};

  size_t Checks = 0;
  for (unsigned Byte = 0; Byte != 256; ++Byte) {
    std::string Input(1, static_cast<char>(Byte));
    if (!checkSnake(Input) || !checkCamel(Input, false) ||
        !checkCamel(Input, true))
      return 1;
    Checks += 3;
  }

  uint64_t State = 0x9e3779b97f4a7c15ULL;
  for (unsigned I = 0; I < 100000; ++I) {
    State = State * 6364136223846793005ULL + 1442695040888963407ULL;
    unsigned Len = static_cast<unsigned>(State >> 60);
    std::string Input;
    Input.reserve(Len);
    for (unsigned J = 0; J < Len; ++J) {
      State = State * 6364136223846793005ULL + 1442695040888963407ULL;
      static const char Alphabet[] =
          "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
      Input.push_back(Alphabet[(State >> 56) % (sizeof(Alphabet) - 1)]);
    }
    Corpus.push_back(std::move(Input));
  }

  for (const std::string &Input : Corpus) {
    if (!checkSnake(Input) || !checkCamel(Input, false) ||
        !checkCamel(Input, true))
      return 1;
    Checks += 3;
  }

  std::printf("corpus: %zu inputs, 0 mismatches\n", Checks);
  benchmark(Corpus);
  std::printf("OK: Rust StringExtras matches C++\n");
}
