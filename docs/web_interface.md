# PixelNode Web Management Interface & REST API

PixelNode includes an embedded, zero-CDN, responsive dark-mode Web Dashboard and REST API built directly into the daemon (`pixelnoded`).

It enables lighting designers, technicians, and operators to monitor node status, trigger rig tests, and adjust configurations from any smartphone, tablet, or laptop on the stage network without needing an SSH client or terminal access.

---

## 1. Accessing the Web Dashboard

Once `pixelnoded` is running on a Raspberry Pi or Linux machine:

```
http://<node-ip-address>:8080/
```

Example: `http://192.168.1.50:8080/`

> **Note**: The web interface is completely self-contained. It requires **no internet access**, no external fonts, and no CDN dependencies. It functions perfectly in completely offline production networks.

---

## 2. Dashboard Features

### 1. Telemetry & Monitor View
* **Render Performance**: Displays real-time rendering frame rate (50.0 FPS target) and total rendered frame counter.
* **Network DMX Traffic**: Displays received packet counts for Art-Net and sACN (E1.31), with error status.
* **Automatic Brightness Limiter (ABL)**: Live calculation of current draw in Amperes and percentage of power supply cap.
* **Live Channel Value Preview**: Visual progress bars and numeric values for DMX channels (Master Dimmer/Strobe, RGBW, Preset Pattern, Audio Sync / Audio Mode).
* **Audio Reactive Sync Status**: Live BPM counter, RMS energy meter, and real-time blinking trigger indicators for Kick, Snare, and Hi-Hat.
* **Universe Subscriptions**: Status table showing active subscribed universes, protocol source (ArtNet / sACN), and stream age in milliseconds.

### 2. Rig Check / Self-Test View
Allows stage technicians to test LED strips directly from their phone/tablet without the lighting console transmitting:
* **RGBW Cycle**: Cycles Red, Green, Blue, White to verify color order and LED health.
* **Address Walker**: Walks a single white pixel across the strip to count LEDs and verify line continuity.
* **Power Soak 100%**: Sets all LEDs to 100% white to test 24V power supply and voltage sag across the line.
* **High-Rate Strobe**: 40Hz test strobe to verify SPI signal timing.
* **Custom Color Picker**: Interactive color wheel & palette (Warm White, Amber, Cyan, Magenta, etc.).
* **STOP / Return to DMX**: Instantly restores lighting desk control.

### 3. Configuration Management View
* Edit Node Name, Bind IP, Art-Net/sACN toggle.
* Configure Personality Mode (`preset_7ch`, `standard`, `pixel_direct`).
* Configure Color Order (`BRG`, `RGB`, `GRB`, `RBG`, `BGR`, `GBR`).
* Adjust Start Universe, Start Channel, and total Pixel Count.
* Configure PSU Max Current Limit (mA).
* Configure Failsafe Policy (`hold`, `hold_then_fade`, `blackout`).
* **Save & Apply Changes**: Atomically writes updated configuration back to disk (`config.toml`) and applies it dynamically to the live render loop.

### 4. Show Lock Protection
* Top-right lock button (`🔒 Locked` / `🔓 Unlocked`).
* When Show Lock is active, all configuration edits and manual rig-check test pattern overrides are locked out with clear warnings to prevent accidental disruptions during live concerts.

---

## 3. REST API Reference

All REST endpoints support Cross-Origin Resource Sharing (CORS) and standard JSON payloads.

### `GET /api/status`
Returns live telemetry snapshot.

**Example Response:**
```json
{
  "node_name": "Stage-WS2811-Node1",
  "version": "0.1.0",
  "uptime_sec": 3612,
  "show_lock": false,
  "fps_x10": 500,
  "frame_count": 180600,
  "artnet_rx": 72200,
  "sacn_rx": 0,
  "current_ma": 3400,
  "max_psu_ma": 15000,
  "failsafe_active": false,
  "test_mode": 0,
  "channels": [255, 255, 0, 128, 0, 5, 0, 0, 0, 0],
  "universes": [
    {
      "universe": 1,
      "source": "ArtNet",
      "healthy": true,
      "age_ms": 18
    }
  ],
  "audio": {
    "connected": true,
    "bpm": 128.5,
    "energy": 0.76,
    "kick": true,
    "snare": false,
    "hihat": true
  }
}
```

---

### `GET /api/config`
Returns active configuration structure as JSON.

```bash
curl http://192.168.1.50:8080/api/config
```

---

### `POST /api/config`
Updates node configuration, saves to TOML atomically on disk, and applies changes to the live rendering loop.

> **Status Codes:**
> * `200 OK`: Configuration saved and applied.
> * `400 Bad Request`: Invalid JSON payload.
> * `403 Forbidden`: Node is Show Locked.

```bash
curl -X POST http://192.168.1.50:8080/api/config \
  -H "Content-Type: application/json" \
  -d '{
    "node": { "name": "Stage-Left-WS2811", "show_lock": false },
    "network": { "bind_ip": "0.0.0.0", "artnet": true, "sacn": true },
    "failsafe": { "on_data_loss": "hold", "hold_timeout_sec": 10, "fade_duration_sec": 3 },
    "output": [{
      "id": "out1",
      "driver": "spi_ws281x",
      "device": "/dev/spidev0.0",
      "chip": "ws2811",
      "color_order": "brg",
      "pixels": 576,
      "psu_max_current_ma": 20000
    }],
    "fixture": [{
      "output": "out1",
      "pixel_range": [0, 575],
      "personality": "preset_7ch",
      "white_mode": "rgb_blend",
      "universe_span": "per_170px",
      "universe": 2,
      "address": 1
    }]
  }'
```

---

### `POST /api/test`
Overrides output with a standalone rig-check pattern or solid color.

```bash
# Start RGBW test cycle
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "rgbw"}'

# Set solid warm amber color
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "color", "color": [255, 140, 20]}'

# Return to live console DMX control
curl -X POST http://192.168.1.50:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"mode": "off"}'
```

---

### `POST /api/lock`
Toggles Show Lock.

```bash
# Lock node
curl -X POST http://192.168.1.50:8080/api/lock \
  -H "Content-Type: application/json" \
  -d '{"show_lock": true}'

# Unlock node
curl -X POST http://192.168.1.50:8080/api/lock \
  -H "Content-Type: application/json" \
  -d '{"show_lock": false}'
```
