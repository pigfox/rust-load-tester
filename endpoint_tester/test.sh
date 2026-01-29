#!/usr/bin/env sh
set -eu
#test.sh
clear
cargo test
#cargo install cargo-llvm-cov
cargo llvm-cov --workspace --lcov --output-path lcov.info
cargo llvm-cov --workspace --summary-only
