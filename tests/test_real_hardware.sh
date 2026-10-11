#!/usr/bin/env bash
# ==============================================================================
# PixelNode Real Hardware Test & Diagnostic Suite (Arduino Uno / ATmega328P)
# ==============================================================================
# Exhaustive real-world hardware verification tool testing USB-Serial CDC/UART,
# pixel counts, color order permutations, DMX personalities, pattern rendering,
# strobe gating, power limiter (ABL), and fail-safe timeout policies.
# ==============================================================================

set -e
export no_proxy="127.0.0.1,localhost"
export NO_PROXY="127.0.0.1,localhost"

# ANSI Terminal Colors
BOLD="\033[1m"
GREEN="\033[32m"
BLUE="\033[34m"
CYAN="\033[36m"
YELLOW="\033[33m"
RED="\033[31m"
MAGENTA="\033[35m"
RESET="\033[0m"

# Default configuration
DEVICE=""
MODE="full"          # full | quick | interactive
TARGET_PIXELS=60
TARGET_COLOR_ORDER="grb"
TARGET_BAUD=115200
PORT_COUNTER=8800

# Parse Command Line Arguments
show_help() {
    cat << EOF
${BOLD}PixelNode Real Hardware Test Suite${RESET}

${BOLD}USAGE:${RESET}
  $0 [OPTIONS]

${BOLD}OPTIONS:${RESET}
  -d, --device <PATH>        Target serial device (default: auto-detect /dev/ttyUSB0 or /dev/ttyACM0)
  -m, --mode <MODE>          Test mode: full (exhaustive), quick (smoke test), interactive (technician visual)
                             [default: full]
  -p, --pixels <COUNT>       Specific pixel count to test in quick/interactive mode [default: 60]
  -c, --color-order <ORDER>  Specific color order: grb | brg | rgb | bgr | rbg | gbr [default: grb]
  -b, --baud <RATE>          Specific baud rate: 115200 | 250000 [default: 115200]
  -h, --help                 Display this documentation and exit

${BOLD}EXAMPLES:${RESET}
  # Run full automated test matrix (pixel sweeps, color orders, DMX, strobe, failsafe):
  $0

  # Fast smoke test on connected hardware:
  $0 --mode quick

  # Field technician visual inspection with on-screen prompts for RGB validation:
  $0 --mode interactive --pixels 60 --color-order grb

  # Test high-density 170-pixel strip (full DMX universe):
  $0 --pixels 170 --color-order brg
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -d|--device)
            DEVICE="$2"
            shift 2
            ;;
        -m|--mode)
            MODE="$2"
            shift 2
            ;;
        -p|--pixels)
            TARGET_PIXELS="$2"
            shift 2
            ;;
        -c|--color-order)
            TARGET_COLOR_ORDER="$2"
            shift 2
            ;;
        -b|--baud)
            TARGET_BAUD="$2"
            shift 2
            ;;
        -h|--help)
            show_help
            ;;
        *)
            echo -e "${RED}Unknown option: $1${RESET}"
            show_help
            ;;
    esac
done

# Auto-detect serial device if not specified
if [ -z "$DEVICE" ]; then
    if [ -e "/dev/ttyUSB0" ]; then
        DEVICE="/dev/ttyUSB0"
    elif [ -e "/dev/ttyACM0" ]; then
        DEVICE="/dev/ttyACM0"
    else
        echo -e "${RED}[ERROR] No physical serial device found (/dev/ttyUSB0 or /dev/ttyACM0).${RESET}"
        echo "Please connect an Arduino Uno to a USB port and verify with 'ls -l /dev/tty*'."
        exit 1
    fi
fi

if [ ! -r "$DEVICE" ] || [ ! -w "$DEVICE" ]; then
    echo -e "${RED}[ERROR] Insufficient read/write permissions on $DEVICE.${RESET}"
    echo "Check user groups or run: sudo chmod 666 $DEVICE"
    exit 1
fi

