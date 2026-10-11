# PixelNode Real Hardware Testing Guide (Arduino Uno / ATmega328P)

## 1. Overview & Purpose

The **PixelNode Real Hardware Test & Diagnostic Suite** (`tests/test_real_hardware.sh`) is an exhaustive verification tool designed for lighting designers, stage technicians, and QA engineers deploying Arduino Uno (ATmega328P) as a USB-serial LED controller.

While software simulations test algorithms in memory, **real hardware verification** validates the complete physical pipeline:
- USB CDC / UART hardware transceivers (CH340 `1a86:7523` or ATmega16U2)
- Linux kernel `termios` raw mode configuration and high-baud communication
- Full-scale LED pixel counts (from 10 px up to the 170 px single-universe ceiling)
- Color order permutations (`grb`, `brg`, `rgb`, `bgr`, `rbg`, `gbr`)
- DMX personalities and pattern synthesizers
- Strobe timing gates and Power Limiter (ABL) current throttling
- Visual validation checklists for live stage fixtures

```
+-----------------------------------------------------------------------------------+
|                        Host (Linux PC / Raspberry Pi)                            |
|                                                                                   |
|  [Art-Net / sACN Generator]                                                      |
|           |                                                                       |
|           v                                                                       |
|  [pixelnoded Daemon] ---> [RenderEngine]                                          |
|                                 |                                                 |
|                                 +---> [SerialMcuSink (115200 / 250000 bps)]       |
+-----------------------------------------------------------------------------------+
                                          |
                                          | USB Link (/dev/ttyUSB0 / /dev/ttyACM0)
                                          v
+-----------------------------------------------------------------------------------+
|                       Arduino Uno (ATmega328P @ 16 MHz)                           |
|                                                                                   |
|  [Hardware UART] ---> [PacketParser (State Machine + CRC-8)]                      |
|                                 |                                                 |
|                                 +---> Pin D6 (WS2811/WS2812B Bitbang)             |
|                                                |                                  |
|                                                v                                  |
|                                       [Physical LED Strip]                        |
+-----------------------------------------------------------------------------------+
```

---

## 2. Test Matrix Architecture

The test suite executes up to **7 automated & interactive phases**:

```
+-----------------------------------------------------------------------------------+
|                           TEST MATRIX PHASES                                      |
+-----------------------------------------------------------------------------------+
| [Phase 1] Low-Level Serial & CRC-8 Wire Test (Cross-language golden vectors)     |
| [Phase 2] Pixel Count Scaling Sweep (10 px, 60 px, 120 px, 170 px MAX Universe)   |
| [Phase 3] Color Order Permutation Matrix (GRB, BRG, RGB, BGR)                     |
| [Phase 4] DMX Personalities Validation (Preset 7ch, Standard 12ch)               |
| [Phase 5] Pattern Animation Sweep (Ch 6 Slots 0, 1, 8, 16, 17)                   |
| [Phase 6] Strobe Gating & Power Limiter (ABL current clamping test)              |
| [Phase 7] Field Technician Visual Inspection (Interactive RGB & Walker sequence) |
+-----------------------------------------------------------------------------------+
```

### Phase 1: Low-Level Serial & CRC-8 Protocol Parity
- Opens the physical serial port (`/dev/ttyUSB0` or `/dev/ttyACM0`) using Rust's `SerialMcuSink`.
- Transmits configuration packets (`OP_CONFIG`) and DMX channel packets (`OP_DMX_CHANNELS`).
- Streams 50 consecutive frames (9,000 pixel bytes) at 50 FPS.
- Verifies that zero bytes are dropped and Dallas/Maxim CRC-8 checksums match.

