#!/usr/bin/env bash
# Builds the Linux Mint / Ubuntu / Debian package of ForgeTerm:
#   dist/forgeterm_<version>-1_amd64.deb  (+ .sha256)
# Installs nothing; writes only to target/ and dist/.
#
# Needs: Rust (rustup), cargo-deb (`cargo install --locked cargo-deb`), dpkg-dev,
# and the build libraries listed in README.md.
#
# Usage (from the repository root):  bash scripts/package-linux.sh
# Install the result:               sudo apt install ./dist/forgeterm_<version>-1_amd64.deb
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

mkdir -p dist/linux
version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -1)

# Every crate compiled into forgeterm (normal dependencies), with its license.
{
  echo "ForgeTerm $version - Created by Erik van de Blaak"
  echo
  echo "Third-party components compiled into forgeterm (name version | license)."
  echo "Their source code is available unmodified on https://crates.io."
  echo "Terminal font: JetBrains Mono, SIL Open Font License 1.1 (JetBrainsMono-OFL.txt)."
  echo
  cargo tree -p forgeterm-app -e normal --prefix none --format "{p}|{l}" |
    grep -v '^forgeterm' | sed 's/ (\*)$//; s/|/  |  /' | sort -u
} > dist/linux/THIRD-PARTY.txt

cargo deb -p forgeterm-app --output dist/
deb=$(ls -t dist/forgeterm_"$version"-*_*.deb | head -1)
(cd dist && sha256sum "$(basename "$deb")" > "$(basename "$deb").sha256")
echo "Created $deb"
cat "$deb.sha256"
