# Contributing to Gloss

Thanks for helping improve Gloss.

## Before opening a change

- Search existing issues and pull requests.
- For substantial behavior or interface changes, open an issue first so the
  design can be discussed.
- Keep changes focused and include tests for observable behavior.
- Update sibling `.gloss` metadata for every touched file.

## Development setup

Gloss requires a stable Rust toolchain and Git.

```console
git clone https://github.com/ArchAstro/gloss.git
cd gloss
cargo build --locked
cargo test --locked --all-targets --all-features
```

Before opening a pull request, run the same core checks as CI:

```console
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo test --locked --all-targets --all-features
cargo run --locked -- lint
cargo audit
```

Install `cargo-audit` with `cargo install cargo-audit --locked` if needed.

## Pull requests

Explain the problem, the chosen solution, and how you verified it. Update
documentation when behavior or configuration changes. By contributing, you
agree that your work is licensed under the repository's MIT license.

Be respectful and follow the [Code of Conduct](CODE_OF_CONDUCT.md).
