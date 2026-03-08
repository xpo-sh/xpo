<p align="center">
  <a href="https://xpo.sh"><img src="assets/banner.png" alt="xpo.sh - Expose local services via secure tunnels" /></a>
</p>

<p align="center">
  <a href="https://xpo.sh"><img src="https://img.shields.io/badge/website-xpo.sh-22d3ee?style=flat-square" alt="Website" /></a>
  <a href="https://crates.io/crates/xpo"><img src="https://img.shields.io/crates/v/xpo?style=flat-square&color=a78bfa" alt="Crates.io" /></a>
  <a href="https://github.com/xpo-sh/xpo/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-4ade80?style=flat-square" alt="License" /></a>
  <a href="https://github.com/xpo-sh/xpo"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-facc15?style=flat-square" alt="Platform" /></a>
</p>

---

**xpo** is an open-source tunneling tool that exposes local services to the internet via secure tunnels. Built in Rust for maximum performance.

## What's available now

`xpo dev` - local HTTPS domains for development. Like `localhost:3000`, but with real HTTPS and a clean `.test` domain.

```bash
$ xpo dev setup
  ✓ Root CA created (P-256 ECDSA, 10yr)
  ✓ CA trusted in system keychain
  ✓ Port forwarding active  443→10443, 80→10080
  Setup complete! Run: xpo dev 3000 -n myapp

$ xpo dev 3000 -n myapp
  → https://myapp.test  →  localhost:3000
  Ctrl+C to stop

  GET / 200 12ms
  GET /_nuxt/ 101 42ms
  GET /favicon.ico 304 3ms
```

## Install

```bash
# One-liner (macOS / Linux)
curl -fsSL https://xpo.sh/install | sh

# Cargo
cargo install xpo

# Homebrew (coming soon)
brew tap xpo-sh/tap && brew install xpo
```

## Quick start

```bash
# 1. One-time setup (generates local CA, requires sudo)
xpo dev setup

# 2. Start HTTPS proxy for your dev server
xpo dev 3000 -n myapp       # https://myapp.test → localhost:3000
xpo dev 5173 -n frontend    # https://frontend.test → localhost:5173
xpo dev 8080 -n api         # https://api.test → localhost:8080

# 3. Clean up orphaned entries (if process was killed)
xpo dev stop

# 4. Full uninstall (remove CA, trust, port forwarding)
xpo dev uninstall
```

## Features

- **Real HTTPS** - trusted certificates, no browser warnings
- **`.test` domains** - IANA reserved, never conflicts with real domains
- **WebSocket support** - HMR/hot-reload works out of the box
- **Request logging** - colored `METHOD /path STATUS ms` in terminal
- **Error pages** - branded 502/504 pages when upstream is down
- **Fast** - Rust + tokio, sub-millisecond proxy overhead
- **Zero config** - one `setup`, then just `xpo dev <port> -n <name>`

## Coming soon

- `xpo share` - public tunnels (`https://myapp.xpo.sh → localhost:3000`)
- Local dashboard with request inspector
- Webhook replay
- And more → [roadmap](https://github.com/xpo-sh/xpo/issues)

## Platform support

| Platform | `xpo dev` | `xpo share` |
|---|---|---|
| macOS (ARM + Intel) | ✅ | ✅ |
| Linux (x86_64 + ARM) | ✅ | ✅ |
| Windows | ✗ | ✅ |

## License

[MIT](LICENSE)

<p align="center">
  <a href="https://xpo.sh">xpo.sh</a> · <a href="https://x.com/getxposh">X/Twitter</a> · <a href="https://github.com/xpo-sh">GitHub</a>
</p>
