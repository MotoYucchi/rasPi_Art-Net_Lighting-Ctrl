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

## 3. Professional Fail-safe & Safety Architecture

In live stage and architectural lighting, unexpected glitches, accidental cable disconnects, or host reboots must **never** cause erratic strobing or uncommanded blackouts. The Arduino Uno firmware incorporates multi-layered safety mechanisms:

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

> [!WARNING]
> Never power WS2811 / WS2812B strips directly from the Arduino Uno's 5V pin! Strips draw substantial current (up to 60 mA per pixel at full white). An external regulated 5V power supply is required.

```
              +----------------------------+
              | External 5V Power Supply   |
              | +5V                     GND|
              +--+-----------------------+-+
                 |                       |
                 |   +-----------------+ |
                 +---+ 1000uF Cap      | |
                 |   | (Electrolytic)  | |
                 |   +-----------------+ |
                 |                       |
                 +-----------------------+----------+ (+5V Power)
                 |                                  |
                 |                                  v
+----------------+--------+               +-------------------+
| Arduino Uno (ATmega328P)|               | WS2811 / WS2812B  |
|                         |   330Ω - 470Ω | LED Strip         |
| Pin D6 (Data Out) ------+---[ Resistor ]+-> Data Input      |
|                         |               |                   |
| GND --------------------+---------------+-> GND (Common)    |
| USB Port (To Linux/PC)  |               +-------------------+
+-------------------------+
```

### Critical Wiring Rules:
1. **Common Ground**: Connect the Arduino `GND` pin directly to the `GND` of the external 5V power supply. Without a common ground reference, the high-speed 800 kHz serial data signal will glitch.
2. **Data Resistor**: Place a `330 Ω` to `470 Ω` resistor between Arduino Pin D6 and the LED strip's `DATA IN`. This dampens transmission line ringing and protects the first pixel's input gate.
3. **Bulk Capacitor**: Connect a `1000 µF, 6.3V` or `16V` electrolytic capacitor across `+5V` and `GND` at the power supply terminals to absorb current surges when all LEDs turn on simultaneously.
4. **Data Pin**: The default data pin is **D6** (`PORTD, Bit 6`). Can be changed in `ws2812_driver.h` if needed.

---

## 5. Host Configuration (`config.toml`)

To use Arduino Uno via USB serial in `pixelnoded`, configure the output device in `config.toml`:

```toml
[node]
name = "PixelNode-Arduino-01"
show_lock = false

[[output]]
id = "out-uno-01"
driver = "serial_mcu"       # or "usb_serial"
device = "/dev/ttyACM0"     # standard Arduino Uno CDC port (/dev/ttyUSB0 for CH340 clones)
baud_rate = 115200          # 115200 standard (or 250000 for zero-error divisor)
chip = "ws2812b"
color_order = "grb"         # grb for WS2812B, brg for WS2811
pixels = 60                 # recommended <= 170 pixels for Uno
psu_max_current_ma = 10000  # 10A current limit (ABL)

[[fixture]]
output = "out-uno-01"
pixel_range = [0, 59]
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
# Run cross-language and native C++ conformance tests:
PATH="$HOME/.cargo/bin:$PATH" cargo test --test arduino_conformance_test
```

All 24 autonomous patterns, 24 audio patterns, CRC-8 integrity vectors, and packet parser error recoveries will be automatically verified against the reference implementation.
