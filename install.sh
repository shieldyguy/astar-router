#!/usr/bin/env bash
# install.sh: put the `astar` binary on this machine.
#
#     ./install.sh                 build it here (cargo, about a minute, no dependencies to fetch) and copy it
#                                  to ~/.local/bin/astar; without cargo, take the newest release's binary
#     ./install.sh --dest DIR      copy it to DIR/astar instead
#     ./install.sh --release       take the release binary even where cargo is found (needs `gh`, logged in)
#     ./install.sh --which         say what it would do, and do nothing
#
# Releases are made when a tag `v*` is pushed (.github/workflows/release.yml): Linux x86_64 and macOS arm64.
# It ends with a self-check: one small scene routed, every net joined.

set -eu
cd "$(dirname "$0")"
dest="$HOME/.local/bin"
from=auto
which=0
while [ $# -gt 0 ]; do
    case "$1" in
        --dest) dest="$2"; shift ;;
        --release) from=release ;;
        --which) which=1 ;;
        -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "install.sh: unknown argument $1" >&2; exit 1 ;;
    esac
    shift
done

if [ "$from" = auto ]; then
    if command -v cargo > /dev/null 2>&1; then from=source; else from=release; fi
fi
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset=astar-linux-x86_64.tar.gz ;;
    Darwin-arm64) asset=astar-macos-arm64.tar.gz ;;
    *) asset="" ;;
esac

if [ "$which" = 1 ]; then
    if [ "$from" = source ]; then echo "install.sh: would build with cargo and copy target/release/astar to $dest/astar"
    else echo "install.sh: would take ${asset:-no binary (this platform has no release)} from the newest release into $dest/astar"; fi
    exit 0
fi

mkdir -p "$dest"
if [ "$from" = source ]; then
    cargo build --release
    cp target/release/astar "$dest/astar"
else
    if [ -z "$asset" ]; then
        echo "install.sh: no release for $(uname -s) $(uname -m): install Rust (rustup.rs) and run ./install.sh" >&2
        exit 1
    fi
    if ! command -v gh > /dev/null 2>&1; then
        echo "install.sh: no cargo and no gh here: install Rust (rustup.rs), or gh to fetch a release" >&2
        exit 1
    fi
    tmp=$(mktemp -d)
    gh release download --repo shieldyguy/astar-router --pattern "$asset" --dir "$tmp"
    tar -xzf "$tmp/$asset" -C "$tmp"
    cp "$tmp/astar" "$dest/astar"
    rm -rf "$tmp"
fi
chmod +x "$dest/astar"

# the self-check: a small scene, every net joined (exit 0)
out=$(mktemp)
if "$dest/astar" route tests/scenes/tiny11.json "$out" > /dev/null 2>&1; then
    echo "install.sh: $dest/astar is in place and routes (tests/scenes/tiny11.json: every net joined)"
else
    echo "install.sh: $dest/astar is in place but the self-check failed: run '$dest/astar route tests/scenes/tiny11.json /tmp/plan.json'" >&2
    rm -f "$out"
    exit 1
fi
rm -f "$out"
case ":$PATH:" in
    *":$dest:"*) ;;
    *) echo "install.sh: $dest is not on PATH: call it by its full path, or add 'export PATH=\"$dest:\$PATH\"'" ;;
esac
