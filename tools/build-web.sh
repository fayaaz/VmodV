#!/usr/bin/env bash
# Build the VmodV browser shell (wasm + wasm-bindgen glue) into web/pkg.
#
# Runs inside the Rust toolchain container (see `make web-build`).
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
WASM_VERSION=${WASM_BINDGEN_VERSION:-0.2.130}
OUT_DIR="$ROOT/web/pkg"
CARGO_HOME=${CARGO_HOME:-/usr/local/cargo}
export CARGO_HOME

cd "$ROOT/crates"

rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p modv-web

BINDGEN=$(command -v wasm-bindgen || true)
if [[ -z "$BINDGEN" || ! -x "$BINDGEN" ]]; then
  cargo install wasm-bindgen-cli --version "$WASM_VERSION" --locked
  BINDGEN="$CARGO_HOME/bin/wasm-bindgen"
fi

mkdir -p "$OUT_DIR"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target}
"$BINDGEN" --target web --out-dir "$OUT_DIR" --out-name modv_web \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/modv_web.wasm"

echo "build-web: wrote $OUT_DIR/modv_web.{js,wasm}"
ls -lh "$OUT_DIR"
