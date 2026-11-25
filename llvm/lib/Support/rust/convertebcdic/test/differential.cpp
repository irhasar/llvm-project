//===-- differential.cpp - Rust vs C++ EBCDIC conversion -----------------===//
//
// Compares the Rust-backed ConverterEBCDIC API with the original C++ code and
// reports throughput for both directions.
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/ConvertEBCDIC.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <random>
#include <string>
#include <system_error>
#include <vector>

namespace llvm {
namespace ConverterEBCDICCpp {
std::error_code convertToEBCDIC(StringRef Source, SmallVectorImpl<char> &Result);
void convertToUTF8(StringRef Source, SmallVectorImpl<char> &Result);
} // namespace ConverterEBCDICCpp
} // namespace llvm

using namespace llvm;

namespace {

struct Result {
  int Status;
  std::vector<char> Bytes;
};

int statusOf(std::error_code EC) {
  if (!EC)
    return 0;
  if (EC == std::errc::invalid_argument)
    return 2;
  return 1;
}

Result rustToEBCDIC(StringRef Source) {
  SmallVector<char, 4096> Out;
  std::error_code EC = ConverterEBCDIC::convertToEBCDIC(Source, Out);
  return {statusOf(EC), std::vector<char>(Out.begin(), Out.end())};
}

Result cppToEBCDIC(StringRef Source) {
  SmallVector<char, 4096> Out;
  std::error_code EC = ConverterEBCDICCpp::convertToEBCDIC(Source, Out);
  return {statusOf(EC), std::vector<char>(Out.begin(), Out.end())};
}

Result rustToUTF8(StringRef Source) {
  SmallVector<char, 4096> Out;
  ConverterEBCDIC::convertToUTF8(Source, Out);
  return {0, std::vector<char>(Out.begin(), Out.end())};
}

Result cppToUTF8(StringRef Source) {
  SmallVector<char, 4096> Out;
  ConverterEBCDICCpp::convertToUTF8(Source, Out);
  return {0, std::vector<char>(Out.begin(), Out.end())};
}

bool checkEBCDIC(StringRef Source) {
  Result Rust = rustToEBCDIC(Source);
  Result Cpp = cppToEBCDIC(Source);
  if (Rust.Status == Cpp.Status && Rust.Bytes == Cpp.Bytes)
    return true;
  std::fprintf(stderr, "EBCDIC mismatch for %zu bytes:", Source.size());
  for (unsigned char C : Source)
    std::fprintf(stderr, " %02x", C);
  std::fprintf(stderr, "\n  rust status=%d bytes=%zu\n", Rust.Status,
               Rust.Bytes.size());
  std::fprintf(stderr, "  cpp  status=%d bytes=%zu\n", Cpp.Status,
               Cpp.Bytes.size());
  return false;
}

bool checkUTF8(StringRef Source) {
  Result Rust = rustToUTF8(Source);
  Result Cpp = cppToUTF8(Source);
  if (Rust.Bytes == Cpp.Bytes)
    return true;
  std::fprintf(stderr, "UTF8 mismatch for %zu bytes:", Source.size());
  for (unsigned char C : Source)
    std::fprintf(stderr, " %02x", C);
  std::fprintf(stderr, "\n");
  return false;
}

bool checkFuzz() {
  std::mt19937 Rng(0x5eed1234);
  std::uniform_int_distribution<unsigned> Length(0, 64);
  std::uniform_int_distribution<unsigned> Byte(0, 255);
  for (unsigned I = 0; I != 100000; ++I) {
    std::string Input;
    unsigned N = Length(Rng);
    for (unsigned J = 0; J != N; ++J)
      Input.push_back(static_cast<char>(Byte(Rng)));
    if (!checkEBCDIC(Input) || !checkUTF8(Input))
      return false;
  }
  return true;
}

double measureEBCDIC(bool Rust, const std::vector<std::string> &Corpus) {
  auto Start = std::chrono::steady_clock::now();
  size_t Sink = 0;
  for (unsigned Round = 0; Round != 2000; ++Round)
    for (const std::string &S : Corpus) {
      Result R = Rust ? rustToEBCDIC(S) : cppToEBCDIC(S);
      Sink += R.Bytes.size();
    }
  auto End = std::chrono::steady_clock::now();
  std::chrono::duration<double, std::nano> D = End - Start;
  if (Sink == 0)
    std::fputc(0, stderr);
  return D.count() / (2000.0 * Corpus.size());
}

double measureUTF8(bool Rust, const std::vector<std::string> &Corpus) {
  auto Start = std::chrono::steady_clock::now();
  size_t Sink = 0;
  for (unsigned Round = 0; Round != 2000; ++Round)
    for (const std::string &S : Corpus) {
      Result R = Rust ? rustToUTF8(S) : cppToUTF8(S);
      Sink += R.Bytes.size();
    }
  auto End = std::chrono::steady_clock::now();
  std::chrono::duration<double, std::nano> D = End - Start;
  if (Sink == 0)
    std::fputc(0, stderr);
  return D.count() / (2000.0 * Corpus.size());
}

} // namespace

int main() {
  const std::string EBCDICCorpus[] = {
      "", "A", "ABC", "Hello, world!",
      std::string("\x51", 1), std::string("\x51\x52\x53", 3),
      std::string("\x81\x82\x83", 3),
  };
  const std::string UTF8Corpus[] = {
      "", "A", "ABC", "Hello, world!", "\xC3\xA9", "\xC2\xA0\xC3\xBF",
  };

  for (const std::string &S : EBCDICCorpus) {
    if (!checkUTF8(S))
      return 1;
  }
  for (const std::string &S : UTF8Corpus) {
    if (!checkEBCDIC(S))
      return 1;
  }
  if (!checkFuzz()) {
    std::fprintf(stderr, "fuzz failed\n");
    return 1;
  }

  std::vector<std::string> ToUTF8;
  std::vector<std::string> ToEBCDIC;
  for (unsigned I = 0; I != 256; ++I) {
    ToUTF8.emplace_back(16 + (I % 4) * 16, static_cast<char>(I));
    ToEBCDIC.emplace_back(16 + (I % 4) * 16,
                          static_cast<char>('A' + (I % 26)));
  }

  double CppUTF8 = 1e100;
  double RustUTF8 = 1e100;
  double CppEBCDIC = 1e100;
  double RustEBCDIC = 1e100;
  for (unsigned Round = 0; Round != 5; ++Round) {
    CppUTF8 = std::min(CppUTF8, measureUTF8(false, ToUTF8));
    RustUTF8 = std::min(RustUTF8, measureUTF8(true, ToUTF8));
    CppEBCDIC = std::min(CppEBCDIC, measureEBCDIC(false, ToEBCDIC));
    RustEBCDIC = std::min(RustEBCDIC, measureEBCDIC(true, ToEBCDIC));
  }

  std::printf("to utf8: c++ %.2f ns, rust %.2f ns, speedup %.2fx\n", CppUTF8,
              RustUTF8, CppUTF8 / RustUTF8);
  std::printf("to ebcdic: c++ %.2f ns, rust %.2f ns, speedup %.2fx\n",
              CppEBCDIC, RustEBCDIC, CppEBCDIC / RustEBCDIC);
  std::printf("OK: Rust EBCDIC conversion matches the C++ reference\n");
  return 0;
}
