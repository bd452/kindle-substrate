#!/bin/sh

set -e

PKG="/mnt/us/kmc/kpm/packages/com.bd452.ksubstrate"
mkdir -p "$PKG/tweaks"

cat > /mnt/us/documents/com.bd452.ksubstrate-enable.sh << EOF
#!/bin/sh
exec "$PKG/app.sh" enable
EOF
chmod +x /mnt/us/documents/com.bd452.ksubstrate-enable.sh

cat > /mnt/us/documents/com.bd452.ksubstrate-disable.sh << EOF
#!/bin/sh
exec "$PKG/app.sh" disable
EOF
chmod +x /mnt/us/documents/com.bd452.ksubstrate-disable.sh

echo "Kindle Substrate installed."
echo "Open com.bd452.ksubstrate-enable.sh from Documents to enable tweaks."
echo "Open com.bd452.ksubstrate-disable.sh from Documents to return to stock UI."
