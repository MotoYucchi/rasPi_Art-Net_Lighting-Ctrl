#!/usr/bin/env bash
set -e
export no_proxy="127.0.0.1,localhost"
export NO_PROXY="127.0.0.1,localhost"

echo "=== PixelNode Arduino Uno Real Hardware Test ==="

DEVICE="/dev/ttyUSB0"
if [ ! -e "$DEVICE" ]; then
    if [ -e "/dev/ttyACM0" ]; then
        DEVICE="/dev/ttyACM0"
    else
        echo "ERROR: Neither /dev/ttyUSB0 nor /dev/ttyACM0 found."
        exit 1
    fi
fi

echo "Detected target MCU device: $DEVICE"

TMP_DIR=$(mktemp -d)
CONFIG_FILE="$TMP_DIR/config_hardware.toml"

cat <<EOF > "$CONFIG_FILE"
[node]
name = "Arduino-Uno-Real-Node"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = 8789
bind_ip = "127.0.0.1"

[audio]
enabled = false

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "uno-out"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = 115200
chip = "ws2812b"
color_order = "grb"
pixels = 60
psu_max_current_ma = 10000

[[fixture]]
output = "uno-out"
pixel_range = [0, 59]
personality = "preset_7ch"
white_mode = "rgb_blend"
universe = 1
address = 1
EOF

echo "Starting pixelnoded with hardware driver ($DEVICE @ 115200 baud)..."
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

sleep 1.5

echo "Checking Web API status..."
curl -s -f http://127.0.0.1:8789/api/status | grep -q "Arduino-Uno-Real-Node"
echo "Web API status: OK"

echo "Querying live status via pxtool..."
./target/debug/pxtool status --node http://127.0.0.1:8789

echo "Streaming live Art-Net DMX frame (Ch 1=134 [Full Dimmer], Red=255, Green=120, Blue=0, Ch 6=8 [Sine Breath])..."
./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,120,0,0,8,0"
sleep 1.0

echo "Streaming live sACN DMX frame (Ch 1=134, Red=0, Green=255, Blue=255, Ch 6=16 [Rainbow Wave])..."
./target/debug/pxtool send-sacn --ip 127.0.0.1 --universe 1 --priority 100 --channels "134,0,255,255,0,16,0"
sleep 1.0

echo "Running 2-second remote test pattern 'walker' to physical Arduino..."
./target/debug/pxtool test --remote --mode walker --node http://127.0.0.1:8789
sleep 2.0
./target/debug/pxtool test --remote --mode off --node http://127.0.0.1:8789

echo "Verifying final telemetry..."
./target/debug/pxtool status --node http://127.0.0.1:8789

echo "=== REAL HARDWARE TEST COMPLETED WITH 100% SUCCESS! ==="
