#!/usr/bin/env bash
# ==============================================================================
# PixelNode Extended Patterns (Slots 24-27) Direct Hardware Verification Tool
# ==============================================================================
# Directly connects to Arduino Uno via USB-Serial, streams real-time animation
# frames with zero dynamic memory overhead, and verifies MCU bidirectional
# telemetry reception in real time.
# ==============================================================================

set -e

DEVICE="${1:-/dev/ttyUSB0}"
PIXELS="${2:-60}"
COLOR_ORDER="${3:-grb}"
BAUD="${4:-115200}"

echo "=================================================================="
echo "PixelNode Extended Patterns Hardware Verification (Slots 24-27)"
echo "Target Device: $DEVICE ($PIXELS px, order: $COLOR_ORDER, baud: $BAUD)"
echo "=================================================================="

if [ ! -e "$DEVICE" ]; then
    echo "ERROR: Serial device '$DEVICE' not found. Please connect Arduino Uno."
    exit 1
fi

echo ""
echo "------------------------------------------------------------------"
echo "[1/4] Slot 24: Center Ripple Pulse (多重センターリップル)"
echo "      - 中央から外側へ波が次々と湧き出し、多重伝搬"
echo "      - Color: Cyan (R=0, G=220, B=255), 5 seconds"
echo "------------------------------------------------------------------"
./target/debug/pxtool test \
    --serial "$DEVICE" \
    --baud "$BAUD" \
    --pixels "$PIXELS" \
    --color-order "$COLOR_ORDER" \
    --pattern 24 \
    --speed 128 \
    --size 128 \
    --color "0,220,255" \
    --duration 5

echo ""
echo "------------------------------------------------------------------"
echo "[2/4] Slot 25: Dual-Tone Color Wave (2色ツートーン波)"
echo "      - 黒（消灯）に落とさず、2色が滑らかに交替して流れる"
echo "      - Color 1: Magenta (255, 0, 180) <-> Color 2: Cyan/Green rotation"
echo "      - Duration: 5 seconds"
echo "------------------------------------------------------------------"
./target/debug/pxtool test \
    --serial "$DEVICE" \
    --baud "$BAUD" \
    --pixels "$PIXELS" \
    --color-order "$COLOR_ORDER" \
    --pattern 25 \
    --speed 110 \
    --size 128 \
    --color "255,0,180" \
    --duration 5

echo ""
echo "------------------------------------------------------------------"
echo "[3/4] Slot 26: Directional Wave Pulse (一方向パルス流)"
echo "      - 左から右へパルスが連続射出され、尾を引いて流れる"
echo "      - Color: Amber Gold (255, 160, 10)"
echo "      - Duration: 5 seconds"
echo "------------------------------------------------------------------"
./target/debug/pxtool test \
    --serial "$DEVICE" \
    --baud "$BAUD" \
    --pixels "$PIXELS" \
    --color-order "$COLOR_ORDER" \
    --pattern 26 \
    --speed 130 \
    --size 128 \
    --color "255,160,10" \
    --duration 5

echo ""
echo "------------------------------------------------------------------"
echo "[4/4] Slot 27: Center Inward Pull (外側集約インワードウェーブ)"
echo "      - 両端から中央へ吸い込まれるように波が押し寄せて中心で消滅"
echo "      - Color: Royal Purple (180, 20, 255)"
echo "      - Duration: 5 seconds"
echo "------------------------------------------------------------------"
./target/debug/pxtool test \
    --serial "$DEVICE" \
    --baud "$BAUD" \
    --pixels "$PIXELS" \
    --color-order "$COLOR_ORDER" \
    --pattern 27 \
    --speed 128 \
    --size 128 \
    --color "180,20,255" \
    --duration 5

echo ""
echo "=================================================================="
echo "All 4 extended patterns verified successfully on physical hardware!"
echo "=================================================================="
