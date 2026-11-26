#!/usr/bin/env bash
# Differential check: Rust bitcode writer/reader against the C++ LLVM oracle.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
LLVM_BIN=${LLVM_BIN:-/usr/lib/llvm-19/bin}
V2_BIN=${LLVM_V2_BIN:-}
V2_REQUIRED=0
if [ -n "$V2_BIN" ]; then
    V2_REQUIRED=1
fi
if [ -z "$V2_BIN" ]; then
    for candidate in /usr/lib/llvm-24/bin /usr/lib/llvm-23/bin; do
        if [ -x "$candidate/llvm-as" ]; then
            V2_BIN=$candidate
            break
        fi
    done
fi
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
        ./target/debug/llvm-as-rust "$rust_ll" -o "$rust_bc"
        if ! "$LLVM_BIN/llvm-bcanalyzer" "$rust_bc" >/dev/null 2>&1; then
            echo "FAIL bcanalyzer fixture round trip: $name"
            status=1
        fi
        "$LLVM_BIN/llvm-dis" "$rust_bc" -o "$read_canon"
        if ! diff -u <(normalize "$cpp_ll") <(normalize "$read_canon") >"$TMP/$name-writer-fixture.diff"; then
            echo "FAIL writer fixture: $name"
            cat "$TMP/$name-writer-fixture.diff"
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

v2_ready=0
if [ -n "$V2_BIN" ]; then
    v2_ready=1
    for tool in llvm-as llvm-dis llvm-bcanalyzer; do
        if [ ! -x "$V2_BIN/$tool" ]; then
            v2_ready=0
        fi
    done
fi
if [ "$V2_REQUIRED" -eq 1 ] && [ "$v2_ready" -ne 1 ]; then
    echo "FAIL v2 oracle: LLVM_V2_BIN must contain llvm-as, llvm-dis, and llvm-bcanalyzer: $V2_BIN"
    status=1
fi

if [ "$v2_ready" -eq 1 ]; then
    for input in tests/corpus/v2/*.ll; do
        [ -e "$input" ] || continue
        name="v2-$(basename "$input")"
        name=${name%.*}
        cpp_bc="$TMP/$name-cpp.bc"
        rust_bc="$TMP/$name-rust.bc"
        cpp_ll="$TMP/$name-cpp.ll"
        rust_ll="$TMP/$name-rust.ll"
        read_ll="$TMP/$name-read.ll"
        read_bc="$TMP/$name-read.bc"
        read_canon="$TMP/$name-read-canon.ll"

        "$V2_BIN/llvm-as" "$input" -o "$cpp_bc"
        "$V2_BIN/llvm-dis" "$cpp_bc" -o "$cpp_ll"
        ./target/debug/llvm-as-rust "$input" -o "$rust_bc"
        "$V2_BIN/llvm-dis" "$rust_bc" -o "$rust_ll"

        if ! diff -u <(normalize "$cpp_ll") <(normalize "$rust_ll") >"$TMP/$name-writer.diff"; then
            echo "FAIL v2 writer: $name"
            cat "$TMP/$name-writer.diff"
            status=1
        fi
        if ! "$V2_BIN/llvm-bcanalyzer" "$rust_bc" >/dev/null 2>&1; then
            echo "FAIL v2 bcanalyzer writer: $name"
            status=1
        fi

        ./target/debug/llvm-dis-rust "$cpp_bc" -o "$read_ll"
        ./target/debug/llvm-as-rust "$read_ll" -o "$read_bc"
        "$V2_BIN/llvm-dis" "$read_bc" -o "$read_canon"
        if ! diff -u <(normalize "$cpp_ll") <(normalize "$read_canon") >"$TMP/$name-reader.diff"; then
            echo "FAIL v2 reader: $name"
            cat "$TMP/$name-reader.diff"
            status=1
        fi
        if [ "$status" -eq 0 ]; then
            echo "PASS $name"
        fi
    done
elif [ "$V2_REQUIRED" -eq 0 ]; then
    echo "SKIP v2 oracle (set LLVM_V2_BIN to an LLVM 23+ bin directory)"
fi

exit "$status"
