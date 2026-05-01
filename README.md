# llm-proxy-app

Cross-platform system tray app for [llm-proxy](https://github.com/maplezzk/llm-proxy).

Built with [Tauri](https://tauri.app/) — lightweight (~20MB), single binary with embedded proxy.

## Features

- 🖥️ **System tray** on macOS, Windows, and Linux
- 🚀 **Auto-starts** llm-proxy on app launch
- 🔗 **One click** opens Admin UI in browser
- 📦 **Single install** — no Node.js required (llm-proxy compiled to standalone binary)

## Install

Download from [Releases](https://github.com/maplezzk/llm-proxy-app/releases).

## Development

```bash
# Install dependencies
npm install

# Build llm-proxy binary (requires bun)
./scripts/build-proxy.sh

# Run in dev mode
npm run dev

# Build for distribution
npm run build
```

## Architecture

```
llm-proxy-app
├── src/                    # Web frontend (landing page)
├── src-tauri/              # Rust backend
│   ├── binaries/           # Bundled llm-proxy binary
│   └── src/
│       └── lib.rs          # Tray + process management
└── scripts/
    └── build-proxy.sh      # Build llm-proxy with bun
```
