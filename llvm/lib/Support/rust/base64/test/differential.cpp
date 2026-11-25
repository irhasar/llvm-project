//===-- differential.cpp - Differential test for the Rust Base64 ---------===//
//
// Compares the Rust encoder/decoder against the C++ implementation that LLVM
// ships today: the encodeBase64 template from llvm/Support/Base64.h and the
// decodeBase64 loop from llvm/lib/Support/Base64.cpp. Both references are
// copied verbatim so the test can be built with plain g++ against the crate's
// static archive, without an LLVM build.
//
// The same program prints a throughput table so a regression shows up in the
// logs next to the correctness result.
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

extern "C" size_t llvm_base64_encode_rust(const uint8_t *Src, size_t Len,
                                     char *Dst);
extern "C" int llvm_base64_decode_rust(const uint8_t *Src, size_t Len, char *Dst,
                                  size_t *OutLen, size_t *BadIndex);
extern "C" int llvm_base64_validate_rust(const uint8_t *Src, size_t Len,
                                         size_t *OutLen, size_t *BadIndex);
extern "C" int llvm_base64_decode_valid_rust(const uint8_t *Src, size_t Len,
                                             char *Dst, size_t *OutLen);

namespace {

//===----------------------------------------------------------------------===//
// C++ reference: encodeBase64 from llvm/Support/Base64.h.
//===----------------------------------------------------------------------===//

std::string ReferenceEncode(const uint8_t *Bytes, size_t Size) {
  static const char Table[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
                              "abcdefghijklmnopqrstuvwxyz"
                              "0123456789+/";
  std::string Buffer;
  Buffer.resize(((Size + 2) / 3) * 4);

  size_t i = 0, j = 0;
  for (size_t n = Size / 3 * 3; i < n; i += 3, j += 4) {
    uint32_t x = ((unsigned char)Bytes[i] << 16) |
                 ((unsigned char)Bytes[i + 1] << 8) |
                 (unsigned char)Bytes[i + 2];
    Buffer[j + 0] = Table[(x >> 18) & 63];
    Buffer[j + 1] = Table[(x >> 12) & 63];
    Buffer[j + 2] = Table[(x >> 6) & 63];
    Buffer[j + 3] = Table[x & 63];
  }
  if (i + 1 == Size) {
    uint32_t x = ((unsigned char)Bytes[i] << 16);
    Buffer[j + 0] = Table[(x >> 18) & 63];
    Buffer[j + 1] = Table[(x >> 12) & 63];
    Buffer[j + 2] = '=';
    Buffer[j + 3] = '=';
  } else if (i + 2 == Size) {
    uint32_t x =
        ((unsigned char)Bytes[i] << 16) | ((unsigned char)Bytes[i + 1] << 8);
    Buffer[j + 0] = Table[(x >> 18) & 63];
    Buffer[j + 1] = Table[(x >> 12) & 63];
    Buffer[j + 2] = Table[(x >> 6) & 63];
    Buffer[j + 3] = '=';
  }
  return Buffer;
}

//===----------------------------------------------------------------------===//
// C++ reference: decodeBase64 from llvm/lib/Support/Base64.cpp.
//===----------------------------------------------------------------------===//

#define INVALID_BASE64_BYTE 64

char decodeBase64Byte(uint8_t Ch) {
  constexpr char Inv = INVALID_BASE64_BYTE;
  static const char DecodeTable[] = {
      Inv, Inv, Inv, Inv, Inv, Inv, Inv, Inv, // ........
      Inv, Inv, Inv, Inv, Inv, Inv, Inv, Inv, // ........
      Inv, Inv, Inv, Inv, Inv, Inv, Inv, Inv, // ........
      Inv, Inv, Inv, Inv, Inv, Inv, Inv, Inv, // ........
      Inv, Inv, Inv, Inv, Inv, Inv, Inv, Inv, // ........
      Inv, Inv, Inv, 62,  Inv, Inv, Inv, 63,  // ...+.../
      52,  53,  54,  55,  56,  57,  58,  59,  // 01234567
      60,  61,  Inv, Inv, Inv, 0,   Inv, Inv, // 89...=..
      Inv, 0,   1,   2,   3,   4,   5,   6,   // .ABCDEFG
      7,   8,   9,   10,  11,  12,  13,  14,  // HIJKLMNO
      15,  16,  17,  18,  19,  20,  21,  22,  // PQRSTUVW
      23,  24,  25,  Inv, Inv, Inv, Inv, Inv, // XYZ.....
      Inv, 26,  27,  28,  29,  30,  31,  32,  // .abcdefg
      33,  34,  35,  36,  37,  38,  39,  40,  // hijklmno
      41,  42,  43,  44,  45,  46,  47,  48,  // pqrstuvw
      49,  50,  51                            // xyz.....
  };
  if (Ch >= sizeof(DecodeTable))
    return Inv;
  return DecodeTable[Ch];
}

enum Status { Ok, LengthError, BadChar };

struct DecodeResult {
  Status S = LengthError;
  size_t BadIndex = 0;
  std::vector<char> Output;
};

DecodeResult ReferenceDecode(const char *Input, size_t InputLength) {
  DecodeResult R;
  constexpr char Base64InvalidByte = INVALID_BASE64_BYTE;
  R.Output.clear();
  if (InputLength == 0) {
    R.S = Ok;
    return R;
  }
  if ((InputLength % 4) != 0) {
    R.S = LengthError;
    return R;
  }
  const size_t FirstValidEqualIdx = InputLength - 2;
  char Hex64Bytes[4];
  for (size_t Idx = 0; Idx < InputLength; Idx += 4) {
    for (size_t ByteOffset = 0; ByteOffset < 4; ++ByteOffset) {
      const size_t ByteIdx = Idx + ByteOffset;
      const char Byte = Input[ByteIdx];
      const char DecodedByte = decodeBase64Byte(Byte);
      bool Illegal = DecodedByte == Base64InvalidByte;
      if (!Illegal && Byte == '=') {
        if (ByteIdx < FirstValidEqualIdx) {
          Illegal = true;
        } else if (ByteIdx == FirstValidEqualIdx &&
                   Input[ByteIdx + 1] != '=') {
          Illegal = true;
        }
      }
      if (Illegal) {
        R.S = BadChar;
        R.BadIndex = ByteIdx;
        return R;
      }
      Hex64Bytes[ByteOffset] = DecodedByte;
    }
    R.Output.push_back((Hex64Bytes[0] << 2) +
                       ((Hex64Bytes[1] >> 4) & 0x03));
    R.Output.push_back((Hex64Bytes[1] << 4) +
                       ((Hex64Bytes[2] >> 2) & 0x0f));
    R.Output.push_back((Hex64Bytes[2] << 6) + (Hex64Bytes[3] & 0x3f));
  }
  if (Input[InputLength - 1] == '=') {
    R.Output.pop_back();
    if (Input[InputLength - 2] == '=')
      R.Output.pop_back();
  }
  R.S = Ok;
  return R;
}

//===----------------------------------------------------------------------===//
// Rust wrappers: the same shape the C++ callers will use.
//===----------------------------------------------------------------------===//

std::string RustEncode(const uint8_t *Bytes, size_t Size) {
  std::string Buffer;
  Buffer.resize(((Size + 2) / 3) * 4);
  if (!Buffer.empty())
    llvm_base64_encode_rust(Bytes, Size, Buffer.data());
  return Buffer;
}

DecodeResult RustDecode(const char *Input, size_t InputLength) {
  DecodeResult R;
  if (InputLength == 0) {
    R.S = Ok;
    return R;
  }
  if ((InputLength % 4) != 0) {
    R.S = LengthError;
    return R;
  }
  R.Output.resize(InputLength / 4 * 3);
  size_t OutLen = 0;
  size_t BadIndex = 0;
  int Rc = llvm_base64_decode_rust(reinterpret_cast<const uint8_t *>(Input),
                              InputLength, R.Output.data(), &OutLen, &BadIndex);
  R.Output.resize(OutLen);
  R.BadIndex = BadIndex;
  R.S = Rc == 0 ? Ok : (Rc == 1 ? LengthError : BadChar);
  return R;
}

// Benchmark-only path that matches decodeBase64: validate the whole input,
// then decode it. The correctness tests use RustDecode, which keeps the
// partial output the C++ loop leaves behind.
DecodeResult RustDecodePublic(const char *Input, size_t InputLength) {
  DecodeResult R;
  if (InputLength == 0) {
    R.S = Ok;
    return R;
  }
  if ((InputLength % 4) != 0) {
    R.S = LengthError;
    return R;
  }
  size_t DecodedLen = 0;
  size_t BadIndex = 0;
  int Rc = llvm_base64_validate_rust(
      reinterpret_cast<const uint8_t *>(Input), InputLength, &DecodedLen,
      &BadIndex);
  R.BadIndex = BadIndex;
  if (Rc != 0) {
    R.S = Rc == 1 ? LengthError : BadChar;
    if (Rc == 2) {
      const size_t GoodBlocks = BadIndex / 4;
      const size_t PartialLen = GoodBlocks * 3;
      if (PartialLen != 0) {
        R.Output.resize(PartialLen);
        size_t OutLen = 0;
        int DecodeRc = llvm_base64_decode_valid_rust(
            reinterpret_cast<const uint8_t *>(Input), GoodBlocks * 4,
            R.Output.data(), &OutLen);
        if (DecodeRc != 0 || OutLen != PartialLen) {
          R.Output.clear();
          R.S = BadChar;
        }
      }
    }
    return R;
  }
  R.Output.resize(DecodedLen);
  size_t OutLen = 0;
  Rc = llvm_base64_decode_valid_rust(
      reinterpret_cast<const uint8_t *>(Input), InputLength, R.Output.data(),
      &OutLen);
  if (Rc != 0 || OutLen != DecodedLen) {
    R.S = BadChar;
    return R;
  }
  R.S = Ok;
  return R;
}

//===----------------------------------------------------------------------===//
// Test harness.
//===----------------------------------------------------------------------===//

uint64_t State = 0x243F6A8885A308D3ULL;

uint64_t Next() {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

int Failures = 0;

void Fail(const char *What, size_t Len, size_t Index = 0) {
  if (Failures < 20)
    std::fprintf(stderr, "mismatch: %s len=%zu index=%zu\n", What, Len, Index);
  ++Failures;
}

bool SameResult(const DecodeResult &A, const DecodeResult &B, size_t Len) {
  if (A.S != B.S) {
    Fail("decode status", Len);
    return false;
  }
  if (A.S == BadChar && A.BadIndex != B.BadIndex) {
    Fail("decode bad index", Len, A.BadIndex);
    return false;
  }
  if (A.Output != B.Output) {
    Fail("decode output", Len);
    return false;
  }
  return true;
}

double BenchEncode(bool Rust, const std::vector<uint8_t> &Data, size_t Len,
                   size_t Iters) {
  double Best = 1e30;
  for (int Rep = 0; Rep < 5; ++Rep) {
    auto Start = std::chrono::steady_clock::now();
    size_t Sink = 0;
    for (size_t I = 0; I < Iters; ++I) {
      std::string S = Rust ? RustEncode(Data.data(), Len)
                           : ReferenceEncode(Data.data(), Len);
      Sink += S.size();
    }
    auto End = std::chrono::steady_clock::now();
    if (Sink == 0xDEADBEEF)
      std::fprintf(stderr, "unreachable\n");
    double Ns = std::chrono::duration_cast<std::chrono::nanoseconds>(End - Start)
                    .count();
    Best = std::min(Best, Ns / double(Iters));
  }
  return Best;
}

double BenchDecode(bool Rust, bool Public, const std::vector<char> &Encoded,
                   size_t Iters) {
  double Best = 1e30;
  for (int Rep = 0; Rep < 5; ++Rep) {
    auto Start = std::chrono::steady_clock::now();
    size_t Sink = 0;
    for (size_t I = 0; I < Iters; ++I) {
      DecodeResult R =
          !Rust ? ReferenceDecode(Encoded.data(), Encoded.size())
                : Public ? RustDecodePublic(Encoded.data(), Encoded.size())
                         : RustDecode(Encoded.data(), Encoded.size());
      Sink += R.Output.size();
    }
    auto End = std::chrono::steady_clock::now();
    if (Sink == 0xDEADBEEF)
      std::fprintf(stderr, "unreachable\n");
    double Ns = std::chrono::duration_cast<std::chrono::nanoseconds>(End - Start)
                    .count();
    Best = std::min(Best, Ns / double(Iters));
  }
  DecodeResult Expected = ReferenceDecode(Encoded.data(), Encoded.size());
  DecodeResult Check =
      !Rust ? Expected
            : Public ? RustDecodePublic(Encoded.data(), Encoded.size())
                     : RustDecode(Encoded.data(), Encoded.size());
  SameResult(Expected, Check, Encoded.size());
  return Best;
}

size_t IterationsFor(size_t Len, size_t Budget) {
  size_t Iters = std::max<size_t>(1, Budget / (Len + 16));
  return std::min<size_t>(Iters, 4'000'000);
}

} // namespace

int main() {
  std::vector<uint8_t> Data(1 << 20);
  for (auto &B : Data)
    B = static_cast<uint8_t>(Next());

  // Fixed vectors, including the empty input and every padding case.
  struct Vector {
    const char *Plain;
    const char *Encoded;
  };
  const Vector Vectors[] = {{"", ""},         {"f", "Zg=="},
                            {"fo", "Zm8="},   {"foo", "Zm9v"},
                            {"foob", "Zm9vYg=="},
                            {"fooba", "Zm9vYmE="},
                            {"foobar", "Zm9vYmFy"}};
  for (const Vector &V : Vectors) {
    size_t PlainLen = std::strlen(V.Plain);
    size_t EncLen = std::strlen(V.Encoded);
    std::string Ref = ReferenceEncode(
        reinterpret_cast<const uint8_t *>(V.Plain), PlainLen);
    std::string Rust = RustEncode(
        reinterpret_cast<const uint8_t *>(V.Plain), PlainLen);
    if (Ref != V.Encoded || Rust != V.Encoded) {
      Fail("rfc4648 encode", PlainLen);
      continue;
    }
    DecodeResult RefDec = ReferenceDecode(V.Encoded, EncLen);
    DecodeResult RustDec = RustDecode(V.Encoded, EncLen);
    DecodeResult RustPub = RustDecodePublic(V.Encoded, EncLen);
    if (!SameResult(RefDec, RustDec, EncLen) ||
        !SameResult(RefDec, RustPub, EncLen))
      continue;
    if (RustPub.Output.size() != PlainLen ||
        (PlainLen != 0 &&
         std::memcmp(RustPub.Output.data(), V.Plain, PlainLen) != 0))
      Fail("rfc4648 public decode", PlainLen);
  }

  // Encode: every length through 1024, plus larger powers of two.
  for (size_t Len = 0; Len <= 1024; ++Len) {
    std::string Ref = ReferenceEncode(Data.data(), Len);
    std::string Rust = RustEncode(Data.data(), Len);
    if (Ref != Rust)
      Fail("encode", Len);
  }
  for (size_t Len : {2047u, 2048u, 2049u, 4095u, 4096u, 65535u, 65536u,
                     100000u, 1048576u}) {
    if (ReferenceEncode(Data.data(), Len) != RustEncode(Data.data(), Len))
      Fail("encode", Len);
  }

  // Decode valid encodings of every length through 1024.
  for (size_t Len = 0; Len <= 1024; ++Len) {
    std::string Enc = ReferenceEncode(Data.data(), Len);
    DecodeResult Ref = ReferenceDecode(Enc.data(), Enc.size());
    DecodeResult Rust = RustDecode(Enc.data(), Enc.size());
    DecodeResult RustPub = RustDecodePublic(Enc.data(), Enc.size());
    if (!SameResult(Ref, Rust, Len) || !SameResult(Ref, RustPub, Len))
      continue;
    if (RustPub.Output.size() != Len ||
        (Len != 0 &&
         std::memcmp(RustPub.Output.data(), Data.data(), Len) != 0))
      Fail("public decode roundtrip", Len);
  }

  // Invalid input: every byte value in every position of a four-character
  // block, for a few block counts so the "last chunk" rules are exercised.
  for (size_t Blocks : {1u, 2u, 3u}) {
    for (size_t Pos = 0; Pos < Blocks * 4; ++Pos) {
      for (int Byte = 0; Byte < 256; ++Byte) {
        std::vector<char> Input(Blocks * 4, 'A');
        Input[Pos] = static_cast<char>(Byte);
        DecodeResult Ref = ReferenceDecode(Input.data(), Input.size());
        DecodeResult Rust = RustDecode(Input.data(), Input.size());
        DecodeResult RustPub = RustDecodePublic(Input.data(), Input.size());
        SameResult(Ref, Rust, Input.size());
        SameResult(Ref, RustPub, Input.size());
      }
    }
  }

  // Random inputs, both valid alphabet and arbitrary bytes, at lengths that
  // are and are not multiples of four.
  static const char Alphabet[] =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
  for (size_t Trial = 0; Trial < 20000; ++Trial) {
    size_t Len = Next() % 40;
    std::vector<char> Input(Len);
    bool Arbitrary = (Next() & 1) != 0;
    for (auto &C : Input) {
      if (Arbitrary)
        C = static_cast<char>(Next());
      else
        C = Alphabet[Next() % (sizeof(Alphabet) - 1)];
    }
    DecodeResult Ref = ReferenceDecode(Input.data(), Input.size());
    DecodeResult Rust = RustDecode(Input.data(), Input.size());
    DecodeResult RustPub = RustDecodePublic(Input.data(), Input.size());
    SameResult(Ref, Rust, Input.size());
    SameResult(Ref, RustPub, Input.size());
  }

  if (Failures)
    return 1;
  std::printf("published vectors: 7 RFC 4648 cases\n");
  std::printf("encode differential: %d lengths\n", 1025 + 9);
  std::printf("decode differential: %d valid + %d exhaustive + 20000 random\n",
              1025, (4 + 8 + 12) * 256);

  const size_t Lengths[] = {12, 48, 192, 768, 4096, 65536, 1048576};
  std::printf("\n%-14s", "encode");
  for (size_t L : Lengths)
    std::printf("%10zu", L);
  std::printf("\n");

  std::vector<double> RefEnc, RustEnc, RefDec, RustDec, RustDecOnePass;
  for (size_t L : Lengths)
    RefEnc.push_back(BenchEncode(false, Data, L, IterationsFor(L, 100'000'000)));
  for (size_t L : Lengths)
    RustEnc.push_back(BenchEncode(true, Data, L, IterationsFor(L, 100'000'000)));

  std::printf("%-14s", "C++ ns/call");
  for (double V : RefEnc)
    std::printf("%10.1f", V);
  std::printf("\n%-14s", "Rust ns/call");
  for (double V : RustEnc)
    std::printf("%10.1f", V);
  std::printf("\n%-14s", "speedup");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", RefEnc[I] / RustEnc[I]);
  std::printf("\n%-14s", "Rust MB/s");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%10.0f", Lengths[I] / RustEnc[I] * 1e3);
  std::printf("\n");

  std::vector<std::vector<char>> Encoded;
  for (size_t L : Lengths) {
    std::string E = ReferenceEncode(Data.data(), L);
    Encoded.emplace_back(E.begin(), E.end());
  }
  for (size_t I = 0; I < std::size(Lengths); ++I)
    RefDec.push_back(BenchDecode(false, false, Encoded[I],
                                 IterationsFor(Lengths[I], 100'000'000)));
  for (size_t I = 0; I < std::size(Lengths); ++I)
    RustDec.push_back(BenchDecode(true, true, Encoded[I],
                                  IterationsFor(Lengths[I], 100'000'000)));
  for (size_t I = 0; I < std::size(Lengths); ++I)
    RustDecOnePass.push_back(BenchDecode(
        true, false, Encoded[I], IterationsFor(Lengths[I], 100'000'000)));

  std::printf("\n%-14s", "decode");
  for (size_t L : Lengths)
    std::printf("%10zu", L);
  std::printf("\n%-14s", "C++ ns/call");
  for (double V : RefDec)
    std::printf("%10.1f", V);
  std::printf("\n%-14s", "Rust 1-pass");
  for (double V : RustDecOnePass)
    std::printf("%10.1f", V);
  std::printf("\n%-14s", "Rust public");
  for (double V : RustDec)
    std::printf("%10.1f", V);
  std::printf("\n%-14s", "speedup");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%9.2fx", RefDec[I] / RustDec[I]);
  std::printf("\n%-14s", "Rust MB/s");
  for (size_t I = 0; I < std::size(Lengths); ++I)
    std::printf("%10.0f", Lengths[I] / RustDec[I] * 1e3);
  std::printf("\n");

  if (Failures)
    return 1;
  std::printf("\nOK: Rust Base64 is bit-identical to the C++ reference\n");
  return 0;
}
