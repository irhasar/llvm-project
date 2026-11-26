//===-- differential.cpp - VersionTuple Rust vs C++ -----------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/VersionTuple.h"

#include <chrono>
#include <cstdio>
#include <string>
#include <vector>

using namespace llvm;

bool referenceTryParse(llvm::StringRef Input, VersionTuple &V);

namespace {

struct RustFields {
  uint32_t Major;
  uint32_t Minor;
  uint32_t Subminor;
  uint32_t Build;
  uint32_t Subbuild;
  uint32_t Flags;
};

extern "C" uint8_t llvm_versiontuple_try_parse(const uint8_t *Data, size_t Len,
                                                RustFields *Out);

VersionTuple fromRust(const RustFields &F) {
  if (F.Flags & 8)
    return VersionTuple(F.Major, F.Minor, F.Subminor, F.Build, F.Subbuild);
  if (F.Flags & 4)
    return VersionTuple(F.Major, F.Minor, F.Subminor, F.Build);
  if (F.Flags & 2)
    return VersionTuple(F.Major, F.Minor, F.Subminor);
  if (F.Flags & 1)
    return VersionTuple(F.Major, F.Minor);
  return VersionTuple(F.Major);
}

bool check(llvm::StringRef Input) {
  VersionTuple Cpp(9, 9, 9);
  bool CppError = referenceTryParse(Input, Cpp);

  VersionTuple Public(9, 9, 9);
  bool PublicError = Public.tryParse(Input);
  if (PublicError != CppError || Public.asTuple() != Cpp.asTuple()) {
    std::fprintf(stderr, "public API mismatch len=%zu reference=%d public=%d\n",
                 Input.size(), CppError, PublicError);
    return false;
  }

  RustFields F{};
  bool RustError =
      llvm_versiontuple_try_parse(reinterpret_cast<const uint8_t *>(Input.data()),
                                  Input.size(), &F) != 0;

  if (CppError != RustError) {
    std::fprintf(stderr, "status mismatch len=%zu cpp=%d rust=%d bytes:",
                 Input.size(), CppError, RustError);
    for (unsigned char C : Input.bytes())
      std::fprintf(stderr, " %02x", C);
    std::fputc('\n', stderr);
    return false;
  }

  if (CppError)
    return true;

  VersionTuple Rust = fromRust(F);
  if (Cpp.asTuple() != Rust.asTuple()) {
    auto [CMaj, CMin, CSub, CBuild, CSubbuild] = Cpp.asTuple();
    auto [RMaj, RMin, RSub, RBuild, RSubbuild] = Rust.asTuple();
    std::fprintf(stderr,
                 "value mismatch len=%zu cpp=(%u,%u,%u,%u,%u) "
                 "rust=(%u,%u,%u,%u,%u) bytes:",
                 Input.size(), CMaj, CMin, CSub, CBuild, CSubbuild, RMaj, RMin,
                 RSub, RBuild, RSubbuild);
    for (unsigned char C : Input.bytes())
      std::fprintf(stderr, " %02x", C);
    std::fputc('\n', stderr);
    return false;
  }
  return true;
}

unsigned observeTuple(const VersionTuple &V) {
  auto [Major, Minor, Subminor, Build, Subbuild] = V.asTuple();
  return Major + Minor + Subminor + Build + Subbuild;
}

void benchmark() {
  std::vector<std::string> Inputs;
  for (unsigned I = 0; I < 1000; ++I)
    Inputs.push_back(std::to_string(I) + "." + std::to_string(I + 1) + "." +
                     std::to_string(I + 2) + "." + std::to_string(I + 3) + "." +
                     std::to_string(I % 1024));

  volatile unsigned Sink = 0;
  for (int Rep = 0; Rep < 200; ++Rep)
    for (const auto &S : Inputs) {
      VersionTuple V;
      bool Parsed = referenceTryParse(S, V);
      Sink += Parsed ? 1 : observeTuple(V);
    }
  auto Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : Inputs) {
      VersionTuple V;
      bool Parsed = referenceTryParse(S, V);
      Sink += Parsed ? 1 : observeTuple(V);
    }
  auto End = std::chrono::steady_clock::now();
  double CppNs =
      std::chrono::duration<double, std::nano>(End - Start).count() /
      (20000.0 * Inputs.size());

  for (int Rep = 0; Rep < 200; ++Rep)
    for (const auto &S : Inputs) {
      VersionTuple V;
      bool Parsed = V.tryParse(S);
      Sink += Parsed ? 1 : observeTuple(V);
    }
  Start = std::chrono::steady_clock::now();
  for (int Rep = 0; Rep < 20000; ++Rep)
    for (const auto &S : Inputs) {
      VersionTuple V;
      bool Parsed = V.tryParse(S);
      Sink += Parsed ? 1 : observeTuple(V);
    }
  End = std::chrono::steady_clock::now();
  double RustNs =
      std::chrono::duration<double, std::nano>(End - Start).count() /
      (20000.0 * Inputs.size());

  std::printf("benchmark: c++ %.2f ns rust-public %.2f ns speedup %.2fx\n",
              CppNs, RustNs, CppNs / RustNs);
}

} // namespace

int main() {
  std::vector<std::string> Corpus = {
      "", "0", "1", "17", "1.2", "1.2.3", "1.2.3.4", "1.2.3.4.5",
      "0.0.0.0.0", "1.", ".1", "1..2", "1.2.", "1.2.3.", "1.2.3.4.",
      "1.2.3.4.5.6", "1.2.3.4x", "a", "-1", "1.-2", "1.2.3.1048576",
      "1.2.3.1048575", "1.2.3.4.1024", "1.2.3.4.1023",
      "4294967295.4294967295.4294967295.1048575.1023",
      "4294967296.2147483648.2147483649.1048576.1024",
  };

  std::size_t Checks = 0;
  for (const auto &S : Corpus) {
    if (!check(S))
      return 1;
    ++Checks;
  }

  // Structured values cover each component boundary and bitfield truncation.
  for (unsigned Major : {0u, 1u, 17u, 4294967295u}) {
    for (unsigned Minor : {0u, 1u, 2147483647u, 2147483648u, 4294967295u}) {
      for (unsigned Build : {0u, 1u, 1048575u}) {
        std::string S = std::to_string(Major) + "." + std::to_string(Minor) +
                        ".3." + std::to_string(Build);
        if (!check(S))
          return 1;
        ++Checks;
      }
    }
  }

  uint64_t State = 0x9e3779b97f4a7c15ULL;
  for (unsigned I = 0; I < 300000; ++I) {
    State = State * 6364136223846793005ULL + 1442695040888963407ULL;
    unsigned Len = static_cast<unsigned>(State >> 58);
    std::string Input;
    for (unsigned J = 0; J < Len; ++J) {
      State = State * 6364136223846793005ULL + 1442695040888963407ULL;
      static const char Alphabet[] = "0123456789....abcXYZ-_/";
      Input.push_back(Alphabet[(State >> 56) % (sizeof(Alphabet) - 1)]);
    }
    if (!check(Input))
      return 1;
    ++Checks;
  }

  std::printf("corpus: %zu inputs, 0 mismatches\n", Checks);
  benchmark();
  std::printf("OK: Rust VersionTuple::tryParse matches C++\n");
}
