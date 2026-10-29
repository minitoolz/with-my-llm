#!/bin/sh
# Install the latest stable with-my-llm release on Linux or macOS.
set -eu

main() {
    fail() { printf 'Error: %s\n' "$*" >&2; exit 1; }

    for command in curl tar mktemp uname; do
        command -v "$command" >/dev/null 2>&1 || fail "Required command not found: $command"
    done
    case "$(uname -s)" in
        Linux) platform=unknown-linux-musl ;;
        Darwin) platform=apple-darwin ;;
        *) fail 'Only Linux and macOS are supported.' ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        arm64|aarch64) arch=aarch64 ;;
        *) fail 'Only x86_64 and ARM64 are supported.' ;;
    esac
    if command -v sha256sum >/dev/null 2>&1; then
        checksum_tool=sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        checksum_tool=shasum
    else
        fail 'Install sha256sum or shasum to verify the download.'
    fi

    install_dir=${WMLM_INSTALL_DIR:-"$HOME/.local/bin"}
    case "$install_dir" in
        /*) ;;
        *) fail 'WMLM_INSTALL_DIR must be an absolute path.' ;;
    esac
    [ ! -d "$install_dir/wmlm" ] || fail "$install_dir/wmlm is a directory."
    work_dir=$(mktemp -d "${TMPDIR:-/tmp}/wmlm-install.XXXXXX")
    stage_dir=
    trap 'rm -rf "$work_dir"; if [ -n "$stage_dir" ]; then rm -rf "$stage_dir"; fi' EXIT
    trap 'exit 1' HUP INT TERM

    archive="wmlm-$arch-$platform.tar.gz"
    printf 'Downloading latest wmlm for %s-%s...\n' "$arch" "$platform"
    # Resolve the tag once so both assets come from the same release.
    release_url=$(curl --proto '=https' --tlsv1.2 -fLsS -o /dev/null \
        -w '%{url_effective}' https://github.com/minitoolz/with-my-llm/releases/latest) || fail 'Could not find the latest release.'
    case "$release_url" in
        https://github.com/minitoolz/with-my-llm/releases/tag/v*) ;;
        *) fail 'Unexpected latest release URL.' ;;
    esac
    tag=${release_url##*/}
    base_url="https://github.com/minitoolz/with-my-llm/releases/download/$tag"
    curl --proto '=https' --tlsv1.2 -fLsS -o "$work_dir/$archive" "$base_url/$archive" || fail 'Could not download the release archive.'
    curl --proto '=https' --tlsv1.2 -fLsS -o "$work_dir/checksum" "$base_url/$archive.sha256" || fail 'Could not download the release checksum.'
    read -r expected remainder < "$work_dir/checksum" || fail 'Invalid checksum file.'
    case "$expected" in
        ''|*[!0-9a-fA-F]*) fail 'Invalid SHA-256 checksum.' ;;
    esac
    [ "${#expected}" -eq 64 ] || fail 'Invalid SHA-256 checksum length.'
    if [ "$checksum_tool" = sha256sum ]; then
        actual=$(sha256sum "$work_dir/$archive")
    else
        actual=$(shasum -a 256 "$work_dir/$archive")
    fi
    [ "${actual%% *}" = "$expected" ] || fail 'Checksum mismatch; installation cancelled.'

    [ "$(tar -tzf "$work_dir/$archive")" = wmlm ] || fail 'Unexpected archive contents.'
    case "$(tar -tvzf "$work_dir/$archive")" in
        -*) ;;
        *) fail 'The archive must contain a regular wmlm file.' ;;
    esac
    mkdir -p "$install_dir"
    stage_dir=$(mktemp -d "$install_dir/.wmlm-install.XXXXXX")
    tar -xOzf "$work_dir/$archive" wmlm > "$stage_dir/wmlm"
    [ -s "$stage_dir/wmlm" ] || fail 'The downloaded executable is empty.'
    chmod 755 "$stage_dir/wmlm"
    mv -f "$stage_dir/wmlm" "$install_dir/wmlm"
    printf 'Installed wmlm %s to %s/wmlm\n' "$tag" "$install_dir"
    case ":$PATH:" in
        *":$install_dir:"*) ;;
        *) printf 'Add this directory to your PATH: %s\n' "$install_dir" ;;
    esac
    printf 'Run wmlm cfg to get started.\n'
}

# Keep execution last so a truncated piped download cannot partially run main.
main "$@"
