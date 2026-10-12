#!/bin/sh
set -eu

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

debug_versions=$(cargo run --quiet -p greign-cli -- version --schema-only)
release_versions=$(cargo run --quiet --release -p greign-cli -- version --schema-only)
test "$debug_versions" = "$release_versions"

printf '%s\n' "all repository checks passed"

