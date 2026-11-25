//===-- differential.cpp - Rust vs C++ Rust v0 demangler --------*- C++ -*-===//
//
// Compares llvm::rustDemangle with the Rust port on real symbols, randomized
// inputs, and mutations of valid symbols, then reports throughput.
//
//===----------------------------------------------------------------------===//

#include "RustDemangleFFI.h"
#include "llvm/Demangle/Demangle.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <string>
#include <string_view>
#include <vector>

static bool rustDemangle(std::string_view Mangled, std::string &Result) {
  char *Text = llvm::detail::rustDemangleViaFFI(Mangled);
  if (!Text)
    return false;
  Result.assign(Text);
  std::free(Text);
  return true;
}

static bool referenceDemangle(std::string_view Mangled, std::string &Result) {
  char *Demangled = llvm::rustDemangle(Mangled);
  if (!Demangled)
    return false;
  Result = Demangled;
  std::free(Demangled);
  return true;
}

static bool check(std::string_view Mangled) {
  std::string Rust;
  std::string Reference;
  bool RustOK = rustDemangle(Mangled, Rust);
  bool ReferenceOK = referenceDemangle(Mangled, Reference);

  if (RustOK == ReferenceOK && (!RustOK || Rust == Reference))
    return true;

  std::fprintf(stderr, "mismatch for %zu-byte input\n", Mangled.size());
  std::fprintf(stderr, "  rust ok=%d: ", RustOK);
  if (RustOK)
    std::fprintf(stderr, "%s\n", Rust.c_str());
  else
    std::fprintf(stderr, "<null>\n");
  std::fprintf(stderr, "  c++  ok=%d: ", ReferenceOK);
  if (ReferenceOK)
    std::fprintf(stderr, "%s\n", Reference.c_str());
  else
    std::fprintf(stderr, "<null>\n");
  std::fprintf(stderr, "  input: %.*s\n", static_cast<int>(Mangled.size()),
               Mangled.data());
  return false;
}

