//===-- stable.cpp - Golden tests for LLVM's Base64 API -------------------===//
//
// Exercises llvm::encodeBase64 and llvm::decodeBase64 through the public
// headers. Built against LLVMSupport so it covers the Rust crate and the C++
// fallback with the same binary layout.
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Base64.h"
#include "llvm/Support/Error.h"
#include <cstdint>
#include <cstdio>
#include <string>
#include <vector>

namespace {

struct Vector {
  const char *Plain;
  const char *Encoded;
};

const Vector Vectors[] = {{"", ""},         {"f", "Zg=="},
                          {"fo", "Zm8="},   {"foo", "Zm9v"},
                          {"foob", "Zm9vYg=="},
                          {"fooba", "Zm9vYmE="},
                          {"foobar", "Zm9vYmFy"}};

int Failures = 0;

void Expect(bool Cond, const char *What) {
  if (!Cond) {
    std::fprintf(stderr, "FAIL: %s\n", What);
    ++Failures;
  }
}

} // namespace

int main() {
  for (const Vector &V : Vectors) {
    std::string Enc = llvm::encodeBase64(std::string(V.Plain));
    Expect(Enc == V.Encoded, V.Encoded);

    std::vector<char> Dec;
    if (llvm::Error E = llvm::decodeBase64(V.Encoded, Dec)) {
      llvm::consumeError(std::move(E));
      Expect(false, V.Encoded);
      continue;
    }
    Expect(std::string(Dec.begin(), Dec.end()) == V.Plain, V.Encoded);
  }

  std::vector<uint16_t> Wide = {'a', 'b', 'c'};
  Expect(llvm::encodeBase64(Wide) == "YWJj", "wide elements narrow");

  std::vector<char> Out;
  Expect(!llvm::decodeBase64("AA==", Out) && Out.size() == 1, "AA==");
  Expect(!llvm::decodeBase64("AAA=", Out) && Out.size() == 2, "AAA=");

  for (const char *Bad : {"A===", "=AAA", "AA=A", "====", "A"}) {
    std::vector<char> Dec;
    llvm::Error E = llvm::decodeBase64(Bad, Dec);
    Expect(static_cast<bool>(E), Bad);
    llvm::consumeError(std::move(E));
  }

  {
    // Invalid input must be rejected before the output is sized for the
    // maximum decoded length. An invalid first character leaves the vector
    // empty in both backends.
    std::string Bad(1 << 20, 'A');
    Bad[0] = '!';
    std::vector<char> Dec(8, 'x');
    llvm::Error E = llvm::decodeBase64(Bad, Dec);
    Expect(static_cast<bool>(E), "large invalid input");
    llvm::consumeError(std::move(E));
    Expect(Dec.empty(), "invalid input does not resize output");
  }

  if (Failures)
    return 1;
  std::printf("golden vectors: %zu RFC 4648 cases\n",
              sizeof(Vectors) / sizeof(Vectors[0]));
  std::printf("error cases: 6\n");
  std::printf("OK: Base64 encode/decode through the public API\n");
  return 0;
}
