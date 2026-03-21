# Contributing

Contributions are welcome. Please open an issue before submitting a pull request.

## Git Hooks

Install the repository-managed Git hooks once per clone:

```sh
./scripts/setup-git-hooks.sh
```

The pre-commit hook runs `cargo fmt --check` and blocks the commit if formatting is out of date.
