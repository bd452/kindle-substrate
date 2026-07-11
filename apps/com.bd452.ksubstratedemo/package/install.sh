#!/bin/sh

set -e

PKG="/mnt/us/kmc/kpm/packages/com.bd452.ksubstratedemo"

cat > /mnt/us/documents/com.bd452.ksubstratedemo.sh << EOF
#!/bin/sh
exec "$PKG/app.sh"
EOF
chmod +x /mnt/us/documents/com.bd452.ksubstratedemo.sh

echo "Kindle Substrate Demo installed. Open com.bd452.ksubstratedemo.sh from Documents to run it."
