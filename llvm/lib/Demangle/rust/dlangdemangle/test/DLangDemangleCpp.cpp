//===-- DLangDemangleCpp.cpp - Renamed C++ reference demangler ------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Build the original C++ implementation under a second symbol so the
// differential harness can compare it with the public Rust-backed
// llvm::dlangDemangle in the same executable.
//
//===----------------------------------------------------------------------===//

#undef LLVM_RUST_DLANG
#define dlangDemangle dlangDemangleCpp
#include "../../../DLangDemangle.cpp"
