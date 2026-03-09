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

**xpo** is an open-source tunneling tool that exposes local services to the internet via secure HTTPS tunnels. Built in Rust for maximum performance.

## Install

```bash
curl -fsSL https://xpo.sh/install | sh
```

## Public tunnels

Expose any local port to the internet with a single command:

```bash
$ xpo login
  ✓ Logged in as you@email.com

$ xpo share 3000
  ╭───────────────────────────────────────────╮
  │                                           │
  │  xpo share                                │
  │                                           │
  │  https://a1b2c3.xpo.sh -> localhost:3000  │
  │                                           │
  │  you@email.com - Ctrl+C to stop           │
  │                                           │
  ╰───────────────────────────────────────────╯

  GET  /           200   12ms
  GET  /_nuxt/     101   42ms
  GET  /api/data   200    8ms

$ xpo share 3000 -s myapp
  https://myapp.xpo.sh -> localhost:3000
```

## Local HTTPS

Real HTTPS on localhost with `.test` domains. No browser warnings, WebSocket/HMR works out of the box:

```bash
$ xpo dev setup
  ✓ Root CA created (P-256 ECDSA, 10yr)
  ✓ CA trusted in system keychain
  ✓ Port forwarding active  443->10443, 80->10080

$ xpo dev 3000 -n myapp
  -> https://myapp.test  ->  localhost:3000

  GET / 200 12ms
  GET /_nuxt/ 101 42ms
```

## Features

- **HTTPS tunnels** - Let's Encrypt wildcard TLS, zero config
- **WebSocket relay** - HMR/hot-reload works through tunnel
- **Local HTTPS** - trusted `.test` domains for development
- **Auto-reconnect** - exponential backoff on connection loss
- **Request logging** - colored terminal output with timing
- **Custom subdomains** - `xpo share 3000 -s myapp`
- **GitHub/Google auth** - OAuth login, no email/password
- **Fast** - Rust + tokio, sub-millisecond proxy overhead
- **Open source** - MIT licensed

## Commands

```bash
xpo login                   # authenticate with GitHub or Google
xpo share <port>            # public HTTPS tunnel
xpo share <port> -s <name>  # custom subdomain
xpo dev setup               # one-time local HTTPS setup
xpo dev <port> -n <name>    # local HTTPS proxy
xpo dev stop                # clean up
xpo status                  # show session info
xpo logout                  # clear session
```

## Platform support

| Platform | `xpo dev` | `xpo share` |
|---|---|---|
| macOS (ARM + Intel) | Full | Full |
| Linux (x86_64 + ARM) | Full | Full |
| Windows | Not supported | Full |

## Coming soon

- Local dashboard with request inspector
- Webhook replay
- QR code for mobile testing
- Connection pooling
- Project config (xpo.yaml)

## License

[MIT](LICENSE)

<p align="center">
  <a href="https://xpo.sh">xpo.sh</a> · <a href="https://x.com/getxposh">X/Twitter</a> · <a href="https://github.com/xpo-sh">GitHub</a>
</p>
