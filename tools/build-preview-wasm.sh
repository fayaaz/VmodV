#!/usr/bin/env bash
# Build the WebAssembly preview runtime used by the GitHub Pages catalogue.
#
# Runs inside the Rust toolchain container (see `make preview-wasm`). The
# output is committed under docs/assets/wasm so Pages can serve the site as
# plain static files; CI rebuilds it to catch drift.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
WASM_VERSION=${WASM_BINDGEN_VERSION:-0.2.130}
OUT_DIR="$ROOT/docs/assets/wasm"
CARGO_HOME=${CARGO_HOME:-/usr/local/cargo}
export CARGO_HOME

cd "$ROOT/crates"

rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p modv-preview-wasm

BINDGEN=$(command -v wasm-bindgen || true)
if [[ -z "$BINDGEN" || ! -x "$BINDGEN" ]]; then
  cargo install wasm-bindgen-cli --version "$WASM_VERSION" --locked
  BINDGEN="$CARGO_HOME/bin/wasm-bindgen"
fi

mkdir -p "$OUT_DIR"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target}
"$BINDGEN" --target web --out-dir "$OUT_DIR" --out-name modv-preview \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/modv_preview_wasm.wasm"

echo "build-preview-wasm: wrote $OUT_DIR/modv-preview.{js,wasm}"
ls -lh "$OUT_DIR"
