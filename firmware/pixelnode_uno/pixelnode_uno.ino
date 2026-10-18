/**
 * @file pixelnode_uno.ino
 * @brief High-reliability PixelNode firmware for Arduino Uno (ATmega328P).
 *
 * Provides:
 * 1. High-speed USB-Serial streaming mode (OP_STREAM_PIXELS) from Linux/RasPi host.
 * 2. Onboard DMX lighting pattern generator (OP_DMX_CHANNELS) supporting all 24 autonomous
 *    lighting patterns (Ch 6) and 24 audio sync patterns (Ch 7).
 * 3. Fail-safe engine with AVR hardware watchdog (2s) and link timeout protection.
 *    - Mode HOLD: Freezes last valid frame during communication dropouts.
 *    - Mode AUTONOMOUS: Transitions to local ambient scene if host disconnects.
 *    - Mode BLACKOUT: Smoothly zeroes LED strip.
 * 4. Zero dynamic allocation (no malloc/free, no String), bounded single-buffer memory model (<60% SRAM).
 * 5. Power-on visual confidence check: flashes R->G->B->Amber on first 5 LEDs at startup
 *    to immediately verify strip power, ground, and Pin 6 data continuity before PC connects.
 *
 * Hardware:
 * - MCU: Arduino Uno (ATmega328P @ 16 MHz)
 * - LED Strip: WS2811 / WS2812B on Pin D6
 * - USB Baud: 115200 bps (default) or 250000 / 500000 bps
 */

#include "pixelnode_protocol.h"
#include "pixelnode_patterns.h"
#include "ws2812_driver.h"

#if defined(ARDUINO) && defined(__AVR__)
#include <avr/wdt.h>
#endif

// Default configuration
#define DEFAULT_PIXEL_COUNT 60
#define DEFAULT_TIMEOUT_MS  500
#define PIN_STATUS_LED      13

// Static parser and unified buffer model (saves 510 bytes SRAM)
static PxPacketParser parser;
static uint16_t num_pixels = DEFAULT_PIXEL_COUNT;
static uint8_t failsafe_policy = PX_FAILSAFE_HOLD;
static uint16_t link_timeout_ms = DEFAULT_TIMEOUT_MS;

// Unified single strip buffer (GRB / RGB bytes)
static uint8_t strip_buffer[PX_MAX_PIXELS * 3];

// DMX channel state for standalone pattern rendering
static struct {
    uint8_t master_dimmer;
    PxRgbColor base_color;
    uint8_t strobe;
    uint8_t pattern_ch6;
    uint8_t audio_ch7;
    uint8_t speed;
    uint8_t size;
    bool has_dmx_command;
} dmx_state = {
    255,             // dimmer
    {255, 120, 20},  // base color (warm amber default)
    0,               // strobe open
    0,               // pattern 0 (static)
    0,               // audio 0 (bypass)
    128,             // speed 1.0x
    128,             // size 1.0x
    false
};

static PxAudioFeatures audio_state = {0, 0, 0, 0, 0, 0, 0, 0, 0, 120};

static uint32_t last_packet_time = 0;
static uint32_t last_render_time = 0;
static bool in_failsafe = false;

static uint8_t strip_color_order = 0; // 0=GRB, 1=RGB, 2=RBG, 3=BRG, 4=BGR, 5=GBR

static void pack_render_to_strip(void) {
    if (strip_color_order == 1) {
        // RGB: no swap needed since internal render engine outputs RGB
        return;
    }
    for (uint16_t i = 0; i < num_pixels; i++) {
        uint16_t offset = i * 3;
        uint8_t r = strip_buffer[offset];
        uint8_t g = strip_buffer[offset + 1];
        uint8_t b = strip_buffer[offset + 2];
        switch (strip_color_order) {
            case 0: // GRB (WS2812B default)
                strip_buffer[offset]     = g;
                strip_buffer[offset + 1] = r;
                strip_buffer[offset + 2] = b;
                break;
            case 2: // RBG (Generic 12V strip)
                strip_buffer[offset]     = r;
                strip_buffer[offset + 1] = b;
                strip_buffer[offset + 2] = g;
                break;
            case 3: // BRG (WS2811 standard)
                strip_buffer[offset]     = b;
                strip_buffer[offset + 1] = r;
                strip_buffer[offset + 2] = g;
                break;
            case 4: // BGR
                strip_buffer[offset]     = b;
                strip_buffer[offset + 1] = g;
                strip_buffer[offset + 2] = r;
                break;
            case 5: // GBR
                strip_buffer[offset]     = g;
                strip_buffer[offset + 1] = b;
                strip_buffer[offset + 2] = r;
                break;
            default: // GRB fallback
                strip_buffer[offset]     = g;
                strip_buffer[offset + 1] = r;
                strip_buffer[offset + 2] = b;
                break;
        }
    }
}

