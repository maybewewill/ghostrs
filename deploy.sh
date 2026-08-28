set -euo pipefail

echo "========================================="
echo "   Spectre Server Deployment Script      "
echo "========================================="

if ! command -v docker &> /dev/null; then
    echo "[+] Installing Docker..."
    curl -fsSL https://get.docker.com -o get-docker.sh
    sudo sh get-docker.sh
    sudo usermod -aG docker "$USER" || true
    rm -f get-docker.sh
fi

if ! docker compose version &> /dev/null; then
    echo "[+] Installing Docker Compose plugin..."
    sudo apt-get update && sudo apt-get install -y docker-compose-plugin
fi

mkdir -p maps war3 replays data
if [ -d "spectre.db" ]; then
    rm -rf spectre.db
fi

echo "[+] Pulling and starting Spectre in Docker..."
docker compose down || true
docker compose pull
docker compose up -d

echo "========================================="
echo "   Spectre is now running in background  "
echo "   To view logs: docker compose logs -f  "
echo "========================================="
docker compose logs -f --tail=50