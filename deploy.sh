#!/usr/bin/env bash
set -euo pipefail

echo "========================================="
echo "   Ghost-RS Server Deployment Script     "
echo "========================================="

# 1. Check & Install Docker if needed
if ! command -v docker &> /dev/null; then
    echo "[+] Installing Docker..."
    curl -fsSL https://get.docker.com -o get-docker.sh
    sudo sh get-docker.sh
    sudo usermod -aG docker "$USER" || true
    rm -f get-docker.sh
fi

# 2. Check & Install Docker Compose plugin if needed
if ! docker compose version &> /dev/null; then
    echo "[+] Installing Docker Compose plugin..."
    sudo apt-get update && sudo apt-get install -y docker-compose-plugin
fi

# 3. Ensure directories exist
mkdir -p maps war3 replays

# 4. Build and Run Ghost-RS
echo "[+] Building and starting Ghost-RS in Docker..."
docker compose down || true
docker compose up -d --build

echo "========================================="
echo "   Ghost-RS is now running in background "
echo "   To view logs: docker compose logs -f  "
echo "========================================="
docker compose logs -f --tail=50