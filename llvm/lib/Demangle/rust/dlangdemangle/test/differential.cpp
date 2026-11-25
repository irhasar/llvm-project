//===-- differential.cpp - Rust vs C++ D demangler -------------*- C++ -*-===//
//
// Compares llvm::dlangDemangle with the Rust port on generated D symbols,
// randomized inputs, and mutations, then reports throughput.
//
//===----------------------------------------------------------------------===//

#include "DLangDemangleFFI.h"
#include "llvm/Demangle/Demangle.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

namespace llvm {
char *dlangDemangleCpp(std::string_view MangledName);
}

static bool rustDemangle(std::string_view Mangled, std::string &Result) {
  char *Text = llvm::dlangDemangle(Mangled);
  if (!Text)
    return false;
  Result.assign(Text);
  std::free(Text);
  return true;
}

static bool referenceDemangle(std::string_view Mangled, std::string &Result) {
  char *Text = llvm::dlangDemangleCpp(Mangled);
  if (!Text)
    return false;
  Result.assign(Text);
  std::free(Text);
  return true;
}

static bool check(std::string_view Mangled) {
  std::string Rust;
  std::string Reference;
  bool RustOK = rustDemangle(Mangled, Rust);
  bool ReferenceOK = referenceDemangle(Mangled, Reference);

  if (RustOK == ReferenceOK && (!RustOK || Rust == Reference))
    return true;

  std::fprintf(stderr, "mismatch for %zu-byte input: %.*s\n", Mangled.size(),
               static_cast<int>(Mangled.size()), Mangled.data());
  std::fprintf(stderr, "  bytes:");
  for (unsigned char C : Mangled) std::fprintf(stderr, " %02x", C);
  std::fprintf(stderr, "\n");
  std::fprintf(stderr, "  rust ok=%d: %s\n", RustOK,
               RustOK ? Rust.c_str() : "<null>");
  std::fprintf(stderr, "  c++  ok=%d: %s\n", ReferenceOK,
               ReferenceOK ? Reference.c_str() : "<null>");
  return false;
}

static std::vector<std::string> makeCorpus() {
  std::vector<std::string> Corpus;
  const char *Names[] = {"foo", "bar", "baz", "test", "std", "core", "a",
                         "longer_name", "__init", "__vtbl", "__Class",
                         "__ModuleInfo", "__Interface"};
  const std::string_view Types = "ivabcdefghjklmnopqrstuwzN";

  for (const char *Name : Names) {
    for (char Type : Types) {
      std::string Symbol = "_D";
      Symbol += std::to_string(std::strlen(Name));
      Symbol += Name;
      Symbol += Type;
      if (Type == 'z')
        Symbol += 'i';
      if (Type == 'N')
        Symbol += 'n';
      Corpus.push_back(Symbol);
    }
    Corpus.push_back(std::string("_D") + std::to_string(std::strlen(Name)) +
                     Name + "Z");
  }

  const char *Qualified[][3] = {
      {"foo", "bar", nullptr},
      {"foo", "bar", "baz"},
      {"test", "foo", "bar"},
      {"std", "core", "a"},
      {"__init", nullptr, nullptr},
      {"__vtbl", nullptr, nullptr},
      {"__Class", nullptr, nullptr},
      {"__ModuleInfo", nullptr, nullptr},
      {"__Interface", nullptr, nullptr},
  };
  for (const auto &Q : Qualified) {
    std::string Symbol = "_D";
    for (const char *Part : Q) {
      if (!Part)
        break;
      Symbol += std::to_string(std::strlen(Part));
      Symbol += Part;
    }
    Symbol += "Z";
    Corpus.push_back(Symbol);
  }

  for (const char *Name : Names) {
    std::string Base = "_D";
    Base += std::to_string(std::strlen(Name));
    Base += Name;
    Base += "3bar";
    for (const char *Ref : {"Qe", "QeZ", "Qe3bazZ", "QeQe", "Qba", "Qe3bazQeZ"})
      Corpus.push_back(Base + Ref);
  }

  Corpus.push_back("_Dmain");
  Corpus.push_back("_D3foo6__initZ");
  Corpus.push_back("_D3foo6__vtblZ");
  Corpus.push_back("_D3foo7__ClassZ");
  Corpus.push_back("_D3foo11__InterfaceZ");
  Corpus.push_back("_D3foo12__ModuleInfoZ");
  Corpus.push_back("_D3foo3barQeZ");
  Corpus.push_back("_D3foo3barQe3bazZ");
  Corpus.push_back("_D3foo3barQeQe");
  Corpus.push_back("_D3foo3barQe3bazQeZ");
  Corpus.push_back("_D03foo3bari");
  Corpus.push_back("_D0003foo3bari");
  Corpus.push_back("_D3foo3bar6__S3fooZ");
  Corpus.push_back("_D3foo6__S1233fooZ");
  Corpus.push_back("_D3foo3bar6__S1233bazZ");
  // A back reference must stop at an embedded NUL, as the C++ string_view does.
  Corpus.emplace_back("_D3a\0bQeZ", 9);
  Corpus.push_back("_D3foo3bar6__S3bazZ");
  Corpus.push_back("_D3foo3bar6__S3foo3bari");
  return Corpus;
}

