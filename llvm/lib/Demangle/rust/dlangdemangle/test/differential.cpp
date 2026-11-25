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
bool dlangDemangleCppBytes(std::string_view MangledName, std::string &Result);
}

static bool rustDemangleBytes(std::string_view Mangled, std::string &Result) {
  uint8_t *Buffer = nullptr;
  intptr_t Len = llvm_dlangdemangle_demangle(
      reinterpret_cast<const uint8_t *>(Mangled.data()), Mangled.size(),
      &Buffer);
  if (Len <= 0) {
    std::free(Buffer);
    return false;
  }
  Result.assign(reinterpret_cast<const char *>(Buffer),
                static_cast<size_t>(Len));
  std::free(Buffer);
  return true;
}

static bool referenceDemangleBytes(std::string_view Mangled,
                                   std::string &Result) {
  return llvm::dlangDemangleCppBytes(Mangled, Result);
}

static bool rustDemanglePublic(std::string_view Mangled, std::string &Result) {
  char *Text = llvm::detail::dlangDemangleViaFFI(Mangled);
  if (!Text)
    return false;
  Result.assign(Text);
  bool Terminated = Text[Result.size()] == '\0';
  std::free(Text);
  return Terminated;
}

static bool referenceDemanglePublic(std::string_view Mangled,
                                    std::string &Result) {
  char *Text = llvm::dlangDemangleCpp(Mangled);
  if (!Text)
    return false;
  Result.assign(Text);
  bool Terminated = Text[Result.size()] == '\0';
  std::free(Text);
  return Terminated;
}

static void printBytes(const char *Label, const std::string &S) {
  std::fprintf(stderr, "  %s (%zu):", Label, S.size());
  for (unsigned char C : S)
    std::fprintf(stderr, " %02x", C);
  std::fputc('\n', stderr);
}

static bool check(std::string_view Mangled) {
  std::string RustBytes;
  std::string ReferenceBytes;
  bool RustOK = rustDemangleBytes(Mangled, RustBytes);
  bool ReferenceOK = referenceDemangleBytes(Mangled, ReferenceBytes);

  std::string RustPublic;
  std::string ReferencePublic;
  bool RustPublicOK = rustDemanglePublic(Mangled, RustPublic);
  bool ReferencePublicOK = referenceDemanglePublic(Mangled, ReferencePublic);

  bool BytesMatch =
      RustOK == ReferenceOK && (!RustOK || RustBytes == ReferenceBytes);
  bool PublicMatch = RustPublicOK == ReferencePublicOK &&
                     (!RustPublicOK || RustPublic == ReferencePublic);
  if (BytesMatch && PublicMatch)
    return true;

  std::fprintf(stderr, "mismatch for %zu-byte input\n", Mangled.size());
  printBytes("input", std::string(Mangled));
  printBytes("rust raw", RustBytes);
  printBytes("c++ raw", ReferenceBytes);
  printBytes("rust public", RustPublic);
  printBytes("c++ public", ReferencePublic);
  std::fprintf(stderr, "  ok raw=%d/%d public=%d/%d\n", RustOK, ReferenceOK,
               RustPublicOK, ReferencePublicOK);
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
  // A target after the first NUL is still reachable and decodes as an identifier.
  Corpus.emplace_back("_D3a\0b3barQeZ", 13);
  // The C++ entry point accepts a parse that stops at NUL and ignores trailing
  // bytes in the explicit input length.
  Corpus.emplace_back("_D2T2a\0bQZ", 10);
  Corpus.push_back("_D3foo3bar6__S3bazZ");
  Corpus.push_back("_D3foo3bar6__S3foo3bari");
  return Corpus;
}

static bool checkNulSuffix() {
  std::string_view Input("_D3a\0b3barQeZ", 13);
  std::string Expected("a\0b.bar.bar", 11);
  std::string RustBytes;
  std::string ReferenceBytes;
  std::string RustPublic;
  std::string ReferencePublic;
  if (!rustDemangleBytes(Input, RustBytes) ||
      !referenceDemangleBytes(Input, ReferenceBytes) ||
      !rustDemanglePublic(Input, RustPublic) ||
      !referenceDemanglePublic(Input, ReferencePublic))
    return false;
  if (RustBytes != Expected || ReferenceBytes != Expected)
    return false;
  // The public C-string wrapper must stop at the embedded NUL.
  return RustPublic == "a" && ReferencePublic == "a";
}

static std::vector<std::string> makeLongCorpus() {
  // Long qualified names whose repeated back references share one target.
  std::vector<std::string> Corpus;
  for (size_t Len : {size_t{64}, size_t{256}, size_t{1024}}) {
    std::string Symbol = "_D" + std::to_string(Len) + std::string(Len, 'a') +
                         "3bar";
    for (const char *Ref : {"Qe", "Qg", "Qi", "Qk", "Qm", "Qo", "Qq", "Qs"})
      Symbol += Ref;
    Symbol += "Z";
    Corpus.push_back(Symbol);
  }
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
  std::vector<std::string> LongCorpus = makeLongCorpus();
  Corpus.insert(Corpus.end(), LongCorpus.begin(), LongCorpus.end());
  unsigned Failures = 0;
  for (const std::string &Symbol : Corpus) {
    if (!check(Symbol))
      ++Failures;
  }

  if (!checkNulSuffix()) {
    std::fprintf(stderr, "differential NUL suffix comparison failed\n");
    return 1;
  }

  if (!checkFuzz()) {
    std::fprintf(stderr, "differential fuzz failed\n");
    return 1;
  }

  double Cpp = 1e100;
  double Rust = 1e100;
  double CppApi = 1e100;
  double RustApi = 1e100;
  double CppLong = 1e100;
  double RustLong = 1e100;
  for (unsigned Round = 0; Round != 5; ++Round) {
    Cpp = std::min(Cpp, measureRound(false, Corpus));
    Rust = std::min(Rust, measureRound(true, Corpus));
    CppApi = std::min(CppApi, measureApiRound(false, Corpus));
    RustApi = std::min(RustApi, measureApiRound(true, Corpus));
    CppLong = std::min(CppLong, measureRound(false, LongCorpus));
    RustLong = std::min(RustLong, measureRound(true, LongCorpus));
  }
  std::printf("corpus: %zu symbols, %u mismatches\n", Corpus.size(), Failures);
  std::printf("fuzz: 300000 inputs, 0 mismatches\n");
  std::printf("benchmark: c++ %.1f ns/symbol, rust %.1f ns/symbol, "
              "speedup %.2fx\n",
              Cpp, Rust, Cpp / Rust);
  std::printf("api: c++ %.1f ns/symbol, rust %.1f ns/symbol, speedup %.2fx\n",
              CppApi, RustApi, CppApi / RustApi);
  std::printf("long qualified: c++ %.1f ns/symbol, rust %.1f ns/symbol, "
              "speedup %.2fx\n",
              CppLong, RustLong, CppLong / RustLong);

  if (Failures != 0) {
    std::fprintf(stderr, "%u mismatches\n", Failures);
    return 1;
  }
  std::printf("OK: Rust dlangDemangle matches the C++ reference\n");
  return 0;
}
