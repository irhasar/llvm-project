//===--- Base64.h - Base64 Encoder/Decoder ----------------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// This file provides generic base64 encoder/decoder.
//
//===----------------------------------------------------------------------===//

#ifndef LLVM_SUPPORT_BASE64_H
#define LLVM_SUPPORT_BASE64_H

#include "llvm/Support/Compiler.h"
#include "llvm/Support/Error.h"
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

/// Encodes \p Len bytes from \p Src into \p Dst, which must have room for
/// `(Len + 2) / 3 * 4` bytes, and returns the number of bytes written.
/// Implemented by the Rust port when LLVM_ENABLE_RUST_BASE64 is on and by the
/// scalar fallback in Base64.cpp otherwise.
extern "C" LLVM_ABI size_t llvm_base64_encode(const void *Src, size_t Len,
                                              char *Dst);

namespace llvm {

template <class InputBytes> std::string encodeBase64(InputBytes const &Bytes) {
  std::string Buffer;
  Buffer.resize(((Bytes.size() + 2) / 3) * 4);
  if (!Buffer.empty())
    llvm_base64_encode(Bytes.data(), Bytes.size(), Buffer.data());
  return Buffer;
}

LLVM_ABI llvm::Error decodeBase64(llvm::StringRef Input,
                                  std::vector<char> &Output);

} // end namespace llvm

#endif
