#!/usr/bin/env bash
# What CI runs, stopping at the first failure. Run before every commit.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo shear
cargo nextest run --workspace
cargo test --workspace --doc
