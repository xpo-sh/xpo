<p align="center">
  <a href="https://xpo.sh"><img src="assets/banner.png" alt="xpo.sh — Expose local services via secure tunnels" /></a>
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

## License

[MIT](LICENSE)

<p align="center">
  <a href="https://xpo.sh">xpo.sh</a> · <a href="https://x.com/getxposh">X/Twitter</a> · <a href="https://github.com/xpo-sh">GitHub</a>
</p>
