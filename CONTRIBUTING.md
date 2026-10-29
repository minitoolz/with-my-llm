# Contributing

See the [README](README.md) for installation and usage. Contributor documentation covers development, releases, and implementation details:

- [Publishing releases](docs/releasing.md)
- [Implementation notes](docs/implementation.md): updates, port selection, model discovery, and connection recovery.
- [Agent instructions](AGENTS.md): repository rules for coding agents.

## Local checks

Before creating a commit, read [.github/workflows/ci.yml](.github/workflows/ci.yml) and run its checks locally:

```sh
sh -n install-wmlm.sh
python3 scripts/test_installer.py
cargo fmt --all -- --check
cargo test --locked
cargo build --locked
python3 scripts/smoke_tui.py
cargo clippy --locked --all-targets -- -D warnings
```

Build before running the TUI smoke test so it exercises the current source. Fix failures and rerun affected checks before committing. Keep these commands aligned with the workflow.

CI also checks Rust 1.86 compatibility, installation without a locked dependency resolution, and platform builds. Report any checks you could not run locally; do not claim they passed. If you push a commit, check its GitHub Actions run and investigate failures caused by your changes.

## Documentation audiences

Keep the README focused on people installing, configuring, and using with-my-llm. It should explain common workflows, relevant limitations, and troubleshooting in user-facing terms.

Put contributor workflows and technical implementation details in this file or linked documents under `docs/`. Put instructions for coding agents in `AGENTS.md`. A brief contributor link in the README is sufficient.
