#!/usr/bin/env bash
# Remove the installed agentsessions binary.
#
# Deletes exactly one file: agentsessions in the install directory. It never
# removes a directory recursively and never touches a data root, so it cannot
# take your config, cache, or indexed history with it. Running it when nothing
# is installed reports "not installed" and exits 0, which makes it safe to
# repeat and safe to call from CI.

set -euo pipefail

fail() {
    printf 'uninstall: error: %s\n' "$1" >&2
    exit 1
}

usage() {
    cat <<'EOF'
usage: uninstall.sh [--prefix <dir>]

  --prefix <dir>  install directory (default: ${XDG_BIN_HOME:-$HOME/.local/bin})
EOF
}

prefix=""

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || fail '--prefix requires a directory'
            prefix="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown argument: $1"
            ;;
    esac
done

binary_name="agentsessions"

if [ -z "$prefix" ]; then
    if [ -n "${XDG_BIN_HOME:-}" ]; then
        prefix="$XDG_BIN_HOME"
    elif [ -n "${HOME:-}" ]; then
        prefix="$HOME/.local/bin"
    else
        fail 'neither XDG_BIN_HOME nor HOME is set; pass --prefix <dir>'
    fi
fi

installed_binary="$prefix/$binary_name"

if [ ! -e "$installed_binary" ]; then
    printf 'not installed: %s\n' "$installed_binary"
    exit 0
fi

rm -f "$installed_binary" || fail "cannot remove $installed_binary"
printf 'removed: %s\n' "$installed_binary"
printf '\n'
printf 'Your config, data, cache, and logs were not touched. To remove those,\n'
printf 'delete the paths reported by: agentsessions --robot config paths\n'
