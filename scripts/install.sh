#!/bin/sh
set -e

cyan="\033[36m"
violet="\033[35m"
green="\033[32m"
red="\033[31m"
bold="\033[1m"
dim="\033[2m"
reset="\033[0m"

REPO="xpo-sh/xpo"
INSTALL_DIR="/usr/local/bin"

printf "\n"
dim2="\033[38;2;85;85;112m"
printf "  ${dim2}                                      dP${reset}\n"
printf "  ${dim2}                                      88${reset}\n"
printf "  ${cyan}dP.  .dP 88d888b. .d8888b.    ${dim2}.d8888b. 88d888b.${reset}\n"
printf "  ${cyan} \`8bd8'  88'  \`88 88'  \`88    ${dim2}Y8ooooo. 88'  \`88${reset}\n"
printf "  ${cyan} .d88b.  88.  .88 88.  .88 ${dim2}dP       88 88    88${reset}\n"
printf "  ${cyan}dP'  \`dP 88Y888P' \`88888P' ${dim2}88 \`88888P' dP    dP${reset}\n"
printf "  ${cyan}         88${reset}\n"
printf "  ${cyan}         dP${reset}\n"
printf "\n"

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) printf "  ${red}✗${reset} Unsupported OS: ${OS}\n"; exit 1 ;;
esac

case "${ARCH}" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) printf "  ${red}✗${reset} Unsupported architecture: ${ARCH}\n"; exit 1 ;;
esac

TARGET="${arch}-${os}"

LATEST=$(curl -fsL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": "//;s/".*//')

if [ -z "${LATEST}" ]; then
  LATEST=$(curl -fsL "https://api.github.com/repos/${REPO}/releases" 2>/dev/null | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": "//;s/".*//')
fi

if [ -z "${LATEST}" ]; then
  printf "  ${red}✗${reset} Could not find a release\n"
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${LATEST}/xpo-${TARGET}.tar.gz"

printf "  ${dim}→${reset} Version:  ${bold}${LATEST}${reset}\n"
printf "  ${dim}→${reset} Target:   ${TARGET}\n"
printf "  ${dim}→${reset} Download: ${dim}${URL}${reset}\n"
printf "\n"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

printf "  ${dim}○${reset} Downloading..."
curl -fsSL "${URL}" -o "${TMPDIR}/xpo.tar.gz"
printf "\r\033[2K  ${green}✓${reset} Downloaded\n"

printf "  ${dim}○${reset} Extracting..."
tar xzf "${TMPDIR}/xpo.tar.gz" -C "${TMPDIR}"
printf "\r\033[2K  ${green}✓${reset} Extracted\n"

printf "  ${dim}○${reset} Installing..."
if [ -w "${INSTALL_DIR}" ]; then
  mv "${TMPDIR}/xpo" "${INSTALL_DIR}/xpo"
else
  sudo mv "${TMPDIR}/xpo" "${INSTALL_DIR}/xpo"
fi
chmod +x "${INSTALL_DIR}/xpo"
printf "\r\033[2K  ${green}✓${reset} Installed to ${INSTALL_DIR}/xpo\n"

printf "\n"
printf "  ${green}✓${reset} ${bold}xpo ${LATEST} installed!${reset}\n"
printf "\n"
printf "  ${dim}Get started:${reset}\n"
printf "    xpo login              ${dim}# authenticate with GitHub/Google${reset}\n"
printf "    xpo share 3000         ${dim}# https://abc123.xpo.sh -> localhost:3000${reset}\n"
printf "    xpo dev 3000 -n myapp  ${dim}# https://myapp.test -> localhost:3000${reset}\n"
printf "\n"
