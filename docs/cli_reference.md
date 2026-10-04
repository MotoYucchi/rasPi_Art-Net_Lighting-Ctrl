# PixelNode CLI Tool (`pxtool`) Reference Manual

`pxtool` is the official field diagnostic, network monitoring, remote configuration, and rig-checking command-line utility for the PixelNode lighting system.

It is designed for rapid troubleshooting on stage, headless terminal environments via SSH, and automated testing pipelines.

---

## Command Overview

```
pxtool <COMMAND>

Commands:
  status       Query status, FPS, live channel values, and health from running pixelnoded
  monitor      Real-time lighting network traffic monitor (packet sniffer for Art-Net / sACN)
  test         Run standalone rig-check self-test patterns (local SPI or via remote daemon)
  config       View, validate, or modify node configuration
  lock         Toggle live show lock (protect node against accidental edits during show)
  send-artnet  Transmit test Art-Net DMX frame
  send-sacn    Transmit test sACN DMX frame
  help         Print this message or help for a specific subcommand
```

---

## 1. `pxtool status`

Queries a running `pixelnoded` daemon and prints comprehensive real-time telemetry.

### Options
* `-n, --node <URL>`: PixelNode HTTP endpoint URL (Default: `http://127.0.0.1:8080`).
* `--json`: Output raw machine-readable JSON instead of human-readable view.

### Examples

#### Human-readable terminal view:
```bash
# Query local node
pxtool status

# Query remote Raspberry Pi node over network
pxtool status --node http://192.168.1.50:8080
```

**Example Output:**
```
========================================================================
 PIXELNODE STATUS: Stage-WS2811-Node1 (v0.1.0)
========================================================================
 Uptime: 02h 15m 40s | Show Lock: UNLOCKED 🔓 | Failsafe: INACTIVE (OK)
------------------------------------------------------------------------
[RENDERING]
 Target: 50.0 FPS | Actual: 50.0 FPS | Total Frames: 407,000
 Power Draw (ABL): 3.20 A / 15.0 A (21%) | Status: OK
 Active Test Override: None (DMX Console Controlled)

[NETWORK & PROTOCOL]
 Art-Net Packets Rx: 24,500 | sACN Packets Rx: 0
   Universe   1 | Source: ArtNet   | State: ACTIVE (Age: 20ms)

[LIVE CHANNEL VALUES]
   Ch 1 [Dim/Strobe]: 255 [████████████████] 100%
   Ch 2 [Red       ]: 255 [████████████████] 100%
   Ch 3 [Green     ]:   0 [░░░░░░░░░░░░░░░░]   0%
   Ch 4 [Blue      ]: 128 [████████░░░░░░░░]  50%
   Ch 5 [White     ]:   0 [░░░░░░░░░░░░░░░░]   0%
   Ch 6 [Pattern   ]:   5 [░░░░░░░░░░░░░░░░]   2%
   Ch 7 [Audio Sync]:   0 [░░░░░░░░░░░░░░░░]   0%

[AUDIO REACTIVE SYNC]
 Status: CONNECTED | BPM: 128.5 | RMS Energy: 78%
 Beat Triggers: [KICK: ON ] [SNARE: OFF] [HIHAT: ON ]
========================================================================
```

#### JSON Output (for automation and monitoring scripts):
```bash
pxtool status --json | jq .fps_x10
```

---

## 2. `pxtool monitor`

Real-time network packet sniffer for lighting protocols. Listens on Art-Net (UDP 6454) and sACN (UDP 5568) and logs incoming packets with timestamp, source IP, universe, sequence number, and preview of channel values.

### Options
* `-i, --interface <IP>`: Local interface IP to bind (Default: `0.0.0.0`).
* `-p, --protocol <artnet|sacn|all>`: Filter protocols (Default: `all`).
* `-u, --universe <ID>`: Filter by specific universe ID (Default: show all).
* `-c, --channels <COUNT>`: Number of channel values to preview per packet (Default: `8`).
* `-d, --duration <SEC>`: Stop monitoring after N seconds (Default: `0` = run indefinitely).

### Examples

```bash
# Monitor all lighting traffic on all interfaces
pxtool monitor

# Isolate Art-Net traffic for Universe 1 only
pxtool monitor --protocol artnet --universe 1

# Capture 10 seconds of traffic and show 16 channel values
pxtool monitor --duration 10 --channels 16
```

**Example Output:**
```
========================================================================
 Lighting Network Monitor on interface '0.0.0.0' (Filter: artnet)
 Filtering Universe: 1
 Press Ctrl+C to stop
========================================================================
[  0.02s] ART-NET | Src: 192.168.1.100   | Univ: 1   | Seq: 12  | Chs(8): [255,255,  0,128,  0,  5,  0,  0]
[  0.04s] ART-NET | Src: 192.168.1.100   | Univ: 1   | Seq: 13  | Chs(8): [255,255,  0,128,  0,  5,  0,  0]
[  0.06s] ART-NET | Src: 192.168.1.100   | Univ: 1   | Seq: 14  | Chs(8): [255,255,  0,128,  0,  5,  0,  0]
```

---

## 3. `pxtool test`

Standalone rig-check and hardware self-test pattern generator. Can run either **locally** directly on Raspberry Pi hardware SPI or **remotely** via the running `pixelnoded` daemon.

