#!/bin/bash
# Build llm-proxy as standalone binary using bun
# Usage: ./scripts/build-proxy.sh [target-triple]
#   e.g. ./scripts/build-proxy.sh aarch64-apple-darwin

set -euo pipefail

TARGET="${1:-$(rustc -vV | grep host | cut -d' ' -f2)}"
LLM_PROXY_DIR="../llm-proxy"
OUT_DIR="src-tauri/binaries"

echo "Building llm-proxy for $TARGET..."

# Build with bun
cd "$LLM_PROXY_DIR"

# Generate single-file bundle first, then compile
npx esbuild src/index.ts --bundle --platform=node --outfile=dist/bundle.js --format=esm

# Compile to standalone binary
bun build --compile --target "bun-$TARGET" ./dist/bundle.js --outfile "llm-proxy"

cd -

# Copy to binaries directory
cp "$LLM_PROXY_DIR/llm-proxy" "$OUT_DIR/llm-proxy-${TARGET}"
chmod +x "$OUT_DIR/llm-proxy-${TARGET}"

# Copy admin UI files alongside binary
cp "$LLM_PROXY_DIR/dist/api/admin-ui.html" "$OUT_DIR/admin-ui.html"
cp "$LLM_PROXY_DIR/dist/api/admin-app.js" "$OUT_DIR/admin-app.js"
cp "$OUT_DIR/admin-ui.html" src-tauri/target/debug/binaries/admin-ui.html 2>/dev/null || true
cp "$OUT_DIR/admin-app.js" src-tauri/target/debug/binaries/admin-app.js 2>/dev/null || true

# Also link for dev mode
ln -sf "$OUT_DIR/llm-proxy-${TARGET}" "$OUT_DIR/llm-proxy"
mkdir -p src-tauri/target/debug/binaries
ln -sf "$(pwd)/$OUT_DIR/llm-proxy-${TARGET}" src-tauri/target/debug/binaries/llm-proxy

echo "Done: $OUT_DIR/llm-proxy-${TARGET}"
