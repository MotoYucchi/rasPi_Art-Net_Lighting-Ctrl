#!/usr/bin/env bash
set -e

echo "=== PixelNode System Integration Test ==="

# 1. Start pixelnoded in background with a temporary config
TMP_DIR=$(mktemp -d)
CONFIG_FILE="$TMP_DIR/config.toml"

cat <<EOF > "$CONFIG_FILE"
[node]
name = "Integration-Test-Node"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = 8788
bind_ip = "127.0.0.1"

[audio]
enabled = true
listen_udp_port = 8765
timeout_ms = 500

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out1"
driver = "virtual"
device = "/dev/null"
chip = "ws2811"
color_order = "brg"
pixels = 50
psu_max_current_ma = 10000

[[fixture]]
output = "out1"
pixel_range = [0, 49]
personality = "preset_7ch"
white_mode = "rgb_blend"
universe = 1
address = 1
EOF

echo "Validating configuration file with pxtool..."
./target/debug/pxtool config validate "$CONFIG_FILE"

echo "Starting pixelnoded daemon..."
./target/debug/pixelnoded "$CONFIG_FILE" &
DAEMON_PID=$!

cleanup() {
    echo "Stopping pixelnoded (PID: $DAEMON_PID)..."
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
    rm -rf "$TMP_DIR"
    echo "Cleanup complete."
}
trap cleanup EXIT

# Allow daemon to initialize sockets and web server
sleep 1.2

echo "Testing Web UI Dashboard response..."
curl -s -f http://127.0.0.1:8788/ > /dev/null
echo "Web UI Dashboard: OK (200 OK)"

echo "Testing Web REST API /api/status endpoint..."
curl -s -f http://127.0.0.1:8788/api/status | grep -q "Integration-Test-Node"
echo "REST API /api/status: OK"

echo "Querying status via pxtool..."
./target/debug/pxtool status --node http://127.0.0.1:8788

echo "Testing show lock toggle via pxtool..."
./target/debug/pxtool lock --node http://127.0.0.1:8788 --enable
./target/debug/pxtool status --node http://127.0.0.1:8788 --json | grep -q '"show_lock":true'
./target/debug/pxtool lock --node http://127.0.0.1:8788 --disable

# 2. Transmit Art-Net DMX frame via pxtool
# Channels: Dimmer=134 (Full), Red=255, Green=100, Blue=50, White=0, Pattern=0 (Static), Audio=0
echo "Sending Art-Net DMX Frame..."
./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,100,50,0,0,0"

sleep 0.4

# 3. Transmit sACN DMX frame via pxtool
echo "Sending sACN DMX Frame..."
./target/debug/pxtool send-sacn --ip 127.0.0.1 --universe 1 --priority 100 --channels "134,0,255,128,0,0,0"

sleep 0.4

# 4. Trigger remote test pattern via pxtool
echo "Triggering remote test pattern..."
./target/debug/pxtool test --remote --mode walker --node http://127.0.0.1:8788
./target/debug/pxtool test --remote --mode off --node http://127.0.0.1:8788

# 5. Run rig-check standalone test
echo "Running Rig Check (RGBW Cycle 1s)..."
./target/debug/pxtool test --mode rgbw --pixels 10 --duration 1

echo "=== Integration Test Succeeded! ==="
