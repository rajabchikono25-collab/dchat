#!/bin/bash
# MinIO Quick Start Script for dchat Testnet (Linux/macOS)

set -e

echo "=== dchat MinIO Quick Start ==="
echo ""

# Check for container runtime
if command -v docker &> /dev/null; then
    RUNTIME="docker"
    echo "✓ Found Docker"
elif command -v podman &> /dev/null; then
    RUNTIME="podman"
    echo "✓ Found Podman"
else
    echo "✗ Neither Docker nor Podman found. Please install one."
    exit 1
fi

echo ""
echo "Starting MinIO container..."

# Stop and remove existing container
$RUNTIME stop dchat-minio 2>/dev/null || true
$RUNTIME rm dchat-minio 2>/dev/null || true

# Start MinIO
$RUNTIME run -d \
  --name dchat-minio \
  -p 9000:9000 \
  -p 9001:9001 \
  -e "MINIO_ROOT_USER=dchat_admin" \
  -e "MINIO_ROOT_PASSWORD=dchat_secure_password_change_me" \
  quay.io/minio/aistor/minio:latest server /data --console-address ":9001"

echo "✓ MinIO container started"
echo ""

# Wait for MinIO to be ready
echo "Waiting for MinIO to be ready..."
for i in {1..30}; do
    if curl -sf http://localhost:9000/minio/health/live > /dev/null 2>&1; then
        echo "✓ MinIO is ready!"
        break
    fi
    echo -n "."
    sleep 1
done

echo ""
echo ""

# Create buckets
echo "Creating buckets..."

BUCKETS=(
    "dchat-testnet"
    "dchat-media"
    "dchat-attachments"
    "dchat-backups"
    "dchat-avatars"
    "dchat-channels"
)

for bucket in "${BUCKETS[@]}"; do
    echo "  Creating bucket: $bucket"
    $RUNTIME run --rm \
        --network="host" \
        quay.io/minio/aistor/mc:latest \
        /bin/sh -c "mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me && mc mb dchat/$bucket --ignore-existing" 2>/dev/null || true
done

echo "✓ Buckets created"
echo ""

# Set public access
echo "Setting public read access for media buckets..."
$RUNTIME run --rm \
    --network="host" \
    quay.io/minio/aistor/mc:latest \
    /bin/sh -c "mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me && mc anonymous set download dchat/dchat-media && mc anonymous set download dchat/dchat-avatars" 2>/dev/null || true

echo "✓ Public access configured"
echo ""

# Display summary
echo "=== MinIO is ready! ==="
echo ""
echo "MinIO Console: http://localhost:9001"
echo "  Username: dchat_admin"
echo "  Password: dchat_secure_password_change_me"
echo ""
echo "S3 API Endpoint: http://localhost:9000"
echo ""
echo "Buckets created:"
for bucket in "${BUCKETS[@]}"; do
    echo "  - $bucket"
done
echo ""
echo "Test upload:"
echo "  mc alias set dchat http://localhost:9000 dchat_admin dchat_secure_password_change_me"
echo "  mc cp test.txt dchat/dchat-testnet/"
echo ""
echo "View logs:"
echo "  $RUNTIME logs -f dchat-minio"
echo ""
echo "Stop MinIO:"
echo "  $RUNTIME stop dchat-minio"
echo ""
