#!/bin/bash
set -euo pipefail

echo "=========================================="
echo "  Configuring PhotonCAM OS on Raspberry Pi"
echo "=========================================="

# 1. Install prerequisites (avahi-daemon for .local mDNS discovery, camera utilities)
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends avahi-daemon udev libudev1 v4l-utils fswebcam

# 2. Set Hostname to 'photoncam'
echo "photoncam" > /etc/hostname
sed -i 's/raspberrypi/photoncam/g' /etc/hosts || true

# 3. Create default user 'pi' (password: raspberry) with full sudo access
useradd -m -s /bin/bash -G sudo,dialout pi 2>/dev/null || true
echo "pi:raspberry" | chpasswd
echo "pi ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/010_pi-nopasswd
chmod 0440 /etc/sudoers.d/010_pi-nopasswd

# Also set userconf.txt in boot partition for Raspberry Pi OS first-boot compatibility
PASS_HASH='$6$lO003f6.ZlQ6xZHz$xvHTDBjtC1eQqkt30JW2N0NMNPHTnkGyXmuuSDU2z9Viq83FLCoFcoabxBYTft.X/V5mz7mhiKCHQULRnFLb71'
mkdir -p /boot/firmware
echo "pi:$PASS_HASH" > /boot/firmware/userconf.txt 2>/dev/null || true
echo "pi:$PASS_HASH" > /boot/userconf.txt 2>/dev/null || true

# 4. Enable SSH by default so user can access terminal over network
mkdir -p /boot/firmware
touch /boot/firmware/ssh 2>/dev/null || true
touch /boot/ssh 2>/dev/null || true

# 5. Locate and install photoncam-bridge binary
SRC_BIN=""
for candidate in \
    "/mounted-github-repo/.github/scripts/photoncam-bridge" \
    "/mounted-github-repo/target/aarch64-unknown-linux-gnu/release/photoncam-bridge" \
    "/mounted-github-repo/photoncam-bridge" \
    "/mounted-github-repo/dist/photoncam-bridge-linux-arm64" \
    "$(dirname "$0")/photoncam-bridge" \
    "/photoncam-bridge"; do
    if [ -f "$candidate" ]; then
        SRC_BIN="$candidate"
        break
    fi
done

if [ -z "$SRC_BIN" ]; then
    echo "ERROR: photoncam-bridge binary not found! Contents of /mounted-github-repo:"
    ls -la /mounted-github-repo || true
    ls -la /mounted-github-repo/.github/scripts || true
    exit 1
fi

echo "==> Installing photoncam-bridge from $SRC_BIN..."
cp "$SRC_BIN" /usr/local/bin/photoncam-bridge
chmod 755 /usr/local/bin/photoncam-bridge
ln -sf /usr/local/bin/photoncam-bridge /usr/local/bin/laserburn-bridge

# 5. Create systemd service
cat << 'EOF' > /etc/systemd/system/photoncam-bridge.service
[Unit]
Description=PhotonCAM Bridge & Web Server for Laser CAM & CNC
After=network.target network-online.target avahi-daemon.service
Wants=network-online.target

[Service]
Type=simple
User=root
ExecStart=/usr/local/bin/photoncam-bridge --port 8080 --tcp-bridge 2323
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

# 6. Enable services
mkdir -p /etc/systemd/system/multi-user.target.wants
ln -sf /etc/systemd/system/photoncam-bridge.service /etc/systemd/system/multi-user.target.wants/photoncam-bridge.service
ln -sf /lib/systemd/system/avahi-daemon.service /etc/systemd/system/multi-user.target.wants/avahi-daemon.service || true
ln -sf /lib/systemd/system/ssh.service /etc/systemd/system/multi-user.target.wants/ssh.service || true
systemctl enable avahi-daemon 2>/dev/null || true
systemctl enable ssh 2>/dev/null || true
systemctl enable photoncam-bridge.service 2>/dev/null || true

# 7. Verification
if [ ! -x /usr/local/bin/photoncam-bridge ]; then
    echo "ERROR: /usr/local/bin/photoncam-bridge was not installed properly!"
    exit 1
fi

echo "PhotonCAM OS customization complete! Binary installed to /usr/local/bin/photoncam-bridge"