static uint8_t tx_seq = 0;

static void send_status_reply(void) {
    uint8_t frame[14];
    frame[0] = PX_MAGIC_0;
    frame[1] = PX_MAGIC_1;
    frame[2] = PX_OP_STATUS_REPLY;
    frame[3] = tx_seq++;
    frame[4] = 7; // Len low
    frame[5] = 0; // Len high
    frame[6] = (uint8_t)(parser.packets_received & 0xFF);
    frame[7] = (uint8_t)((parser.packets_received >> 8) & 0xFF);
    frame[8] = (uint8_t)(parser.packets_crc_errors & 0xFF);
    frame[9] = (uint8_t)(num_pixels & 0xFF);
    frame[10] = (uint8_t)((num_pixels >> 8) & 0xFF);
    frame[11] = dmx_state.pattern_ch6;
    frame[12] = in_failsafe ? 1 : 0;
    frame[13] = px_calculate_crc8(&frame[2], 11);
    Serial.write(frame, 14);
}

static void handle_packet(const PxPacketParser* p) {
    last_packet_time = millis();
    in_failsafe = false;

    switch (p->opcode) {
        case PX_OP_STREAM_PIXELS: {
            uint16_t copy_len = p->expected_len;
            if (copy_len > PX_MAX_PIXELS * 3) {
                copy_len = PX_MAX_PIXELS * 3;
            }
            // Auto-detect and adapt num_pixels if stream is larger than default
            uint16_t stream_px = copy_len / 3;
            if (stream_px > num_pixels) {
                num_pixels = stream_px;
            }
            for (uint16_t i = 0; i < copy_len; i++) {
                strip_buffer[i] = p->payload[i];
            }
            ws2812_show(strip_buffer, copy_len);
            break;
        }

        case PX_OP_DMX_CHANNELS: {
            if (p->expected_len >= 9) {
                dmx_state.master_dimmer = p->payload[0];
                dmx_state.base_color.r  = p->payload[1];
                dmx_state.base_color.g  = p->payload[2];
                dmx_state.base_color.b  = p->payload[3];
                dmx_state.strobe        = p->payload[4];
                dmx_state.pattern_ch6   = p->payload[5];
                dmx_state.audio_ch7     = p->payload[6];
                dmx_state.speed         = p->payload[7];
                dmx_state.size          = p->payload[8];
                dmx_state.has_dmx_command = true;
            }
            break;
        }

        case PX_OP_AUDIO_FEATURES: {
            if (p->expected_len >= 6) {
                audio_state.energy     = p->payload[0];
                audio_state.bass       = p->payload[1];
                audio_state.mid        = p->payload[2];
                audio_state.treble     = p->payload[3];
                audio_state.bpm        = p->payload[4];
                audio_state.is_kick    = (p->payload[5] & 0x01) ? 1 : 0;
                audio_state.is_snare   = (p->payload[5] & 0x02) ? 1 : 0;
                audio_state.is_hihat   = (p->payload[5] & 0x04) ? 1 : 0;
            }
            break;
        }

        case PX_OP_CONFIG: {
            if (p->expected_len >= 5) {
                uint16_t px = p->payload[0] | ((uint16_t)p->payload[1] << 8);
                if (px > 0 && px <= PX_MAX_PIXELS) {
                    num_pixels = px;
                }
                failsafe_policy = p->payload[2];
                uint16_t to = p->payload[3] | ((uint16_t)p->payload[4] << 8);
                if (to >= 50) {
                    link_timeout_ms = to;
                }
                if (p->expected_len >= 6) {
                    strip_color_order = p->payload[5];
                }
                send_status_reply(); // Immediately acknowledge config update
            }
            break;
        }

        case PX_OP_HEARTBEAT:
            send_status_reply();
            break;

        default:
            break;
    }
}

