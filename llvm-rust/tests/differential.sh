#!/usr/bin/env bash
# Differential check: Rust bitcode writer/reader against the C++ LLVM oracle.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
llvm_major() {
    "$1/llvm-as" --version 2>/dev/null | sed -n 's/.*LLVM version \([0-9][0-9]*\)\..*/\1/p' | head -n1
}

LLVM_BIN=${LLVM_BIN:-/usr/lib/llvm-19/bin}
LLVM19_BIN=${LLVM19_BIN:-}
if [ -z "$LLVM19_BIN" ] && [ "$(llvm_major "$LLVM_BIN")" = 19 ]; then
    LLVM19_BIN=$LLVM_BIN
fi
if [ -z "$LLVM19_BIN" ] && [ -x /usr/lib/llvm-19/bin/llvm-as ]; then
    LLVM19_BIN=/usr/lib/llvm-19/bin
fi
if [ -z "$LLVM19_BIN" ]; then
    for candidate in /usr/local/llvm-19/bin /opt/llvm-19/bin; do
        if [ -x "$candidate/llvm-as" ] && [ "$(llvm_major "$candidate")" = 19 ]; then
            LLVM19_BIN=$candidate
            break
        fi
    done
fi

v2_complete() {
    [ -n "$1" ] && [ -x "$1/llvm-as" ] && [ -x "$1/llvm-dis" ] && [ -x "$1/llvm-bcanalyzer" ]
}

V2_BIN=${LLVM_V2_BIN:-}
V2_REQUIRED=0
if [ -n "$V2_BIN" ]; then
    V2_REQUIRED=1
fi
if [ -z "$V2_BIN" ]; then
    for candidate in /usr/lib/llvm-25/bin /usr/lib/llvm-24/bin /usr/lib/llvm-23/bin; do
        if v2_complete "$candidate"; then
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

# The base corpus is compared against a possibly target-aware oracle, while
# Rust still emits the legacy three-location encoding. Target memory is tested
# separately by the version-2 fixture, so ignore its spelling here.
normalize_base() {
    normalize "$1" | sed -E 's/, target_mem[01]?: [a-z]+//g'
}

status=0
for input in tests/corpus/*.ll tests/corpus/*.bc; do
    # This fixture exercises LLVM 19 three-location semantics and is checked
    # separately with LLVM19_BIN below.
    case "$input" in
        tests/corpus/aarch64_memory.ll) continue ;;
    esac
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
        if ! diff -u <(normalize_base "$cpp_ll") <(normalize_base "$read_canon") >"$TMP/$name-reader-canon.diff"; then
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
        if ! diff -u <(normalize_base "$cpp_ll") <(normalize_base "$read_canon") >"$TMP/$name-writer-fixture.diff"; then
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

    if ! diff -u <(normalize_base "$cpp_ll") <(normalize_base "$rust_ll") >"$TMP/$name-writer.diff"; then
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
    if ! diff -u <(normalize_base "$cpp_ll") <(normalize_base "$read_canon") >"$TMP/$name-reader.diff"; then
        echo "FAIL reader: $name"
        cat "$TMP/$name-reader.diff"
        status=1
    fi

    if [ "$status" -eq 0 ]; then
        echo "PASS $name"
    fi
done

if [ -x "$LLVM19_BIN/llvm-as" ] && [ -x "$LLVM19_BIN/llvm-dis" ]; then
    input=tests/corpus/aarch64_memory.ll
    name=aarch64_memory
    cpp_bc="$TMP/$name-cpp.bc"
    rust_bc="$TMP/$name-rust.bc"
    cpp_ll="$TMP/$name-cpp.ll"
    rust_ll="$TMP/$name-rust.ll"
    read_ll="$TMP/$name-read.ll"
    read_bc="$TMP/$name-read.bc"
    read_canon="$TMP/$name-read-canon.ll"

    "$LLVM19_BIN/llvm-as" "$input" -o "$cpp_bc"
    ./target/debug/llvm-as-rust --llvm19 "$input" -o "$rust_bc"
    "$LLVM19_BIN/llvm-dis" "$cpp_bc" -o "$cpp_ll"
    "$LLVM19_BIN/llvm-dis" "$rust_bc" -o "$rust_ll"
    if ! diff -u <(normalize "$cpp_ll") <(normalize "$rust_ll") >"$TMP/$name-writer.diff"; then
        echo "FAIL writer: $name (LLVM 19)"
        cat "$TMP/$name-writer.diff"
        status=1
    fi

    ./target/debug/llvm-dis-rust --llvm19 "$cpp_bc" -o "$read_ll"
    "$LLVM19_BIN/llvm-as" "$read_ll" -o "$read_bc"
    "$LLVM19_BIN/llvm-dis" "$read_bc" -o "$read_canon"
    if ! diff -u <(normalize "$cpp_ll") <(normalize "$read_canon") >"$TMP/$name-reader.diff"; then
        echo "FAIL reader: $name (LLVM 19)"
        cat "$TMP/$name-reader.diff"
        status=1
    fi

    if [ "$status" -eq 0 ]; then
        echo "PASS $name"
    fi
else
    echo "FAIL LLVM 19 AArch64 fixture: no LLVM 19 oracle (set LLVM19_BIN or use LLVM 19 as LLVM_BIN)"
    status=1
fi

v2_ready=0
if v2_complete "$V2_BIN"; then
    v2_ready=1
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