static uint64_t nextRandom(uint64_t &State) {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

static std::string mutate(const std::string &Input, uint64_t &State) {
  std::string Result = Input;
  unsigned Op = static_cast<unsigned>(nextRandom(State) % 4);
  if (Result.empty())
    Op = 1;

  switch (Op) {
  case 0: {
    size_t Index = static_cast<size_t>(nextRandom(State) % Result.size());
    Result[Index] = static_cast<char>(nextRandom(State) & 0xff);
    break;
  }
  case 1: {
    size_t Index = static_cast<size_t>(nextRandom(State) % (Result.size() + 1));
    Result.insert(Result.begin() + Index,
                  static_cast<char>(nextRandom(State) & 0xff));
    break;
  }
  case 2: {
    size_t Index = static_cast<size_t>(nextRandom(State) % Result.size());
    Result.erase(Result.begin() + Index);
    break;
  }
  default: {
    size_t Length = static_cast<size_t>(nextRandom(State) % (Result.size() + 1));
    Result.resize(Length);
    break;
  }
  }

  if (Result.size() > 256)
    Result.resize(256);
  return Result;
}

static bool runFuzz(const std::vector<std::string> &Corpus) {
  static constexpr char Alphabet[] =
      "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_.";
  uint64_t State = 0x9e3779b97f4a7c15ULL;
  size_t Failures = 0;

  for (size_t I = 0; I != 50000; ++I) {
    size_t Length = static_cast<size_t>(nextRandom(State) % 80);
    std::string Input;
    if ((nextRandom(State) & 1) != 0)
      Input = "_R";
    for (size_t J = 0; J != Length; ++J)
      Input.push_back(Alphabet[nextRandom(State) % (sizeof(Alphabet) - 1)]);
    if (!check(Input))
      ++Failures;
  }

  for (size_t I = 0; I != 50000 && !Corpus.empty(); ++I) {
    const std::string &Base = Corpus[nextRandom(State) % Corpus.size()];
    std::string Input = Base;
    unsigned Mutations = 1 + static_cast<unsigned>(nextRandom(State) % 4);
    for (unsigned M = 0; M != Mutations; ++M)
      Input = mutate(Input, State);
    if (!check(Input))
      ++Failures;
  }

  std::printf("fuzz: 100000 inputs, %zu mismatches\n", Failures);
  return Failures == 0;
}

static double measure(bool Rust, const std::vector<std::string> &Corpus,
                      unsigned Iterations) {
  volatile size_t Sink = 0;
  std::string Result;
  auto Start = std::chrono::steady_clock::now();
  for (unsigned I = 0; I != Iterations; ++I) {
    for (const std::string &Symbol : Corpus) {
      Result.clear();
      bool OK = Rust ? rustDemangle(Symbol, Result)
                     : referenceDemangle(Symbol, Result);
      Sink += OK ? Result.size() : 1;
    }
  }
  auto End = std::chrono::steady_clock::now();
  (void)Sink;
  double Seconds = std::chrono::duration<double>(End - Start).count();
  return Seconds * 1e9 / (static_cast<double>(Iterations) * Corpus.size());
}

static double measureApi(bool Rust, const std::vector<std::string> &Corpus,
                        unsigned Iterations) {
  volatile size_t Sink = 0;
  auto Start = std::chrono::steady_clock::now();
  for (unsigned I = 0; I != Iterations; ++I) {
    for (const std::string &Symbol : Corpus) {
      if (Rust) {
        char *Demangled = llvm::detail::rustDemangleViaFFI(Symbol);
        if (Demangled) {
          // Match the C++ branch: both measure the public pointer API,
          // including the length scan a C string caller needs.
          Sink += std::strlen(Demangled);
          std::free(Demangled);
        }
      } else {
        char *Demangled = llvm::rustDemangle(Symbol);
        if (Demangled) {
          Sink += std::strlen(Demangled);
          std::free(Demangled);
        }
      }
    }
  }
  auto End = std::chrono::steady_clock::now();
  (void)Sink;
  double Seconds = std::chrono::duration<double>(End - Start).count();
  return Seconds * 1e9 / (static_cast<double>(Iterations) * Corpus.size());
}

static void benchmarkPair(bool Api, const std::vector<std::string> &Corpus,
                          unsigned Iterations, unsigned Rounds,
                          const char *Label) {
  double BestCpp = 1e300;
  double BestRust = 1e300;
  for (unsigned I = 0; I != Rounds; ++I) {
    // Alternate the backends so CPU frequency drift cannot favour either one.
    double Cpp = Api ? measureApi(false, Corpus, Iterations)
                     : measure(false, Corpus, Iterations);
    double Rust = Api ? measureApi(true, Corpus, Iterations)
                      : measure(true, Corpus, Iterations);
    if (Cpp < BestCpp)
      BestCpp = Cpp;
    if (Rust < BestRust)
      BestRust = Rust;
  }
  std::printf("%s: c++ %.1f ns/symbol, rust %.1f ns/symbol, speedup %.2fx\n",
              Label, BestCpp, BestRust, BestCpp / BestRust);
}

static bool loadCorpus(const char *Path, std::vector<std::string> &Corpus) {
  std::ifstream Input(Path);
  if (!Input) {
    std::fprintf(stderr, "cannot open corpus %s\n", Path);
    return false;
  }

  std::string Line;
  while (std::getline(Input, Line)) {
    if (!Line.empty() && Line.back() == '\r')
      Line.pop_back();
    if (!Line.empty())
      Corpus.push_back(Line);
  }
  return !Corpus.empty();
}

int main(int argc, char **argv) {
  const char *CorpusPath = nullptr;
#ifdef RUSTDEMANGLE_CORPUS
  CorpusPath = RUSTDEMANGLE_CORPUS;
#endif
  bool Benchmark = true;
  for (int I = 1; I < argc; ++I) {
    if (std::strcmp(argv[I], "--no-bench") == 0)
      Benchmark = false;
    else
      CorpusPath = argv[I];
  }

  std::vector<std::string> Corpus;
  if (CorpusPath) {
    if (!loadCorpus(CorpusPath, Corpus))
      return 1;
  } else {
    Corpus = {
        "_RNvC6_123foo3bar",
        "_RNvNtNtCs8uuN1nrGffB_4core3str5count14do_count_chars",
        "_RINvMNtCs8uuN1nrGffB_4core3stre12trim_matchesNvMNtNtB5_4char7methodsc"
        "13is_whitespaceECsh0g7Hz38UNf_3std",
    };
  }

  size_t Failures = 0;
  for (const std::string &Symbol : Corpus)
    if (!check(Symbol))
      ++Failures;
  std::printf("corpus: %zu symbols, %zu mismatches\n", Corpus.size(), Failures);

  if (!runFuzz(Corpus))
    ++Failures;

  if (Benchmark) {
    unsigned Iterations = 20000;
    constexpr unsigned Rounds = 5;
    benchmarkPair(false, Corpus, Iterations, Rounds, "benchmark");
    benchmarkPair(true, Corpus, Iterations, Rounds, "api");
  }

  if (Failures != 0) {
    std::fprintf(stderr, "%zu failures\n", Failures);
    return 1;
  }
  std::printf("OK: Rust rustDemangle matches the C++ reference\n");
  return 0;
}
