#!/usr/bin/env bash
set -euo pipefail

# xpo.sh server setup script for Ubuntu 24
# Run as root on a fresh Contabo/Hetzner VPS

echo "=== xpo.sh server setup ==="

# 1. System updates
apt-get update && apt-get upgrade -y
apt-get install -y ca-certificates curl ufw

# 2. Create xpo user
if ! id -u xpo &>/dev/null; then
    useradd --system --create-home --shell /usr/sbin/nologin xpo
    echo "Created user: xpo"
fi

# 3. Create directories
mkdir -p /etc/xpo
mkdir -p /var/log/xpo

chown xpo:xpo /var/log/xpo

# 4. Firewall
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp    # SSH
ufw allow 80/tcp    # HTTP (redirect to HTTPS)
ufw allow 443/tcp   # HTTPS (tunnel traffic)
ufw allow 8081/tcp  # WebSocket (tunnel control channel)
ufw --force enable
echo "Firewall configured"

# 5. Create env file template
if [ ! -f /etc/xpo/server.env ]; then
    cat > /etc/xpo/server.env << 'EOF'
JWT_SECRET=CHANGE_ME_TO_REAL_SECRET
EOF
    chmod 600 /etc/xpo/server.env
    echo "Created /etc/xpo/server.env - UPDATE JWT_SECRET!"
fi

# 6. Install systemd service
cp deploy/xpo-server.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable xpo-server
echo "Systemd service installed"

echo ""
echo "=== Next steps ==="
echo "1. Edit /etc/xpo/server.env (set JWT_SECRET)"
echo "2. Copy xpo-server binary to /usr/local/bin/"
echo "3. systemctl start xpo-server"
echo "4. Configure Cloudflare DNS: *.xpo.sh -> $(curl -s ifconfig.me)"