static bool checkFuzz() {
  uint32_t State = 0x12345678;
  auto Next = [&]() {
    State = State * 1664525u + 1013904223u;
    return State;
  };
  const char Alphabet[] =
      "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";

  for (unsigned I = 0; I != 100000; ++I) {
    std::string Input = "_D";
    unsigned Len = 3 + (Next() % 48);
    for (unsigned J = 0; J != Len; ++J)
      Input += Alphabet[Next() % (sizeof(Alphabet) - 1)];
    if (!check(Input))
      return false;
  }

  for (unsigned I = 0; I != 50000; ++I) {
    std::string Input = "_D3foo3bar";
    unsigned Refs = 1 + (Next() % 4);
    for (unsigned J = 0; J != Refs; ++J) {
      Input += 'Q';
      unsigned Letters = 1 + (Next() % 3);
      for (unsigned K = 0; K != Letters; ++K)
        Input += static_cast<char>('a' + (Next() % 26));
    }
    Input += static_cast<char>("ivZ"[Next() % 3]);
    if (!check(Input))
      return false;
  }

  for (unsigned I = 0; I != 50000; ++I) {
    std::string Input = "_D3foo";
    unsigned Refs = 1 + (Next() % 4);
    for (unsigned J = 0; J != Refs; ++J) {
      Input += 'Q';
      unsigned Letters = 1 + (Next() % 3);
      for (unsigned K = 0; K != Letters; ++K)
        Input += static_cast<char>('A' + (Next() % 26));
    }
    Input += static_cast<char>("iv"[Next() % 2]);
    if (!check(Input))
      return false;
  }

  std::vector<std::string> Corpus = makeCorpus();
  for (unsigned I = 0; I != 100000; ++I) {
    std::string Input = Corpus[Next() % Corpus.size()];
    unsigned Mutations = 1 + (Next() % 4);
    for (unsigned J = 0; J != Mutations; ++J) {
      switch (Next() % 3) {
      case 0:
        if (!Input.empty())
          Input[Next() % Input.size()] = Alphabet[Next() % (sizeof(Alphabet) - 1)];
        break;
      case 1:
        Input.insert(Input.begin() + (Next() % (Input.size() + 1)),
                     Alphabet[Next() % (sizeof(Alphabet) - 1)]);
        break;
      case 2:
        if (!Input.empty())
          Input.erase(Input.begin() + (Next() % Input.size()));
        break;
      }
    }
    if (!check(Input))
      return false;
  }
  return true;
}

static double measureRound(bool Rust, const std::vector<std::string> &Corpus) {
  volatile size_t Sink = 0;
  auto Start = std::chrono::steady_clock::now();
  for (unsigned Rep = 0; Rep != 200; ++Rep) {
    for (const std::string &Symbol : Corpus) {
      char *Text =
          Rust ? llvm::dlangDemangle(Symbol) : llvm::dlangDemangleCpp(Symbol);
      if (Text) {
        std::string Copy(Text);
        Sink += Copy.size();
        if (!Copy.empty())
          Sink += static_cast<unsigned char>(Copy[0]);
        std::free(Text);
      } else {
        ++Sink;
      }
    }
  }
  auto End = std::chrono::steady_clock::now();
  return std::chrono::duration<double, std::nano>(End - Start).count() /
         (200.0 * Corpus.size());
}

static double measureApiRound(bool Rust,
                              const std::vector<std::string> &Corpus) {
  volatile size_t Sink = 0;
  auto Start = std::chrono::steady_clock::now();
  for (unsigned Rep = 0; Rep != 200; ++Rep) {
    for (const std::string &Symbol : Corpus) {
      char *Text =
          Rust ? llvm::dlangDemangle(Symbol) : llvm::dlangDemangleCpp(Symbol);
      if (Text) {
        Sink += std::strlen(Text);
        std::free(Text);
      } else {
        ++Sink;
      }
    }
  }
  auto End = std::chrono::steady_clock::now();
  return std::chrono::duration<double, std::nano>(End - Start).count() /
         (200.0 * Corpus.size());
}

int main() {
  std::vector<std::string> Corpus = makeCorpus();
  unsigned Failures = 0;
  for (const std::string &Symbol : Corpus) {
    if (!check(Symbol))
      ++Failures;
  }

  if (!checkFuzz()) {
    std::fprintf(stderr, "differential fuzz failed\n");
    return 1;
  }

  double Cpp = 1e100;
  double Rust = 1e100;
  double CppApi = 1e100;
  double RustApi = 1e100;
  for (unsigned Round = 0; Round != 5; ++Round) {
    Cpp = std::min(Cpp, measureRound(false, Corpus));
    Rust = std::min(Rust, measureRound(true, Corpus));
    CppApi = std::min(CppApi, measureApiRound(false, Corpus));
    RustApi = std::min(RustApi, measureApiRound(true, Corpus));
  }
  std::printf("corpus: %zu symbols, %u mismatches\n", Corpus.size(), Failures);
  std::printf("fuzz: 300000 inputs, 0 mismatches\n");
  std::printf("benchmark: c++ %.1f ns/symbol, rust %.1f ns/symbol, "
              "speedup %.2fx\n",
              Cpp, Rust, Cpp / Rust);
  std::printf("api: c++ %.1f ns/symbol, rust %.1f ns/symbol, speedup %.2fx\n",
              CppApi, RustApi, CppApi / RustApi);

  if (Failures != 0) {
    std::fprintf(stderr, "%u mismatches\n", Failures);
    return 1;
  }
  std::printf("OK: Rust dlangDemangle matches the C++ reference\n");
  return 0;
}
