# Rust project checks

set positional-arguments
set shell := ["bash", "-euo", "pipefail", "-c"]

# List available commands
default:
    @just --list

# Run all checks sequentially (order matters!)
check: clippy format test

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
    @cargo fmt --all

# Auto-fix clippy warnings, then fail on any remaining
clippy:
    @cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged --quiet -- -D clippy::all 2>&1 | { grep -v "^0 errors" || true; }

# Build the project
build:
    cargo build --workspace --all-targets

# Run tests
test:
    #!/usr/bin/env bash
    set -euo pipefail
    output=$(cargo test --workspace --quiet 2>&1) || { echo "$output"; exit 1; }
    echo "$output" | tail -1

# Install debug binaries globally via symlink
install-dev:
    cargo build && ln -sf $(pwd)/target/debug/consult-llm-mcp ~/.cargo/bin/consult-llm-mcp && ln -sf $(pwd)/target/debug/consult-llm-monitor ~/.cargo/bin/consult-llm-monitor

# Install release binaries globally
install:
    cargo install --offline --path . --locked
    cargo install --offline --path crates/monitor --locked

# Run the application
run *ARGS:
    cargo run -- "$@"

# Run the TUI monitor
monitor:
    cargo run -p consult-llm-monitor