void setup() {
    // 1. Initialize Serial communication immediately to capture host handshake
    Serial.begin(115200);

#if defined(ARDUINO) && defined(__AVR__)
    // Enable AVR Hardware Watchdog Timer: 2.0 second timeout
    wdt_enable(WDTO_2S);
#endif

    pinMode(PIN_STATUS_LED, OUTPUT);
    digitalWrite(PIN_STATUS_LED, LOW);

    ws2812_init();

    // Initialize packet parser
    px_parser_init(&parser);

    // Send ASCII readiness banner to host
    Serial.println(F("PX_READY"));

    // -------------------------------------------------------------------------
    // Power-on Visual Confidence Check (400ms sequence):
    // Confirms physical LED strip power, GND, and Pin D6 continuity on boot.
    // Flashes first 5 LEDs: Red -> Green -> Blue -> Warm Amber -> Off.
    // -------------------------------------------------------------------------
    for (uint8_t phase = 0; phase < 4; phase++) {
#if defined(ARDUINO) && defined(__AVR__)
        wdt_reset();
#endif
        for (uint16_t i = 0; i < num_pixels * 3; i++) {
            strip_buffer[i] = 0;
        }
        uint16_t test_px = (num_pixels < 5) ? num_pixels : 5;
        for (uint16_t i = 0; i < test_px; i++) {
            uint16_t idx = i * 3;
            if (phase == 0) {
                // Red (GRB order: G=0, R=80, B=0)
                strip_buffer[idx] = 0; strip_buffer[idx + 1] = 80; strip_buffer[idx + 2] = 0;
            } else if (phase == 1) {
                // Green (GRB order: G=80, R=0, B=0)
                strip_buffer[idx] = 80; strip_buffer[idx + 1] = 0; strip_buffer[idx + 2] = 0;
            } else if (phase == 2) {
                // Blue (GRB order: G=0, R=0, B=80)
                strip_buffer[idx] = 0; strip_buffer[idx + 1] = 0; strip_buffer[idx + 2] = 80;
            } else {
                // Soft amber (warm standby)
                strip_buffer[idx] = 20; strip_buffer[idx + 1] = 60; strip_buffer[idx + 2] = 5;
            }
        }
        ws2812_show(strip_buffer, num_pixels * 3);
        delay(100);
    }

    // Clear strip to black
    for (uint16_t i = 0; i < sizeof(strip_buffer); i++) {
        strip_buffer[i] = 0;
    }
    ws2812_show(strip_buffer, num_pixels * 3);

    last_packet_time = millis();
    last_render_time = millis();
}

void loop() {
#if defined(ARDUINO) && defined(__AVR__)
    // Pet the hardware watchdog
    wdt_reset();
#endif

    // 1. Process incoming serial data stream
    while (Serial.available() > 0) {
        uint8_t byte = (uint8_t)Serial.read();
        if (px_parser_feed_byte(&parser, byte)) {
            handle_packet(&parser);
        }
    }

    uint32_t now = millis();
    float t_sec = (float)now * 0.001f;

    // 2. Link Timeout & Fail-safe Handling
    bool link_timed_out = (now - last_packet_time) > link_timeout_ms;
    if (link_timed_out) {
        if (!in_failsafe) {
            in_failsafe = true;
            if (failsafe_policy == PX_FAILSAFE_BLACKOUT) {
                for (uint16_t i = 0; i < num_pixels * 3; i++) {
                    strip_buffer[i] = 0;
                }
                ws2812_show(strip_buffer, num_pixels * 3);
            }
        }

        if (failsafe_policy == PX_FAILSAFE_AUTONOMOUS) {
            // Render smooth fallback breath throttled to 30 FPS (every 33ms)
            if (now - last_render_time >= 33) {
                last_render_time = now;
                PxRgbColor warm_amber = {255, 120, 20};
                px_render_pattern(8 * 8, warm_amber, 128, 128, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
                pack_render_to_strip();
                ws2812_show(strip_buffer, num_pixels * 3);
            }
        }
    }

    // 3. Autonomous DMX Pattern Rendering (active when dmx_command is received and not blacked out)
    if (dmx_state.has_dmx_command) {
        bool is_blackout = in_failsafe && (failsafe_policy == PX_FAILSAFE_BLACKOUT);
        if (!is_blackout && (now - last_render_time >= 20)) {
            last_render_time = now;
            uint8_t audio_slot = dmx_state.audio_ch7 / 8;
            if (audio_slot > 0) {
                px_render_audio_pattern(dmx_state.audio_ch7, &audio_state, dmx_state.base_color, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            } else {
                px_render_pattern(dmx_state.pattern_ch6, dmx_state.base_color, dmx_state.speed, dmx_state.size, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            }

            px_apply_dimmer_and_strobe(dmx_state.master_dimmer, dmx_state.strobe, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            pack_render_to_strip();
            ws2812_show(strip_buffer, num_pixels * 3);
        }
    }

    // 4. Periodic telemetry status reply (1 Hz) to notify host of MCU health & packet count
    static uint32_t last_status_tx = 0;
    if (now - last_status_tx >= 1000) {
        last_status_tx = now;
        send_status_reply();
    }

    // 5. Status LED indication
    if (in_failsafe) {
        digitalWrite(PIN_STATUS_LED, (now / 250) % 2 == 0 ? HIGH : LOW); // Fast blink warning
    } else {
        digitalWrite(PIN_STATUS_LED, (now / 1000) % 2 == 0 ? HIGH : LOW); // Heartbeat normal
    }
}
