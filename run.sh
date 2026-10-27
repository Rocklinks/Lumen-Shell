#!/bin/sh
# Convenience launcher: build from source if cargo is available, otherwise use the
# prebuilt binary shipped in this repo. Any arguments are passed straight to lumen.
#
#   ./run.sh              start the shell
#   ./run.sh --settings   open the settings panel of the running shell
set -e
cd "$(dirname "$0")"

if command -v cargo >/dev/null 2>&1 && [ -f Cargo.toml ]; then
    cargo build --release
    exec ./target/release/lumen "$@"
fi

if [ -x ./lumen-x86_64 ]; then
    echo "run.sh: cargo not found, using the prebuilt binary ./lumen-x86_64" >&2
    exec ./lumen-x86_64 "$@"
fi

echo "run.sh: neither cargo nor ./lumen-x86_64 found." >&2
echo "Install Rust from https://rustup.rs and re-run, or download a prebuilt binary." >&2
exit 1
