#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p greign-app
exec ./target/release/greign-app "$@"