### Test Patterns
* `rgbw`: Red -> Green -> Blue -> White cycling (1s interval) to verify color order and defective subpixels.
* `walker`: Single bright white pixel walking sequentially down the strip (verifies pixel count, continuity, and direction).
* `soak`: 100% full white continuous burn-in test to stress-test 24V power supplies, injection cables, and thermal limits.
* `strobe`: 40Hz high-rate strobe (tests signal latch timing and integrity).
* `color`: Solid user-specified RGB color (with `--color R,G,B`).
* `off`: Terminate test mode and return to lighting console DMX control.

### Options
* `-m, --mode <MODE>`: `rgbw` | `walker` | `soak` | `strobe` | `color` | `off`.
* `-p, --pixels <N>`: Total number of pixels (Default: `300`).
* `--color-order <ORDER>`: `brg` | `rgb` | `grb` (Default: `brg`).
* `--color <R,G,B>`: Custom color for `color` mode (e.g. `255,128,0`).
* `--spi <PATH>`: Direct Linux SPI device path (e.g. `/dev/spidev0.0`). Omit for console preview.
* `-r, --remote`: Trigger test pattern on running `pixelnoded` daemon via REST API.
* `-n, --node <URL>`: Remote daemon URL when `--remote` is used (Default: `http://127.0.0.1:8080`).
* `-d, --duration <SEC>`: Run for N seconds, then exit (Default: `0` = infinite).

### Examples

#### Local SPI Execution (Direct hardware check):
```bash
# Test 576 pixels of WS2811 24V strip on Raspberry Pi SPI
pxtool test --mode rgbw --pixels 576 --color-order brg --spi /dev/spidev0.0

# 30-second full power soak test to check PSU voltage sag
pxtool test --mode soak --pixels 576 --spi /dev/spidev0.0 --duration 30

# Address walker to count exact pixels
pxtool test --mode walker --pixels 576 --spi /dev/spidev0.0
```

#### Remote Execution (Over network, while daemon is running):
```bash
# Trigger RGBW test pattern on remote stage node
pxtool test --remote --mode rgbw --node http://192.168.1.50:8080

# Send solid warm amber color
pxtool test --remote --mode color --color 255,140,20 --node http://192.168.1.50:8080

# Return node to live lighting console DMX control
pxtool test --remote --mode off --node http://192.168.1.50:8080
```

---

## 4. `pxtool config`

View, validate, and dynamically update configuration.

### Subcommands

### `pxtool config show`
Displays active configuration from the running node or from a local TOML file.

```bash
# Query active configuration from running node
pxtool config show

# View remote node configuration
pxtool config show --node http://192.168.1.50:8080

# Print local configuration file
pxtool config show --path /etc/pixelnode/config.toml
```

### `pxtool config validate <PATH>`
Validates configuration file syntax, structure, DMX universe/channel boundaries, and PSU settings prior to deployment.

```bash
pxtool config validate /etc/pixelnode/config.toml
```
**Output:**
```
Validating configuration file: '/etc/pixelnode/config.toml'...
PASS: Configuration TOML syntax and structure are valid.
```

### `pxtool config set`
Modifies settings on a running node and saves them atomically to disk.

```bash
# Change fixture DMX universe and start address
pxtool config set --universe 2 --address 1

# Change personality to Pixel Direct RGB (Media server mode)
pxtool config set --personality pixel_direct

# Change color order to GRB and adjust pixel count
pxtool config set --color-order grb --pixels 576

# Set maximum PSU current limit to 20 Amperes (20000 mA)
pxtool config set --max-current-ma 20000
```

---

## 5. `pxtool lock`

Toggles the Live Show Lock. When Show Lock is active:
* Configuration edits via REST API and Web UI are rejected (HTTP 403 Forbidden).
* Standalone Rig Check overrides are disabled.
* Live show DMX operation is protected from accidental button presses or misconfiguration.

```bash
# Lock node prior to show start
pxtool lock --enable

# Unlock node for maintenance
pxtool lock --disable

# Query remote node and lock it
pxtool lock --node http://192.168.1.50:8080 --enable
```

---

## 6. `pxtool send-artnet` and `send-sacn`

Injects test frames into the network to test nodes, profiles, or network paths.

```bash
# Send Art-Net frame with Dimmer at 100%, Red at 100%
pxtool send-artnet --ip 192.168.1.50 --universe 1 --channels "255,255,0,0,0,0,0"

# Send sACN frame with Priority 150
pxtool send-sacn --ip 192.168.1.50 --universe 1 --priority 150 --channels "255,0,255,0,0,0,0"
```

---

## Quick Troubleshooting Recipes

| Problem | Command to diagnose |
| :--- | :--- |
| **No output on strip** | `pxtool status` -> Check `Art-Net Packets Rx` and `Actual FPS`. |
| **Colors are wrong (e.g. Red is Blue)** | `pxtool test --mode rgbw` -> Observe order: R->G->B. If Red displays as Blue, switch `--color-order brg`. |
| **Lighting console not seeing node** | Check `pxtool monitor --protocol artnet` to see if console ArtPoll packets reach the node. |
| **Node dropping output during bass drop** | `pxtool status` -> Check `Power Draw (ABL)` % to see if PSU limit was reached. |
| **Accidental configuration changes feared** | `pxtool lock --enable`. |
