#!/usr/bin/env bash
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
binary="$project_dir/target/release/beacon"
destination=/usr/bin/beacon

if [[ ! -f "$binary" || ! -x "$binary" ]]; then
    echo 'Build the executable first: cargo build --release --locked' >&2
    exit 1
fi

if [[ -L "$destination" && "$(readlink -- "$destination")" == "$binary" ]]; then
    echo "$destination already links to $binary"
    exit 0
fi

if [[ -e "$destination" || -L "$destination" ]]; then
    echo "Refusing to replace existing $destination" >&2
    exit 1
fi

if (( EUID == 0 )); then
    ln -sT -- "$binary" "$destination"
else
    sudo ln -sT -- "$binary" "$destination"
fi

echo "Linked $destination -> $binary"
