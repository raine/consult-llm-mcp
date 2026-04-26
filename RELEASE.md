# Releasing

Requires [`cargo-release`](https://github.com/raine/rust-release-tools) from rust-release-tools.

```bash
cargo-release --skip-publish patch   # or minor / major
```

This bumps the version in `Cargo.toml`, generates a changelog entry, commits,
tags, and pushes. GitHub Actions then builds binaries, creates the GitHub
release, publishes all three crates to crates.io, and updates the Homebrew tap.

Use `--skip-publish` because crates.io publishing is handled by CI
(`publish-crates` job) so the workspace dependency order is resolved correctly.