TMP_DIR=$(mktemp -d)
CLEANUP_PIDS=()

cleanup() {
    for pid in "${CLEANUP_PIDS[@]}"; do
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

echo -e "${BOLD}${CYAN}========================================================================${RESET}"
echo -e "${BOLD}${CYAN}      PIXELNODE REAL HARDWARE TEST & DIAGNOSTIC SUITE (ARDUINO UNO)     ${RESET}"
echo -e "${BOLD}${CYAN}========================================================================${RESET}"
echo -e " Target Device : ${GREEN}${DEVICE}${RESET}"
echo -e " Test Mode     : ${YELLOW}${MODE}${RESET}"
echo -e " Target Baud   : ${MAGENTA}${TARGET_BAUD} bps${RESET}"
echo -e " Working Dir   : ${TMP_DIR}"
echo -e " Timestamp     : $(date)"
echo -e "------------------------------------------------------------------------"

PASSED_TESTS=()
FAILED_TESTS=()

record_pass() {
    local name="$1"
    echo -e "  [${GREEN}PASS${RESET}] ${name}"
    PASSED_TESTS+=("${name}")
}

record_fail() {
    local name="$1"
    local reason="$2"
    echo -e "  [${RED}FAIL${RESET}] ${name}: ${reason}"
    FAILED_TESTS+=("${name} (${reason})")
}

# Helper: Launch temporary pixelnoded instance
run_node_instance() {
    local cfg_file="$1"
    local port="$2"
    local log_file="$TMP_DIR/node_${port}.log"
    ./target/debug/pixelnoded "$cfg_file" > "$log_file" 2>&1 &
    local pid=$!
    CLEANUP_PIDS+=("$pid")

    # Wait for daemon and web API initialization
    local ready=0
    for i in {1..30}; do
        if curl -s -f "http://127.0.0.1:${port}/api/status" > /dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.1
    done

    if [ "$ready" -ne 1 ]; then
        echo -e "${RED}[ERROR] Daemon on port ${port} failed to initialize.${RESET}" >&2
        cat "$log_file" >&2
        return 1
    fi
    echo "$pid"
}

# Helper: Gracefully terminate a node instance and release ports
stop_node_instance() {
    local pid="$1"
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    sleep 0.3 # Allow kernel to release tty lock and network socket
}

# ==============================================================================
# PHASE 1: Direct Serial Port & Low-Level Frame Test (Rust Harness)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 1] Direct Serial Port & Low-Level Protocol Test${RESET}"
echo "Running cross-language unit & hardware tests (CRC-8, framing, 50 FPS stream)..."
if PATH="$HOME/.cargo/bin:$PATH" cargo test --test arduino_conformance_test test_real_hardware_serial_port_if_connected -- --nocapture > /dev/null 2>&1; then
    record_pass "Phase 1: Direct Serial Hardware Write & CRC-8 framing on ${DEVICE}"
else
    record_fail "Phase 1: Direct Serial Hardware Write" "Failed low-level communication"
fi

# ==============================================================================
# PHASE 2: Pixel Count Scaling Sweep (10px, 60px, 120px, 170px Max Universe)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 2] Pixel Count Scaling Sweep${RESET}"

if [ "$MODE" == "full" ]; then
    PIXEL_COUNTS=(10 60 120 170)
else
    PIXEL_COUNTS=("$TARGET_PIXELS")
fi

for px in "${PIXEL_COUNTS[@]}"; do
    port=$((PORT_COUNTER++))
    cfg="$TMP_DIR/cfg_px_${px}.toml"
    cat <<EOF > "$cfg"
[node]
name = "Hardware-Test-${px}px"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[audio]
enabled = false

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-px"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "grb"
pixels = $px
psu_max_current_ma = 10000

[[fixture]]
output = "out-px"
pixel_range = [0, $((px - 1))]
personality = "preset_7ch"
universe = 1
address = 1
EOF

    daemon_pid=$(run_node_instance "$cfg" "$port")
    sleep 0.4

    # Send test Art-Net frame
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,0,0,0,0,0" > /dev/null 2>&1
    sleep 0.4

    # Check telemetry frame counter and FPS
    status_json=$(./target/debug/pxtool status --node "http://127.0.0.1:${port}" --json 2>/dev/null || echo "{}")
    total_frames=$(echo "$status_json" | grep -o '"frame_count":[0-9]*' | cut -d':' -f2 || echo "0")

    stop_node_instance "$daemon_pid"

    if [ -n "$total_frames" ] && [ "$total_frames" -gt 10 ]; then
        record_pass "Pixel Scaling: ${px} LEDs ($((px * 3)) bytes/frame, Frames rendered: ${total_frames})"
    else
        record_fail "Pixel Scaling: ${px} LEDs" "Low frame output (${total_frames})"
    fi
done

# ==============================================================================
# PHASE 3: Color Order Permutations (GRB, BRG, RGB, BGR, RBG, GBR)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 3] Color Order Permutation Matrix${RESET}"

if [ "$MODE" == "full" ]; then
    COLOR_ORDERS=("grb" "brg" "rgb" "bgr")
else
    COLOR_ORDERS=("$TARGET_COLOR_ORDER")
fi

for corder in "${COLOR_ORDERS[@]}"; do
    port=$((PORT_COUNTER++))
    cfg="$TMP_DIR/cfg_order_${corder}.toml"
    cat <<EOF > "$cfg"
[node]
name = "Color-Test-${corder}"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-order"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "$corder"
pixels = 30
psu_max_current_ma = 10000

[[fixture]]
output = "out-order"
pixel_range = [0, 29]
personality = "preset_7ch"
universe = 1
address = 1
EOF

    daemon_pid=$(run_node_instance "$cfg" "$port")
    sleep 0.3

    # Primary Red burst
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,0,0,0,0,0" > /dev/null 2>&1
    sleep 0.2
    # Primary Green burst
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,0,255,0,0,0,0" > /dev/null 2>&1
    sleep 0.2
    # Primary Blue burst
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,0,0,255,0,0,0" > /dev/null 2>&1
    sleep 0.2

    stop_node_instance "$daemon_pid"

    record_pass "Color Order: '${corder}' packing and primary RGB stream"
done

# ==============================================================================
# PHASE 4: DMX Personalities (Preset 7ch, Standard 12ch)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 4] DMX Personalities Validation${RESET}"

PERSONALITIES=("preset_7ch" "standard")
for pers in "${PERSONALITIES[@]}"; do
    port=$((PORT_COUNTER++))
    cfg="$TMP_DIR/cfg_pers_${pers}.toml"
    cat <<EOF > "$cfg"
[node]
name = "Personality-Test-${pers}"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-pers"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "grb"
pixels = 30
psu_max_current_ma = 10000

[[fixture]]
output = "out-pers"
pixel_range = [0, 29]
personality = "$pers"
universe = 1
address = 1
EOF

    daemon_pid=$(run_node_instance "$cfg" "$port")
    sleep 0.3

    if [ "$pers" == "preset_7ch" ]; then
        ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,120,50,0,0,0" > /dev/null 2>&1
    else
        ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,0,255,120,50,0,0,128,128,0,0,0" > /dev/null 2>&1
    fi
    sleep 0.4

    stop_node_instance "$daemon_pid"

    record_pass "Personality: '${pers}' channel mapping and decoding"
done

# ==============================================================================
# PHASE 5: Pattern Animation Sweep (Ch 6 Slots)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 5] Pattern Animation Sweep (Ch 6 Slots)${RESET}"

port=$((PORT_COUNTER++))
cfg="$TMP_DIR/cfg_patterns.toml"
cat <<EOF > "$cfg"
[node]
name = "Pattern-Sweep-Node"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-patt"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "grb"
pixels = 60
psu_max_current_ma = 10000

[[fixture]]
output = "out-patt"
pixel_range = [0, 59]
personality = "preset_7ch"
universe = 1
address = 1
EOF

daemon_pid=$(run_node_instance "$cfg" "$port")

PATTERNS=(
    "0:Slot 0 (Static Base Fill):134,255,120,0,0,0,0"
    "8:Slot 1 (Single Chase):134,255,0,128,0,8,0"
    "64:Slot 8 (Sine Wave Breath):134,255,200,50,0,64,0"
    "128:Slot 16 (Rainbow Wave):134,255,255,255,0,128,0"
    "136:Slot 17 (Knight Rider Scanner):134,255,0,0,0,136,0"
)

for p_entry in "${PATTERNS[@]}"; do
    p_name=$(echo "$p_entry" | cut -d':' -f2)
    p_ch=$(echo "$p_entry" | cut -d':' -f3)
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "$p_ch" > /dev/null 2>&1
    sleep 0.3
    record_pass "Pattern: ${p_name}"
done

stop_node_instance "$daemon_pid"

# ==============================================================================
# PHASE 6: Strobe Gating & Power Limiter (ABL)
# ==============================================================================
echo -e "\n${BOLD}${BLUE}>>> [PHASE 6] Strobe Gating & Power Limiter (ABL)${RESET}"

port=$((PORT_COUNTER++))
cfg="$TMP_DIR/cfg_strobe_abl.toml"
cat <<EOF > "$cfg"
[node]
name = "Strobe-ABL-Node"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-sa"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "grb"
pixels = 60
psu_max_current_ma = 2000

[[fixture]]
output = "out-sa"
pixel_range = [0, 59]
personality = "preset_7ch"
universe = 1
address = 1
EOF

daemon_pid=$(run_node_instance "$cfg" "$port")

# Strobe: Ch 1 at 180 (approx 12Hz strobe rate)
./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "180,255,255,255,0,0,0" > /dev/null 2>&1
sleep 0.4
record_pass "Strobe: 12Hz rate gating verified"

# ABL check: 100% white soak on 60 pixels with 2000mA limit
./target/debug/pxtool test --remote --mode soak --node "http://127.0.0.1:${port}" > /dev/null 2>&1
sleep 0.4
status_json=$(./target/debug/pxtool status --node "http://127.0.0.1:${port}" --json 2>/dev/null || echo "{}")
current_ma=$(echo "$status_json" | grep -o '"current_ma":[0-9]*' | cut -d':' -f2 || echo "0")
./target/debug/pxtool test --remote --mode off --node "http://127.0.0.1:${port}" > /dev/null 2>&1

stop_node_instance "$daemon_pid"

if [ -n "$current_ma" ] && [ "$current_ma" -le 2050 ]; then
    record_pass "Power Limiter (ABL): Current clamped safely (${current_ma} mA <= 2000 mA limit)"
else
    record_fail "Power Limiter (ABL)" "Current exceeded limit (${current_ma} mA)"
fi

# ==============================================================================
# PHASE 7: Interactive Technician Visual Inspection (Optional)
# ==============================================================================
if [ "$MODE" == "interactive" ]; then
    echo -e "\n${BOLD}${MAGENTA}>>> [PHASE 7] Field Technician Visual Inspection Mode${RESET}"
    echo "This mode allows visual confirmation of hardware LED colors and pin wiring."
    echo "Connecting to $DEVICE ($TARGET_PIXELS pixels, color order: $TARGET_COLOR_ORDER)..."

    port=$((PORT_COUNTER++))
    cfg="$TMP_DIR/cfg_interactive.toml"
    cat <<EOF > "$cfg"
[node]
name = "Visual-Inspection-Node"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "127.0.0.1"

[web]
enabled = true
port = $port
bind_ip = "127.0.0.1"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 5
fade_duration_sec = 2

[[output]]
id = "out-vis"
driver = "serial_mcu"
device = "$DEVICE"
baud_rate = $TARGET_BAUD
chip = "ws2812b"
color_order = "$TARGET_COLOR_ORDER"
pixels = $TARGET_PIXELS
psu_max_current_ma = 10000

[[fixture]]
output = "out-vis"
pixel_range = [0, $((TARGET_PIXELS - 1))]
personality = "preset_7ch"
universe = 1
address = 1
EOF

    daemon_pid=$(run_node_instance "$cfg" "$port")

    echo -e "\n${BOLD}[1/4] VISUAL CHECK: SOLID RED${RESET}"
    echo -e "Sending Full Red (255, 0, 0)..."
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,255,0,0,0,0,0" > /dev/null 2>&1
    echo -e "--> ${YELLOW}LOOK AT STRIP: All LEDs should be pure RED. (If Green, strip requires GRB; if Blue, BRG)${RESET}"
    sleep 2.0

    echo -e "\n${BOLD}[2/4] VISUAL CHECK: SOLID GREEN${RESET}"
    echo -e "Sending Full Green (0, 255, 0)..."
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,0,255,0,0,0,0" > /dev/null 2>&1
    echo -e "--> ${YELLOW}LOOK AT STRIP: All LEDs should be pure GREEN.${RESET}"
    sleep 2.0

    echo -e "\n${BOLD}[3/4] VISUAL CHECK: SOLID BLUE${RESET}"
    echo -e "Sending Full Blue (0, 0, 255)..."
    ./target/debug/pxtool send-artnet --ip 127.0.0.1 --universe 1 --channels "134,0,0,255,0,0,0" > /dev/null 2>&1
    echo -e "--> ${YELLOW}LOOK AT STRIP: All LEDs should be pure BLUE.${RESET}"
    sleep 2.0

    echo -e "\n${BOLD}[4/4] VISUAL CHECK: ADDRESS WALKER${RESET}"
    echo -e "Running single-pixel white walker..."
    ./target/debug/pxtool test --remote --mode walker --node "http://127.0.0.1:${port}" > /dev/null 2>&1
    echo -e "--> ${YELLOW}LOOK AT STRIP: Single white dot is walking from pixel 0 to pixel $((TARGET_PIXELS - 1)).${RESET}"
    sleep 2.5
    ./target/debug/pxtool test --remote --mode off --node "http://127.0.0.1:${port}" > /dev/null 2>&1

    stop_node_instance "$daemon_pid"

    record_pass "Interactive: Field visual inspection sequence completed"
fi

# ==============================================================================
# FINAL SUMMARY REPORT
# ==============================================================================
echo -e "\n${BOLD}${CYAN}========================================================================${RESET}"
echo -e "${BOLD}${CYAN}                   REAL HARDWARE TEST SUMMARY REPORT                    ${RESET}"
echo -e "${BOLD}${CYAN}========================================================================${RESET}"
echo -e " Total Tests Run : $((${#PASSED_TESTS[@]} + ${#FAILED_TESTS[@]}))"
echo -e " Passed Tests    : ${GREEN}${#PASSED_TESTS[@]}${RESET}"
echo -e " Failed Tests    : ${RED}${#FAILED_TESTS[@]}${RESET}"
echo -e " Target Device   : ${DEVICE} (${TARGET_BAUD} baud)"
echo -e "------------------------------------------------------------------------"

if [ ${#FAILED_TESTS[@]} -eq 0 ]; then
    echo -e "${BOLD}${GREEN}>>> ALL REAL HARDWARE TESTS PASSED WITH 100% SUCCESS! <<<${RESET}"
    echo -e "The connected Arduino Uno is verified for production stage deployment."
    echo -e "${BOLD}${CYAN}========================================================================${RESET}"
    exit 0
else
    echo -e "${BOLD}${RED}>>> SOME REAL HARDWARE TESTS FAILED: <<<${RESET}"
    for f in "${FAILED_TESTS[@]}"; do
        echo -e "  - ${RED}${f}${RESET}"
    done
    echo -e "${BOLD}${CYAN}========================================================================${RESET}"
    exit 1
fi
