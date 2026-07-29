#!/bin/sh
# Protein installer — curl -fsSL https://raw.githubusercontent.com/vianch/protein/main/install.sh | sh
set -eu

REPO="https://github.com/vianch/protein"
TAG="${PROTEIN_VERSION:-}"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "protein is macOS-only: it shells out to caffeinate(8) and pmset(1)." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install stable Rust first:" >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  exit 1
fi

echo "==> building protein from source (this takes a minute)"
if [ -n "$TAG" ]; then
  cargo install --git "$REPO" --tag "$TAG" --bin caf --force
else
  cargo install --git "$REPO" --bin caf --force
fi

BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
echo "==> installed $BIN_DIR/caf"

case ":$PATH:" in
  *":$BIN_DIR:"*) echo "==> run: caf" ;;
  *) echo "==> $BIN_DIR is not on your PATH. Add it:"
     echo "     echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc && exec zsh" ;;
esac
