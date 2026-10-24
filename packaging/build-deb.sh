#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."
if ! command -v cargo-deb >/dev/null 2>&1; then
  echo "Instale cargo-deb no ambiente Rust que você usa: cargo install cargo-deb" >&2
  exit 1
fi
cargo deb
echo
echo "Instalar a partir da raiz do repositório:"
echo "  sudo apt install ./target/debian/openperipheral_0.2.0-1_amd64.deb"
