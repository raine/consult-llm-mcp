# Rust project checks

set positional-arguments
set shell := ["bash", "-euo", "pipefail", "-c"]

# List available commands
default:
    @just --list

# Run all checks sequentially (order matters!)
check: clippy-fix format test

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
    cargo clippy --workspace --all-targets -- -D clippy::all

# Auto-fix clippy warnings
clippy-fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged -- -D clippy::all

# Build the project
build:
    cargo build --workspace --all-targets

# Run tests
test:
    cargo test --workspace

# Install debug binary globally via symlink
dev-install:
    cargo build && ln -sf $(pwd)/target/debug/consult-llm-mcp ~/.cargo/bin/consult-llm-mcp

# Install release binary globally
install:
    cargo install --path .

# Run the application
run *ARGS:
    cargo run -- "$@"

# Run the TUI monitor
monitor:
    cargo run --bin consult-llm-monitor
