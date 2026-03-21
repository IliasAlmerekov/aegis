# Contributing

Contributions are welcome. Please open an issue before submitting a pull request.

## Git Hooks

Install the repository-managed Git hooks once per clone:

```sh
./scripts/setup-git-hooks.sh
```

The pre-push hook mirrors the CI quality gate as closely as possible:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo audit` when `cargo-audit` is installed locally
- `cargo deny check` when `cargo-deny` is installed locally

Any failing step blocks `git push`.