### Phase 2: Pixel Count Scaling Sweep
Tests varying strip lengths to guarantee memory stability on the ATmega328P's 2 KB SRAM:
| Pixel Count | Payload Size | Typical Fixture Type | Verification Focus |
|:---:|:---:|:---|:---|
| **10 px** | 30 bytes | Accent spotlight / Mini puck | Minimal packet latency |
| **60 px** | 180 bytes | Standard 1-meter tape (60 LED/m) | Standard throughput (50 FPS) |
| **100 px** | 300 bytes | **Generic 60 LED/m 12V Tape (5m Reel)** | **Preset 2 field fixture profile** |
| **160 px** | 480 bytes | **BTF-LIGHTING FCOB 24V (10m Reel)** | **Preset 1 high-density COB profile** |
| **170 px** | 510 bytes | **Maximum Full DMX Universe (512 ch)** | **ATmega328P SRAM ceiling stress** |

### Phase 3: Color Order Permutation Matrix
WS2811/WS2812B chips from different manufacturers use different internal shift register configurations. The suite verifies proper byte packing across all permutations:
- **`grb`**: Standard WS2812B / NeoPixel strips.
- **`brg`**: Standard 12V/24V WS2811 bullet nodes and architectural neon strips.
- **`rgb`**: Direct 1-to-1 mapping (BTF-LIGHTING FCOB 24V).
- **`rbg`**: Generic / No-brand 12V SMD strips with swapped blue/green PCB routing.
- **`bgr`**: Common WS2811 alternative.
- **`gbr`**: Special batch variant.

### Phase 4: DMX Personality Validation
- **Preset 7ch**: Tests master dimmer, RGBW mixing, pattern slot selection, and audio bypass.
- **Standard 12ch**: Tests discrete strobe, color, pattern speed, and pattern size parameters.

### Phase 5: Pattern Animation Sweep
Verifies that DMX slot values correctly trigger animation algorithms on physical hardware:
- **Slot 0 (DMX 0)**: Static base color fill.
- **Slot 1 (DMX 8)**: Single anti-aliased chase head.
- **Slot 8 (DMX 64)**: Sine wave biological breathing.
- **Slot 16 (DMX 128)**: Continuous full-spectrum rainbow wave.
- **Slot 17 (DMX 136)**: Knight Rider / Scanner rebounding beam.

### Phase 6: Strobe Gating & Power Limiter (ABL)
- **Strobe Gating**: Tests 12Hz strobe rate cutoffs.
- **Power Limiter (ABL)**: Sets a strict current limit (e.g. 2,000 mA) and triggers a 100% full white soak test (`PowerSoak100`). Verifies that the internal ABL throttles brightness dynamically to protect the power supply.

### Phase 7: Field Technician Visual Inspection Mode (`--mode interactive`)
Enables on-site visual confirmation with guided terminal instructions:
1. **Solid Red (`#FF0000`)**: If LEDs light up Green, change color order to `grb`; if Blue, change to `brg`.
2. **Solid Green (`#00FF00`)**: Verifies green sub-pixel channel.
3. **Solid Blue (`#0000FF`)**: Verifies blue sub-pixel channel.
4. **Address Walker**: A single white dot walks sequentially from pixel 0 to pixel $N-1$, allowing technicians to physically count and verify strip length.

---

## 3. Command Line Usage

### 1. Exhaustive Automated Test (Default)
Runs all phases across all pixel counts, color orders, personalities, and patterns:
```bash
./tests/test_real_hardware.sh
```

### 2. Fast Smoke Test (`--mode quick`)
Runs a quick check on the connected hardware in ~10 seconds:
```bash
./tests/test_real_hardware.sh --mode quick
```

### 3. Interactive Field Inspection Mode (`--mode interactive`)
Guides the lighting technician through visual confirmation of hardware colors:
```bash
./tests/test_real_hardware.sh --mode interactive --pixels 60 --color-order grb
```

### 4. Custom Parameter Testing
```bash
# Test 120 pixels with BRG color order on /dev/ttyUSB0 at 115200 baud:
./tests/test_real_hardware.sh -p 120 -c brg -d /dev/ttyUSB0 -b 115200

# Test high-speed 250,000 baud mode:
./tests/test_real_hardware.sh -b 250000 --mode quick
```

