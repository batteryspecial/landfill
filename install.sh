#!/bin/sh
set -e
cargo install --path .
mkdir -p "$HOME/.cargo/share/man/man1"
cp landfill.1 "$HOME/.cargo/share/man/man1/"
