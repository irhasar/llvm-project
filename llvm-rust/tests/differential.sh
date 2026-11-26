#!/usr/bin/env bash
# Differential check: Rust bitcode writer/reader against the C++ LLVM oracle.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
LLVM_BIN=${LLVM_BIN:-/usr/lib/llvm-19/bin}
CARGO=${CARGO:-cargo}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

cd "$ROOT"
"$CARGO" build -p llvm-rust-tools --bin llvm-as-rust --bin llvm-dis-rust >/dev/null

normalize() {
    grep -v '^source_filename\|^; ModuleID' "$1"
}

status=0
for input in tests/corpus/*.ll tests/corpus/*.bc; do
    name=$(basename "$input")
    name=${name%.*}
    cpp_bc="$TMP/$name-cpp.bc"
    rust_bc="$TMP/$name-rust.bc"
    cpp_ll="$TMP/$name-cpp.ll"
    rust_ll="$TMP/$name-rust.ll"
    read_ll="$TMP/$name-read.ll"
    read_bc="$TMP/$name-read.bc"
    read_canon="$TMP/$name-read-canon.ll"

    if [[ "$input" == *.bc ]]; then
        "$LLVM_BIN/llvm-dis" "$input" -o "$cpp_ll"
        ./target/debug/llvm-dis-rust "$input" -o "$rust_ll"
        if ! "$LLVM_BIN/llvm-bcanalyzer" "$input" >/dev/null 2>&1; then
            echo "FAIL bcanalyzer fixture: $name"
            status=1
        fi
        "$LLVM_BIN/llvm-as" "$rust_ll" -o "$read_bc"
        "$LLVM_BIN/llvm-dis" "$read_bc" -o "$read_canon"
        if ! diff -u <(normalize "$cpp_ll") <(normalize "$read_canon") >"$TMP/$name-reader-canon.diff"; then
            echo "FAIL reader fixture canonical: $name"
            cat "$TMP/$name-reader-canon.diff"
            status=1
        fi
        if [ "$status" -eq 0 ]; then
            echo "PASS $name"
        fi
        continue
    fi

    "$LLVM_BIN/llvm-as" "$input" -o "$cpp_bc"
    ./target/debug/llvm-as-rust "$input" -o "$rust_bc"
    "$LLVM_BIN/llvm-dis" "$cpp_bc" -o "$cpp_ll"
    "$LLVM_BIN/llvm-dis" "$rust_bc" -o "$rust_ll"

    if ! diff -u <(normalize "$cpp_ll") <(normalize "$rust_ll") >"$TMP/$name-writer.diff"; then
        echo "FAIL writer: $name"
        cat "$TMP/$name-writer.diff"
        status=1
    fi

    if ! "$LLVM_BIN/llvm-bcanalyzer" "$rust_bc" >/dev/null 2>&1; then
        echo "FAIL bcanalyzer writer: $name"
        status=1
    fi

    ./target/debug/llvm-dis-rust "$cpp_bc" -o "$read_ll"
    "$LLVM_BIN/llvm-as" "$read_ll" -o "$read_bc"
    "$LLVM_BIN/llvm-dis" "$read_bc" -o "$read_canon"
    if ! "$LLVM_BIN/llvm-bcanalyzer" "$read_bc" >/dev/null 2>&1; then
        echo "FAIL bcanalyzer reader: $name"
        status=1
    fi
    if ! diff -u <(normalize "$cpp_ll") <(normalize "$read_canon") >"$TMP/$name-reader.diff"; then
        echo "FAIL reader: $name"
        cat "$TMP/$name-reader.diff"
        status=1
    fi

    if [ "$status" -eq 0 ]; then
        echo "PASS $name"
    fi
done

exit "$status"
