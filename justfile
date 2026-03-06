# Rust project checks

set positional-arguments
set shell := ["bash", "-euo", "pipefail", "-c"]

# List available commands
default:
    @just --list

# Run all checks
[parallel]
check: format clippy-fix build test clippy

# Run check and fail if there are uncommitted changes (for CI)
check-ci: check
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "Error: check caused uncommitted changes"
        echo "Run 'just check' locally and commit the results"
        git diff --stat
        exit 1
    fi

# Format Rust files
format:
    cargo fmt --all

# Run clippy and fail on any warnings
clippy:
    cargo clippy -- -D clippy::all

# Auto-fix clippy warnings
clippy-fix:
    cargo clippy --fix --allow-dirty -- -W clippy::all

# Build the project
build:
    cargo build --all

# Run tests
test:
    cargo test

# Run the application
run *ARGS:
    cargo run -- "$@"
