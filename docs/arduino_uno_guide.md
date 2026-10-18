# PixelNode Arduino Uno (ATmega328P) USB-Serial Interface Guide

## 1. Overview & Architecture

When running **PixelNode** in production environments where Raspberry Pi GPIO/SPI is unavailable or when offloading hardware driving to an isolated microcontroller, an **Arduino Uno (ATmega328P @ 16 MHz)** can be connected via USB port to a Linux PC or Raspberry Pi host.

The system is designed with a **dual-layer operational architecture** to ensure complete feature parity with Raspberry Pi rendering while maintaining fail-safe resilience against host freezes, USB disconnects, and cable noise.

```
+-------------------------------------------------------------------------------+
|                       Host System (Linux PC / Raspberry Pi)                  |
|                                                                               |
|  [Art-Net / sACN] ---> [DMX Receiver]                                         |
|  [Audio Mic/In]   ---> [audio-analyzerd] (FFT / Beat Detection)               |
|                               |                                               |
|                               v                                               |
|                    [RenderEngine (Rust)]                                      |
|                    - 24 Autonomous Patterns (Ch 6)                            |
|                    - 24 Audio Sync Patterns (Ch 7)                            |
|                    - Master Dimmer & 1..25Hz Strobe                           |
|                    - Gamma 2.2 Table & PSU Power Limiter (ABL)                |
|                               |                                               |
|                               v                                               |
|                    [SerialMcuSink (Rust)]                                     |
|                    - Framing: Magic 'PX', Sequence, Length, CRC-8             |
|                    - Serial Device: /dev/ttyACM0 or /dev/ttyUSB0 (115200 bps) |
+-------------------------------------------------------------------------------+
                                        |  USB Cable (CDC / UART)
                                        v
+-------------------------------------------------------------------------------+
|                       Arduino Uno (ATmega328P @ 16 MHz)                       |
|                                                                               |
|  [Hardware UART] ---> [PacketParser (State Machine + CRC-8)]                  |
|                               |                                               |
|            +------------------+------------------+                            |
|            | (Streaming Mode)                    | (DMX Command / Fallback)   |
|            v                                     v                            |
|  [OP_STREAM_PIXELS]                     [Onboard Pattern Engine]              |
|  Direct Strip Buffer                    - Same 24 Patterns (Ch 6)             |
|                                         - Same 24 Audio Sync (Ch 7)           |
|                                         - Fail-safe Ambient Generator         |
|                                                  |                            |
|            +-------------------------------------+                            |
|            |                                                                  |
|            v                                                                  |
|  [AVR Cycle-Accurate Driver / FastLED] ---> Pin D6 ---> [WS2811 / WS2812B]    |
|                                                                               |
|  [Safety Systems]:                                                            |
|  - Hardware Watchdog Timer (2.0s WDT)                                         |
|  - Software Link Timeout Watchdog (Hold / Fallback / Blackout)                |
|  - Zero Dynamic Memory Allocation (100% Static Bounded Buffers)               |
+-------------------------------------------------------------------------------+
```

---

## 2. Operating Modes

### Mode A: Host-Rendered Streaming Mode (`OP_STREAM_PIXELS`)
- **Default & Primary Mode**: The host `pixelnoded` daemon renders pixel data through its Rust `RenderEngine`.
- **Full Mathematical Parity**: 100% byte-for-byte exact fidelity with Raspberry Pi rendering, including 64-bit floating-point math, Gamma 2.2 correction, and PSU Current Limiting (ABL).
- **MCU Task**: The Arduino Uno simply unpacks the framed RGB payload into its output buffer, validates the Dallas/Maxim CRC-8 checksum, and transmits the bitstream to Pin D6.

### Mode B: Direct DMX Parameter / Fallback Mode (`OP_DMX_CHANNELS`)
- **Direct Parameter Control**: Host or lighting console transmits compact 10-byte DMX packets (`[Dimmer, R, G, B, Strobe, PatternCh6, AudioCh7, Speed, Size]`).
- **Onboard Pattern Engine**: The Arduino Uno executes the exact same mathematical patterns locally:
  - **All 24 Autonomous Patterns (DMX Ch 6)**: Static, Single Chase, Dual Meteor, Gravity Bounce, Newton's Cradle, Liquid Ripple, Plasma Flow, Sand Cascade, Sine Breath, Heartbeat, Campfire, Aurora, Electric Arc, Firefly, Neural Synapse, Jellyfish, Rainbow, Knight Rider, Curtain Wipe, Strobe Matrix, Diamond Sparkle, Sawtooth, Matrix Rain, and Moiré Grid.
  - **All 24 Audio-Reactive Patterns (DMX Ch 7)**: Master Pulse, Kick Flash, Snare Crack, Hi-Hat Shimmer, Spectrum VU, Bass Shockwave, EQ Split, Tempo Strobe, Oscilloscope, and full spectrum modes.
  - Same slot calculation (`slot = dmx / 8`), speed normalization, size scaling, and strobe gating.