---

## 4. Field Troubleshooting & Diagnostic Guide

| Symptom observed on Strip | Root Cause | Solution |
|:---|:---|:---|
| **Red appears Green, or Blue swapped** | Color order mismatch | Switch `color_order` in `config.toml` (e.g. from `rgb` to `grb` or `brg`). Run `test_real_hardware.sh --mode interactive` to confirm. |
| **Random flickering / spark glitches** | Data line ringing or floating ground | 1. Ensure Arduino GND is tied to 5V PSU GND.<br>2. Add a **330Ω to 470Ω** series resistor on Pin D6. |
| **Only the first few pixels light up** | Pixel count setting too low | Increase `pixels` in `[[output]]` and `pixel_range` in `[[fixture]]` in `config.toml`. |
| **LEDs turn faint red toward the end** | 5V voltage drop along strip | Inject 5V power at both ends of the LED strip or every 5 meters. |
| **Serial timeout / Device busy** | Stale daemon holding tty | Run `pkill -f pixelnoded` and check `ls -l /dev/ttyUSB*`. |
| **Board resets on serial connection** | Arduino DTR auto-reset | Normal behavior on connect. `pixelnoded` and the test harness wait for bootloader initialization automatically. |

---

## 5. Verification Log Example

```text
========================================================================
      PIXELNODE REAL HARDWARE TEST & DIAGNOSTIC SUITE (ARDUINO UNO)     
========================================================================
 Target Device : /dev/ttyUSB0
 Test Mode     : full
 Target Baud   : 115200 bps
------------------------------------------------------------------------

>>> [PHASE 1] Direct Serial Port & Low-Level Protocol Test
  [PASS] Phase 1: Direct Serial Hardware Write & CRC-8 framing on /dev/ttyUSB0

>>> [PHASE 2] Pixel Count Scaling Sweep
  [PASS] Pixel Scaling: 10 LEDs (30 bytes/frame, Frames rendered: 41)
  [PASS] Pixel Scaling: 60 LEDs (180 bytes/frame, Frames rendered: 42)
  [PASS] Pixel Scaling: 120 LEDs (360 bytes/frame, Frames rendered: 37)
  [PASS] Pixel Scaling: 170 LEDs (510 bytes/frame, Frames rendered: 26)

>>> [PHASE 3] Color Order Permutation Matrix
  [PASS] Color Order: 'grb' packing and primary RGB stream
  [PASS] Color Order: 'brg' packing and primary RGB stream
  [PASS] Color Order: 'rgb' packing and primary RGB stream
  [PASS] Color Order: 'bgr' packing and primary RGB stream

>>> [PHASE 4] DMX Personalities Validation
  [PASS] Personality: 'preset_7ch' channel mapping and decoding
  [PASS] Personality: 'standard' channel mapping and decoding

>>> [PHASE 5] Pattern Animation Sweep (Ch 6 Slots)
  [PASS] Pattern: Slot 0 (Static Base Fill)
  [PASS] Pattern: Slot 1 (Single Chase)
  [PASS] Pattern: Slot 8 (Sine Wave Breath)
  [PASS] Pattern: Slot 16 (Rainbow Wave)
  [PASS] Pattern: Slot 17 (Knight Rider Scanner)

>>> [PHASE 6] Strobe Gating & Power Limiter (ABL)
  [PASS] Strobe: 12Hz rate gating verified
  [PASS] Power Limiter (ABL): Current clamped safely (663 mA <= 2000 mA limit)

========================================================================
                   REAL HARDWARE TEST SUMMARY REPORT                    
========================================================================
 Total Tests Run : 18
 Passed Tests    : 18
 Failed Tests    : 0
 Target Device   : /dev/ttyUSB0 (115200 baud)
------------------------------------------------------------------------
>>> ALL REAL HARDWARE TESTS PASSED WITH 100% SUCCESS! <<<
The connected Arduino Uno is verified for production stage deployment.
========================================================================
```
