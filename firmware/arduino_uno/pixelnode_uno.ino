/**
 * @file pixelnode_uno.ino
 * @brief High-reliability PixelNode firmware for Arduino Uno (ATmega328P).
 *
 * Provides:
 * 1. High-speed USB-Serial streaming mode (OP_STREAM_PIXELS) from Linux/RasPi host.
 * 2. Onboard DMX lighting pattern generator (OP_DMX_CHANNELS) supporting all 24 autonomous
 *    lighting patterns (Ch 6) and 24 audio sync patterns (Ch 7).
 * 3. Professional fail-safe engine with AVR hardware watchdog (2s) and link timeout protection.
 *    - Mode HOLD: Freezes last valid frame during communication dropouts.
 *    - Mode AUTONOMOUS: Transitions to local ambient scene if host disconnects.
 *    - Mode BLACKOUT: Smoothly zeroes LED strip.
 * 4. Zero dynamic allocation (no malloc/free, no String), bounded ring buffer.
 *
 * Hardware:
 * - MCU: Arduino Uno (ATmega328P @ 16 MHz)
 * - LED Strip: WS2811 / WS2812B on Pin D6
 * - USB Baud: 115200 bps (default) or 250000 bps
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

// Static buffers (Zero allocation on ATmega328P)
static PxPacketParser parser;
static uint16_t num_pixels = DEFAULT_PIXEL_COUNT;
static uint8_t failsafe_policy = PX_FAILSAFE_HOLD;
static uint16_t link_timeout_ms = DEFAULT_TIMEOUT_MS;

// Output buffers (GRB/RGB packed bytes)
static uint8_t strip_buffer[PX_MAX_PIXELS * 3];
static PxRgbColor render_buffer[PX_MAX_PIXELS];

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
static bool in_failsafe = false;

static void pack_render_to_strip(void) {
    // Pack into WS2812 standard GRB order
    for (uint16_t i = 0; i < num_pixels; i++) {
        uint16_t offset = i * 3;
        strip_buffer[offset]     = render_buffer[i].g;
        strip_buffer[offset + 1] = render_buffer[i].r;
        strip_buffer[offset + 2] = render_buffer[i].b;
    }
}

static void handle_packet(const PxPacketParser* p) {
    last_packet_time = millis();
    in_failsafe = false;

    switch (p->opcode) {
        case PX_OP_STREAM_PIXELS: {
            uint16_t copy_len = p->expected_len;
            if (copy_len > num_pixels * 3) {
                copy_len = num_pixels * 3;
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
            }
            break;
        }

        case PX_OP_HEARTBEAT:
        default:
            break;
    }
}

void setup() {
#if defined(ARDUINO) && defined(__AVR__)
    // Enable AVR Hardware Watchdog Timer: 2.0 second timeout
    wdt_enable(WDTO_2S);
#endif

    pinMode(PIN_STATUS_LED, OUTPUT);
    digitalWrite(PIN_STATUS_LED, LOW);

    ws2812_init();

    // Clear strip buffer
    for (uint16_t i = 0; i < sizeof(strip_buffer); i++) {
        strip_buffer[i] = 0;
    }
    ws2812_show(strip_buffer, num_pixels * 3);

    // Initialize packet parser
    px_parser_init(&parser);

    // Serial port initialization (115200 bps standard)
    Serial.begin(115200);

    last_packet_time = millis();
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
    if ((now - last_packet_time) > link_timeout_ms) {
        in_failsafe = true;

        if (failsafe_policy == PX_FAILSAFE_HOLD) {
            // Keep refreshing the last valid frame (prevents LED signal degradation)
            ws2812_show(strip_buffer, num_pixels * 3);
        } else if (failsafe_policy == PX_FAILSAFE_AUTONOMOUS) {
            // Render safe fallback ambient warm breath
            PxRgbColor warm_amber = {255, 120, 20};
            px_render_pattern(8 * 8, warm_amber, 128, 128, t_sec, render_buffer, num_pixels);
            pack_render_to_strip();
            ws2812_show(strip_buffer, num_pixels * 3);
        } else {
            // Blackout
            for (uint16_t i = 0; i < num_pixels * 3; i++) {
                strip_buffer[i] = 0;
            }
            ws2812_show(strip_buffer, num_pixels * 3);
        }
    } else if (dmx_state.has_dmx_command) {
        // 3. Render DMX command pattern if receiving OP_DMX_CHANNELS
        uint8_t audio_slot = dmx_state.audio_ch7 / 8;
        if (audio_slot > 0) {
            px_render_audio_pattern(dmx_state.audio_ch7, &audio_state, dmx_state.base_color, t_sec, render_buffer, num_pixels);
        } else {
            px_render_pattern(dmx_state.pattern_ch6, dmx_state.base_color, dmx_state.speed, dmx_state.size, t_sec, render_buffer, num_pixels);
        }

        px_apply_dimmer_and_strobe(dmx_state.master_dimmer, dmx_state.strobe, t_sec, render_buffer, num_pixels);
        pack_render_to_strip();
        ws2812_show(strip_buffer, num_pixels * 3);
    }

    // 4. Status LED indication
    if (in_failsafe) {
        digitalWrite(PIN_STATUS_LED, (now / 250) % 2 == 0 ? HIGH : LOW); // Fast blink warning
    } else {
        digitalWrite(PIN_STATUS_LED, (now / 1000) % 2 == 0 ? HIGH : LOW); // Heartbeat normal
    }
}