---

## 3. Fail-safe & Safety Architecture

In live stage and architectural lighting, unexpected glitches, accidental cable disconnects, or host reboots must not cause erratic strobing or uncommanded blackouts. The Arduino Uno firmware incorporates multi-layered safety mechanisms:

| Safety Mechanism | Implementation | Production Behavior |
| :--- | :--- | :--- |
| **AVR Hardware Watchdog** | `wdt_enable(WDTO_2S)` | If the MCU firmware ever hangs or deadlocks, the hardware automatically reboots the ATmega328P within 2.0 seconds. |
| **Link Loss Detection** | `(now - last_packet_time) > timeout_ms` (Default: 500 ms) | Detects USB disconnection or host crash within 500 ms and transitions to the configured fail-safe policy. |
| **Fail-safe: HOLD** (`PX_FAILSAFE_HOLD`) | Static frame buffer refresh | **Default for live shows.** Freezes and continuously refreshes the last valid stage picture so the audience sees no interruption. |
| **Fail-safe: AUTONOMOUS** (`PX_FAILSAFE_AUTONOMOUS`) | Onboard pattern engine | Activates an emergency ambient warm amber breath (`#FF7814`) so backstage and stage technicians are not left in complete darkness. |
| **Fail-safe: BLACKOUT** (`PX_FAILSAFE_BLACKOUT`) | Zero buffer output | Smoothly clears the LED strip to 0. |
| **CRC-8 Integrity Check** | Dallas/Maxim polynomial `0x31` | Corrupted bytes caused by ground loops or electromagnetic interference are dropped before reaching the LED strip. |
| **Zero Dynamic Allocation** | `no_alloc` / no `malloc` / no `String` | Completely eliminates memory fragmentation crashes on ATmega328P's 2 KB SRAM ceiling. |

---

## 4. Hardware Wiring & Electrical Specifications

> [!CAUTION]
> **CRITICAL VOLTAGE SEPARATION**:  
> Never connect 24V or 12V power supply positive to the Arduino Uno's 5V, 3.3V, VIN, or GPIO pins! The Arduino Uno operates from host USB 5V power. Only the **DC Negative / GND** is shared between the LED power supply and the Arduino.

### Wiring Diagram for 24V (BTF-LIGHTING FCOB 10m) / 12V (Generic Strip 5m):

```
         +-----------------------------------------------+
         | External LED Power Supply (24V or 12V DC)     |
         | +V (24V / 12V)                       GND (DC-)|
         +---+--------------------------------------+-+--+
             |                                      | |
  Power Feed | (Dual-end power injection)           | | (Common GND)
             |                                      | |
  +----------+--------------------+                 | |
  |                               |                 | |
  v                               v                 | |
+----+----------------------------+----+            | |
| +V | LED Strip (WS2811 24V / 12V)| +V |           | |
| DI | Length: 10m (24V) / 5m (12V)| DI |           | |
|GND |                             |GND |           | |
+--+-+----------------------------+--+-+            | |
   ^                                 ^              | |
   |                                 +--------------+ |
   |                                 (End Injection)  |
   |                                                  |
   +--------------------------------------------------+
   | (Start Injection GND)
   |
   | Data Line (5V Logic)
   |       330Ω - 470Ω Resistor
   +-------[   ///   ]-------+
                             |
                     Pin D6  |
            +----------------+---------+
            | Arduino Uno (ATmega328P) |
            |                          |
            | GND ---------------------+ (Common Ground Wire)
            | USB Port                 |
            +------------+-------------+
                         | USB Cable
                         v
            +--------------------------+
            | Host (Linux PC / RasPi)  |
            +--------------------------+
```

### Critical Wiring Rules for Field Deployment:
1. **Common Ground**: Connect the Arduino `GND` pin directly to the `GND` (negative terminal) of the external 24V or 12V power supply with a low-resistance lead. Without a solid ground reference, high-frequency 800 kHz bitbang data will suffer from frame corruption and flickering.
2. **Data Resistor**: Place a `330 Ω` to `470 Ω` resistor in series between Arduino Pin D6 and the LED strip's `DATA IN`. This dampens transmission line reflections and protects the first WS2811 IC.
3. **Dual-End Power Injection**:
   * **BTF FCOB 24V (10m)**: Connect 24V and GND to both ends (0m and 10m).
   * **Generic 12V (5m)**: Connect 12V and GND to both ends (0m and 5m).
4. **Data Pin**: The default data output pin is **D6** (`PORTD, Bit 6`). Can be customized in `firmware/arduino_uno/ws2812_driver.h` if required.

---

## 5. Host Configuration (`config.toml`)

