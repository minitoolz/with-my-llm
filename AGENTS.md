# Agent instructions

## CI checks before committing

Before creating a commit, read `.github/workflows/ci.yml` and run the CI checks
locally. Keep the commands aligned with the workflow as it changes. The current
checks are:

```sh
sh -n install-wmlm.sh
python3 scripts/test_installer.py
cargo fmt --all -- --check
cargo test --locked
cargo build --locked
python3 scripts/smoke_tui.py
cargo clippy --locked --all-targets -- -D warnings
```

Build before running the TUI smoke test so it exercises the current source.
Fix failures and rerun the affected checks before committing. Report any checks
that could not run locally, including Rust 1.86 compatibility or platform-specific
builds; do not claim those checks passed.

If the commit is pushed, check the GitHub Actions run for that commit and report
its status. Investigate and fix CI failures caused by the changes.

## README audience: strict rule

`README.md` must remain human-user-centric. Only include information that helps
people understand, install, configure, use, update, or troubleshoot with-my-llm.
Describe observable behavior and practical choices in user-facing terms.

Do not add coding-agent instructions, contributor workflows, CI checklists,
release-publishing procedures, internal architecture, implementation algorithms,
or exhaustive parsing and edge-case specifications to the README. Instructions
for coding agents belong in `AGENTS.md`; contributor workflows belong in
`CONTRIBUTING.md` or linked documents under `docs/`; implementation details belong
in `docs/implementation.md`; release procedures belong in `docs/releasing.md`.

When documenting a feature or fix, keep only the user-relevant behavior, setup,
limitations, and troubleshooting in the README. Put supporting technical details
in the appropriate contributor document and keep them accurate when behavior
changes. A brief link to `CONTRIBUTING.md` is allowed; do not duplicate its contents
in the README. Review every README change against this rule before finishing.
