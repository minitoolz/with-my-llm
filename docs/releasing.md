# Publishing releases

Set the version in `Cargo.toml` and update `Cargo.lock`, then push a matching stable tag such as `v0.0.2`. CI validates the tag, runs its checks, and builds all four platform archives. Linux builds are checked for ELF interpreters and shared library dependencies. After all builds succeed, CI uploads the archives and their checksums to a draft GitHub Release and publishes it as latest. An interrupted draft upload can be retried; an already published release is never overwritten.

The updater expects `wmlm-<target>.tar.gz` containing exactly one regular file named `wmlm`, plus `wmlm-<target>.tar.gz.sha256`. The checksum detects corrupted downloads; it is not an independent publisher signature.

See [contributor checks](../CONTRIBUTING.md) before creating a release commit. The release pipeline is defined in [ci.yml](../.github/workflows/ci.yml).

## Installation script

The repository-root `install-wmlm.sh` installs the latest GitHub Release using the same archive names and SHA-256 files as the updater. Keep these assets compatible with both clients. The README links to the script on the `main` branch through `raw.githubusercontent.com`; that URL requires the script to be published on that branch. No GitHub Pages site is required.

The installer resolves the latest release tag once, verifies the archive checksum and contents, then stages the executable in the destination filesystem before replacing it. Its default destination is `~/.local/bin`; `WMLM_INSTALL_DIR` overrides it. Offline fixture tests run with `python3 scripts/test_installer.py` and are included in CI on Linux and macOS.