You can generate configuration files directly using pre-configured hardware profiles:

```bash
# Generate config for BTF-LIGHTING FCOB 24V (10m, 160 px, RGB) via Arduino USB-Serial:
pxtool config preset apply btf_fcob_576_24v_10m --driver serial_mcu --device /dev/ttyUSB0 -o /etc/pixelnode/config.toml

# Generate config for Generic 12V Strip (5m, 100 px, RBG) via Arduino USB-Serial:
pxtool config preset apply nobrand_60led_12v_5m --driver serial_mcu --device /dev/ttyUSB0 -o /etc/pixelnode/config.toml
```

Example `config.toml` structure for Arduino Uno output:

```toml
[node]
name = "PixelNode-Arduino-01"
show_lock = false

[[output]]
id = "out-uno-01"
driver = "serial_mcu"       # or "usb_serial"
device = "/dev/ttyUSB0"     # /dev/ttyUSB0 (CH340) or /dev/ttyACM0 (official Uno)
baud_rate = 115200          # 115200 standard baud
chip = "ws2811"             # ws2811 | ws2812b
color_order = "rgb"         # "rgb" for BTF FCOB, "rbg" for Generic 12V
pixels = 160                # 160 px (BTF FCOB 10m) or 100 px (Generic 12V 5m)
psu_max_current_ma = 7000   # Automatic Brightness Limiter (ABL) current cap

[[fixture]]
output = "out-uno-01"
pixel_range = [0, 159]      # [0, pixels - 1]
personality = "preset_7ch"
universe = 1
address = 1
```

---

## 6. Firmware Compilation & Upload

The firmware has **zero external library dependencies** and compiles cleanly in the official Arduino IDE or PlatformIO.

### Using Arduino IDE:
1. Open `firmware/arduino_uno/pixelnode_uno.ino` in Arduino IDE.
2. Select **Tools > Board > Arduino Uno**.
3. Select **Tools > Port > /dev/ttyACM0** (Linux) or **COMx** (Windows).
4. Click **Upload** (Ctrl+U).

### Verification & Automated Testing:
Run the strict conformance test suite locally using Cargo and native C++:

```bash
# 1. Direct hardware test via pxtool (runs directly on connected Arduino without starting daemon):
pxtool test --serial /dev/ttyUSB0 --preset btf --mode rgbw
pxtool test --serial /dev/ttyUSB0 --preset generic --mode walker

# 2. Run cross-language and native C++ conformance tests:
PATH="$HOME/.cargo/bin:$PATH" cargo test --test arduino_conformance_test

# 3. Run exhaustive real hardware test matrix (pixel sweeps, color orders, DMX, strobe, ABL):
./tests/test_real_hardware.sh --mode quick

# 4. Run interactive visual inspection for field technicians:
./tests/test_real_hardware.sh --mode interactive
```

---

## 7. Power-On Visual Confidence Check & Troubleshooting

### Startup Self-Test (Power-On Confidence Sequence)
Upon powering on or rebooting, the Arduino Uno firmware immediately performs a 400ms visual confidence check on Digital Pin **D6**:
- **First 5 LEDs flash**: **Red** (100ms) $\rightarrow$ **Green** (100ms) $\rightarrow$ **Blue** (100ms) $\rightarrow$ **Warm Amber** (100ms) $\rightarrow$ **Off/Blackout**.
- **What this verifies for field technicians**:
  1. Arduino firmware is running and not hung.
  2. Pin D6 signal output and data line continuity to the strip are functional.
  3. External 12V/24V power supply and Common Ground (GND) are properly connected.
  4. Red, Green, and Blue channels can be visually confirmed.

### Troubleshooting Checklist:
1. **LEDs do not illuminate during the 400ms startup sequence**:
   - Verify external 12V/24V power supply is on and providing voltage at the strip injection points.
   - Verify the Common GND wire between Arduino Uno GND and power supply DC negative is securely connected.
   - Verify the data wire is connected to Arduino Pin **D6** and to strip **DATA IN (DI)**, not DATA OUT (DO).
2. **First few LEDs light up during startup, but no response from host commands**:
   - When the host opens the USB-Serial port (`/dev/ttyUSB0`), the Arduino Uno resets via DTR and takes **1.8 seconds** for the Optiboot bootloader to finish before user code runs. `pxtool` and `pixelnoded` automatically include this startup delay.
   - Run `pxtool test --serial /dev/ttyUSB0 --preset btf --mode rgbw` to verify host-to-MCU streaming.
3. **Flashing with Adafruit_NeoPixel or FastLED**:
   - The firmware automatically detects and uses `<Adafruit_NeoPixel.h>` if available in the Arduino environment. If not installed, it falls back to a cycle-accurate 16MHz AVR unrolled assembly driver with zero external dependencies.
