<p align="center">
  <img src="https://xpo.sh/favicon.svg" width="64" height="64" alt="xpo" />
</p>

<h1 align="center">xpo</h1>

<p align="center">
  Expose local services to the internet via secure tunnels.<br/>
  A modern, open-source alternative to ngrok — written in Rust.
</p>

<p align="center">
  <a href="https://xpo.sh"><img src="https://img.shields.io/badge/website-xpo.sh-22d3ee?style=flat-square" alt="Website" /></a>
  <a href="https://crates.io/crates/xpo"><img src="https://img.shields.io/crates/v/xpo?style=flat-square&color=a78bfa" alt="Crates.io" /></a>
  <a href="https://github.com/xpo-sh/xpo/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-4ade80?style=flat-square" alt="License" /></a>
  <a href="https://github.com/xpo-sh/xpo"><img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-facc15?style=flat-square" alt="Platform" /></a>
</p>

---

```bash
$ xpo share 3000
  ⚡ Tunnel established
  → https://a1b2c3.xpo.sh → localhost:3000

$ xpo share 3000 -s myapp
  ⚡ Tunnel established
  → https://myapp.xpo.sh → localhost:3000

$ xpo share 8080 -d example.com
  ⚡ Tunnel established
  → https://example.com → localhost:8080
```

## Install

```bash
# One-liner (macOS / Linux)
curl -fsSL https://xpo.sh/install | sh

# Cargo
cargo install xpo

# Homebrew
brew tap xpo-sh/tap && brew install xpo

# npm
npm install -g @xposh/cli
```

## Quick Start

```bash
# 1. Login (one-time)
xpo login

# 2. Expose your local server
xpo share 3000
```

That's it. Your local server is now accessible at `https://<random>.xpo.sh`.

## Usage

### Share a local port

```bash
xpo share 3000
# → https://a1b2c3.xpo.sh → localhost:3000
```

### Custom subdomain

```bash
xpo share 3000 -s myapp
# → https://myapp.xpo.sh → localhost:3000
```

### Custom domain (Pro)

```bash
xpo share 8080 -d example.com
# → https://example.com → localhost:8080
```

### Check status

```bash
xpo status
# ⚡ Active tunnels:
#   myapp.xpo.sh → localhost:3000 (2h 14m)
```

## How It Works

1. **Client connects** — `xpo share 3000` opens a WebSocket control channel to the edge server
2. **Subdomain assigned** — Server assigns `abc123.xpo.sh` (or your custom subdomain)
3. **Request arrives** — Browser hits `https://abc123.xpo.sh`
4. **Forwarded** — Server sends the request through the tunnel to your client
5. **Proxied** — Client forwards to `localhost:3000` and returns the response
6. **TLS everywhere** — All traffic encrypted with auto-provisioned Let's Encrypt certificates

```
┌──────────────┐          ┌───────────────────┐
│   xpo CLI    │ ◄──WS──► │   xpo.sh Server   │
│   (client)   │          │   (edge)          │
│              │          │                   │
│  localhost:N │          │  TLS Terminate    │
└──────────────┘          │  Wildcard Route   │
                          │  Auth / Logging   │
                          └───────────────────┘
                                   ▲
                                   │ HTTPS
                          ┌────────┴────────┐
                          │  Browser/Webhook │
                          │  *.xpo.sh        │
                          └─────────────────┘
```

## Features

| Feature | Free | Pro ($5/mo) |
|---|---|---|
| HTTP/HTTPS tunnels | ✅ | ✅ |
| Random subdomain | ✅ | ✅ |
| Custom subdomain | — | ✅ |
| Custom domain | — | ✅ |
| Session limit | 1 hour | Unlimited |
| TCP/UDP tunnels | — | ✅ |
| Webhook replay | — | ✅ |
| Request inspector | — | ✅ |
| Global edge servers | — | ✅ |

## Why xpo?

| | xpo | ngrok | expose.dev |
|---|---|---|---|
| Language | Rust | Go (closed) | PHP |
| Runtime needed | None | None | PHP |
| Open source | ✅ MIT | ❌ | Partial |
| Pro price | **$49/yr** | $99/yr | $79/yr |
| TCP tunnels | ✅ | ✅ | ❌ |
| UDP tunnels | ✅ | ❌ | ❌ |
| HMR detection | ✅ | ❌ | ✅ |

## Configuration

```yaml
# ~/.xpo/config.yaml
token: xpo_tk_abc123def456
default_server: eu.xpo.sh
```

## Project Structure

```
crates/
├── xpo-core/       # Shared protocol, types, auth
├── xpo-client/     # CLI binary
├── xpo-server/     # Edge server binary
└── xpo-dashboard/  # Local request inspector (:4040)
```

## Development

```bash
# Build all crates
cargo build

# Run the client
cargo run -p xpo-client -- share 3000

# Run the server
cargo run -p xpo-server

# Run tests
cargo test --workspace
```

## Contributing

Contributions are welcome! Please open an issue first to discuss what you'd like to change.

## License

[MIT](LICENSE) — Chingiz Huseynzade

<p align="center">
  <a href="https://xpo.sh">xpo.sh</a> · <a href="https://x.com/getxposh">X/Twitter</a> · <a href="https://github.com/xpo-sh">GitHub</a>
</p>
