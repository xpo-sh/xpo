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
mkdir -p /etc/xpo/certs
mkdir -p /etc/xpo/acme
mkdir -p /var/log/xpo

chown -R xpo:xpo /etc/xpo /var/log/xpo

# 4. Firewall
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp    # SSH
ufw allow 80/tcp    # HTTP (redirect to HTTPS)
ufw allow 443/tcp   # HTTPS (Caddy)
ufw allow 8081/tcp  # WebSocket ingress (Caddy)
ufw --force enable
echo "Firewall configured"

# 5. Create env file template
if [ ! -f /etc/xpo/server.env ]; then
    cat > /etc/xpo/server.env << 'EOF'
BASE_DOMAIN=REPLACE_ME.example.com
REGION=eu1
JWT_SECRET=REPLACE_ME
ACME_ENABLED=false
ACME_STAGING=false
# Preferred future path:
# JWT_PUBLIC_KEY_PATH=/etc/xpo/jwt-public.pem
EOF
    chmod 600 /etc/xpo/server.env
    chown xpo:xpo /etc/xpo/server.env
    echo "Created /etc/xpo/server.env - update all placeholders before starting the service"
fi

# 6. Install systemd service
cp deploy/xpo-server.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable xpo-server
echo "Systemd service installed"

echo ""
echo "=== Next steps ==="
echo "1. Edit /etc/xpo/server.env and replace every REPLACE_ME value"
echo "2. Copy xpo-server binary to /usr/local/bin/"
echo "3. Put Caddy in front of xpo-server (:443 -> :8080, :8081 -> :8082)"
echo "4. systemctl start xpo-server"
echo "5. Configure Cloudflare DNS: *.your-domain -> $(curl -s ifconfig.me)"
