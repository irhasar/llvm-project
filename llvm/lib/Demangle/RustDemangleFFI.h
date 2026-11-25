//===--- RustDemangleFFI.h - Rust demangler FFI declarations ----*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Private declarations shared by the Rust demangler wrapper and its tests.
// The return type must match Rust's isize: intptr_t, not int64_t.
//
//===----------------------------------------------------------------------===//

#ifndef LLVM_LIB_DEMANGLE_RUSTDEMANGLEFFI_H
#define LLVM_LIB_DEMANGLE_RUSTDEMANGLEFFI_H

#include <cstddef>
#include <cstdint>

extern "C" std::intptr_t llvm_rustdemangle_demangle(
    const std::uint8_t *Input, std::size_t InputLen, std::uint8_t **Output);

#endif
