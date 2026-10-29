# Contributing

See the [README](README.md) for installation and usage. Contributor documentation covers development, releases, and implementation details:

- [Publishing releases](docs/releasing.md)
- [Implementation notes](docs/implementation.md): updates, port selection, model discovery, and connection recovery.
- [Agent instructions](AGENTS.md): repository rules for coding agents.

## Branching and pull requests

Follow these rules for every repository change, including documentation and
agent instructions. Inspect `git status` and the current branch before editing;
preserve unrelated user changes and never commit them as part of your task.

### Choose a task branch

- Never develop or commit directly on `main` or `dev`. Use one short-lived branch
  per focused task, named `feat/<short-description>`, `fix/<short-description>`,
  or `chore/<short-description>`. Use `chore/` for documentation, maintenance,
  refactoring, and CI changes. Prefer lowercase, hyphen-separated descriptions.
- Do not create other branch prefixes, including `release/`, `releases/`,
  `hotfix/`, or `codex/`. Release tags are separate from branch naming rules;
  follow `docs/releasing.md` when a release is requested.
- Fetch the remote before choosing a base when access is available. Normally
  branch from `origin/dev` and target `dev` with the PR. If `dev` does not exist,
  branch from `origin/main` and target `main`; do not create the permanent `dev`
  branch as a side effect of an ordinary task. If fetching fails, report that
  remote freshness and branch existence are unverified, and use the available
  remote-tracking base for local work.
- Continue on an existing task branch only when it belongs to the current task.
  If switching would mix unrelated work, use an isolated worktree or ask for
  direction. Never reset or stash user changes without authorization.
- A task branch must not track `origin/main` or `origin/dev`. When publishing,
  push explicitly to the same task-branch name, for example
  `git push -u origin chore/branching-policy`.

### Integrate through pull requests

- Both `main` and `dev` require PRs and CI. Never push commits directly to either
  branch, force-push either branch, delete either branch, or weaken/bypass rules
  to complete a task. Administrator access does not waive the separate PR/CI
  ruleset. Only repository administrators may merge into `main`.
- When `dev` exists, promote tested work through a `dev` -> `main` PR. Prefer a
  merge commit for this long-lived branch promotion so shared ancestry is
  preserved. Merge commits, squash, and rebase are all permitted by the current
  policy; prefer squash for focused task PRs. Do not squash or rebase the
  long-lived promotion branch as a routine integration strategy.
- For an explicitly requested urgent production fix, branch `fix/<description>`
  from `origin/main` and target `main`. If `dev` exists, follow up with a PR that
  brings the fix from `main` into `dev`.
- Keep the PR branch up to date with its target. Prefer merging the target into
  a published/shared task branch rather than rewriting published history. For
  a promotion PR, bring any missing `main` changes into `dev` through a separate
  PR first. Never resolve divergence with a force-push to a permanent branch.
- Required checks must pass on the current PR revision, and the branch must be
  up to date before merging. The current required GitHub Actions check names
  are listed below; verify them against `.github/workflows/ci.yml` and current
  repository rules when those change:

  ```text
  Check (ubuntu-24.04)
  Check (macos-15)
  Check (macos-15-intel)
  Rust 1.86 compatibility
  Build x86_64-unknown-linux-musl
  Build aarch64-unknown-linux-musl
  Build x86_64-apple-darwin
  Build aarch64-apple-darwin
  ```

- Zero approvals are currently required. This does not waive CI or authorize an
  agent to merge. Address review feedback and resolve discussions before merge
  even though discussion resolution is not enforced by the ruleset. Publish or
  merge only within the user's authorized scope.
- After a PR is merged, delete its task branch when authorized and no longer
  needed. Never delete `main` or `dev`. Disallowed branch names may be deleted
  for cleanup, but cannot be created or updated under the current rules.

## CI triggers

CI runs for pull requests targeting `dev` or `main`, release tags matching `v*`,
and manual workflow runs. Ordinary branch pushes do not trigger a separate run;
open a PR to run the required checks before merging. Pushing another commit to
an open PR starts a new run and cancels its outdated run.

All eight check and build jobs still run for each PR. Release and manual runs
are not canceled by newer PR runs. Use a manual run when validation is needed
before opening a PR. Required-check names and branch protections are unchanged.

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
