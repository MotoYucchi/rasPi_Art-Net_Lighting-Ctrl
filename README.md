# PixelNode

> **Best-Effort Art-Net & sACN Pixel LED Controller for Linux & Raspberry Pi**  
> *Developed by MotoYucchi*

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Target: Raspberry Pi 3B / x86_64](https://img.shields.io/badge/Platform-Raspberry%20Pi%20%7C%20Linux%20x86__64-lightgrey.svg)](#supported-hardware)

Language: [English](README.md) | [日本語 (Japanese)](README.ja.md)

---

## Overview

**PixelNode** is an open-source software node written in Rust designed to receive **Art-Net 4** and **sACN (E1.31)** lighting data over standard IP networks and drive digital pixel LED strips (such as WS2811 / WS2812B) via hardware SPI on Linux systems, primarily targeting the Raspberry Pi 3B and standard x86_64 platforms.

It includes an embedded headless web dashboard, diagnostic CLI utility (`pxtool`), multi-personality DMX profiles (Preset 7ch, Standard 12ch, Pixel Direct RGB), and real-time audio-reactive modulation.

> [!NOTE]
> **Engineering Reality & Scope**  
> PixelNode runs on a general-purpose Linux kernel (Raspberry Pi OS / standard Linux) and utilizes software timers and asynchronous network I/O. While it incorporates design measures aimed at reducing jitter and latency, **it is not a hard real-time operating system (RTOS)**. Transmission latency and frame timing can be influenced by OS scheduler latency, CPU load, SPI bus arbitration, and network conditions. We strongly encourage pre-production rig-checks and practical verification in your target stage environment.

---

## Design Principles

PixelNode was designed with the following practical goals in mind:

* **Zero-Allocation Rendering Loop**: The inner rendering pipeline (`RenderEngine`) avoids heap allocations (`Vec`, `Box`, format strings) in its hot frame path to minimize garbage collection or allocator-induced latency spikes.
* **Separation of Concerns**: Protocol parsing and pixel output (data plane) are designed to run independently from the web dashboard and CLI monitoring tools (control plane).
* **Failsafe Gracefulness**: Configurable signal-loss policies (`hold`, `hold_then_fade`, `blackout`) to help mitigate sudden blackouts caused by transient network interruptions.
* **Power Limit Awareness (ABL)**: Built-in software Automatic Brightness Limiter to help scale total output current proportionately if estimated power draw exceeds a configured threshold, reducing the risk of power supply sag.
* **Field Serviceability**: Complete standalone diagnostics and rig-checking without requiring a lighting desk to be transmitting.

---

## Technical Constraints & Hardware Considerations

When deploying PixelNode in professional stage, live concert, or architectural lighting installations, please consider the following hardware and system factors:

### 1. Timing Jitter & Software Timers
* **General-Purpose OS vs. RTOS**: Linux does not guarantee microsecond-level determinism. Pixel transmission via the Linux `spidev` driver with DMA significantly reduces CPU overhead, but occasional timing jitter from kernel thread preemption or hardware interrupts may still occur.
* **Core Frequency Stabilization (Raspberry Pi 3B)**: On the Pi 3B, the SPI clock is derived from the VideoCore VPU clock. Under dynamic frequency scaling, SPI timing can fluctuate and cause flickering. We provide configuration recipes (e.g. `core_freq=250`) to fix the clock frequency, though complete elimination of timing variance cannot be guaranteed.

### 2. Electrical Noise & Signal Level Shifting
* **5V Logic Level Translation**: Raspberry Pi GPIO pins output 3.3V logic. Most 24V/5V WS2811 ICs specify a high-level input voltage of $V_{IH} \ge 0.7 \times V_{DD}$ ($\approx 3.5\text{V}$). Connecting a 3.3V GPIO pin directly to the LED data line may result in low noise margins, data corruption, and flickering. **Using a dedicated high-speed CMOS level shifter (e.g., 74AHCT125) with a 33–100Ω series damping resistor is strongly recommended.**
* **Common Ground & Voltage Sag**: High-density 24V strips reduce voltage drop compared to 5V strips, but adequate power injection points and a clean, common reference ground between the Raspberry Pi and the LED power supplies are necessary.

---

## System Architecture

The PixelNode ecosystem consists of three main components:

```
+-------------------------------------------------------------------------+
|                              PixelNode                                  |
|                                                                         |
|  +---------------------+   +---------------------+   +---------------+  |
|  |     pixelnoded      |   |       pxtool        |   |audio-analyzerd|  |
|  |   (Main Daemon)     |   |   (Diagnostic CLI)  |   | (Audio Sync)  |  |
|  |                     |   |                     |   |               |  |
|  | - Art-Net 4 / sACN  |   | - Live Telemetry    |   | - Dual-Res    |  |
|  | - RenderEngine      |   | - SPI Rig Check     |   |   FFT Engine  |  |
|  | - Web Management UI |   | - Dynamic Config    |   | - UDP Feature |  |
|  | - REST API & RDM    |   | - Network Sniffer   |   |   Broadcaster |  |
|  +----------+----------+   +----------+----------+   +-------+-------+  |
|             |                         |                      |          |
+-------------|-------------------------|----------------------|----------+
              | /dev/spidev0.0          | HTTP / REST          | UDP 8765
              v                         v                      v
     +-----------------+       +-----------------+    +-----------------+
     |  WS2811 Strip   |       | Web UI / Desk   |    | Audio Loopback  |
     |  (via 74AHCT125)|       | (Browser / CLI) |    | (DAW / Line-In) |
     +-----------------+       +-----------------+    +-----------------+
```

* **`pixelnoded`**: Core service daemon that handles network DMX ingestion, fixture personality decoding, pattern synthesis, power limiting, and SPI transmission. Also serves the local web dashboard.
* **`pxtool`**: Field CLI utility for querying telemetry, triggering test patterns, sniffing network DMX packets, and modifying runtime settings.
* **`audio-analyzerd`**: Optional helper process for analyzing local audio input (via USB line-in) and broadcasting compact feature frames (`AudioFeaturePacket`) over UDP.

---

## DMX Personalities (Control Modes)

PixelNode supports three main operational personalities:

### 1. Preset 7ch (Recommended / Compact Footprint)
Designed for efficient DMX channel conservation, packing master dimming, strobe, full RGBW mixing, autonomous animation patterns, and audio-reactive synchronization into just **7 DMX channels**:

| Channel | Function | Range | Description |
|:---:|---|:---:|---|
| **1** | Dimmer / Strobe | 0–255 | 0–7: Blackout / 8–134: Master Dimmer / 135–239: Strobe (1–25Hz) / 240–255: Full Open |
| **2** | Red | 0–255 | Base Color Red (0%–100%) |
| **3** | Green | 0–255 | Base Color Green (0%–100%) |
| **4** | Blue | 0–255 | Base Color Blue (0%–100%) |
| **5** | White | 0–255 | Base Color White (RGB blend or native white) |
| **6** | Pattern | 0–255 | 24 Autonomous Animation Patterns (8-value slot spacing) |
| **7** | Audio Sync | 0–255 | 0–7: Off (follows Ch 6) / 8–255: 24 Audio-Reactive Patterns (Overrides Ch 6) |

*Full slot ranges and algorithm details can be found in the [DMX Channel & Slot Specification](docs/dmx_channel_slot_specification.md).*

### 2. Standard 12ch (Expanded Parameter Mode)
Provides separate fader control over Pattern Speed, Pattern Size, Audio Mode, and Audio Gain for operators who want granular manual control.

### 3. Pixel Direct RGB (Media Server / Pixel Mapping)
Direct 3-channel per pixel control ($N \times 3\text{ ch}$) with multi-universe spanning support (`per_170px` industry standard or `packed`).

---

## Lighting Desk Profile Integrations

Pre-built fixture definitions are included in the [`docs/fixtures/`](docs/fixtures/) directory:
* **ChamSys MagicQ**: `MagicQ_PixelNode_Preset7ch.hed`
* **QLC+**: `QLC+_PixelNode_Preset7ch.qxf`
* **Daslight 4/5**: `Daslight_PixelNode_Preset7ch.ssl2`
* **GDTF (grandMA3 etc.)**: `gdtf_description.xml`

---

## Quick Start

### Prerequisites
* Rust 1.75 or later (`cargo`, `rustc`)
* Linux system (Raspberry Pi OS 64-bit or Ubuntu/Debian x86_64)
* ALSA development libraries (for audio analysis):
  ```bash
  sudo apt-get install -y libasound2-dev
  ```

### 1. Build
```bash
git clone https://github.com/motoyucchi/rasPi_Art-Net_Lighting-Ctrl.git pixelnode
cd pixelnode

# Build optimized release binaries
cargo build --release
```

### 2. Configuration
Copy the sample configuration file and customize your setup:
```bash
sudo mkdir -p /etc/pixelnode
sudo cp deploy/config.example.toml /etc/pixelnode/config.toml
```

### 3. Running `pixelnoded`
```bash
# Start daemon with configuration file
./target/release/pixelnoded /etc/pixelnode/config.toml
```

### 4. Field Inspection with `pxtool`
While `pixelnoded` is running, you can inspect live status from another terminal or remotely:
```bash
# View live telemetry, FPS, and channel meters
./target/release/pxtool status

# Run a 10-second RGBW cycle test pattern over the network
./target/release/pxtool test --mode rgbw --remote --duration 10

# Return node to live lighting desk control
./target/release/pxtool test --mode off --remote
```

### 5. Web Dashboard
Open `http://<node-ip>:8080/` in any modern web browser. The dashboard is completely self-contained with **zero external CDN dependencies**, operating properly in offline production networks.

---

## Documentation

Comprehensive architectural notes and references are provided in the [`docs/`](docs/) directory:

* **[Architecture & Design Specification](docs/Architecture&Design.md)**: In-depth technical specification, failure modes (FMEA), timing models, and audio FFT pipeline.
* **[DMX Channel & Slot Specification](docs/dmx_channel_slot_specification.md)**: Full reference table for all 24 lighting patterns and 24 audio-reactive modes.
* **[CLI Reference Manual (`pxtool`)](docs/cli_reference.md)**: Command-line syntax, packet sniffer, and diagnostic recipes.
* **[Web Management Interface Reference](docs/web_interface.md)**: REST API documentation and dashboard capabilities.
* **[Raspberry Pi 3B Setup Guide](deploy/raspi3b_setup.md)**: Hardware wiring, level shifter integration, and OS stabilization guide.

---

## Author & Acknowledgements

* **Author**: MotoYucchi ([@motoyucchi](https://github.com/motoyucchi))
* **Intended Use**: Pixel lighting installations, live events, experimental stage setups, and stage automation prototyping.

---

## License

This project is licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0))
* MIT License ([LICENSE-MIT](http://opensource.org/licenses/MIT))

at your option.
