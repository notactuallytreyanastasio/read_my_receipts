#!/bin/bash
# Setup Raspberry Pi 5 as a WiFi Access Point for receipt printing
# Run as root: sudo bash setup_ap.sh

set -e

echo "=== Receipt Printer WiFi AP Setup ==="

if [ "$EUID" -ne 0 ]; then
    echo "Run as root: sudo bash setup_ap.sh"
    exit 1
fi

echo "[1/6] Installing hostapd and dnsmasq..."
apt-get update -qq
apt-get install -y hostapd dnsmasq

echo "[2/6] Stopping services for configuration..."
systemctl stop hostapd 2>/dev/null || true
systemctl stop dnsmasq 2>/dev/null || true

echo "[3/6] Configuring static IP..."
cat >> /etc/dhcpcd.conf << 'EOF'

# Receipt Printer WiFi AP - static IP
interface wlan0
    static ip_address=192.168.4.1/24
    nohook wpa_supplicant
EOF

echo "[4/6] Installing hostapd config..."
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cp "$SCRIPT_DIR/hostapd.conf" /etc/hostapd/hostapd.conf
sed -i 's|^#DAEMON_CONF=.*|DAEMON_CONF="/etc/hostapd/hostapd.conf"|' /etc/default/hostapd 2>/dev/null || true

echo "[5/6] Installing dnsmasq config..."
cp "$SCRIPT_DIR/dnsmasq.conf" /etc/dnsmasq.d/receipt-printer.conf

echo "[6/6] Enabling services..."
systemctl unmask hostapd
systemctl enable hostapd
systemctl enable dnsmasq

echo ""
echo "=== Setup complete ==="
echo "Reboot to activate the WiFi AP."
echo "SSID: ReceiptPrinter"
echo "Password: printme123"
echo "Pi IP: 192.168.4.1"
echo ""
echo "After reboot, the upload page will be at:"
echo "  http://192.168.4.1/"
echo "(or auto-opens via captive portal when iPhone joins)"
