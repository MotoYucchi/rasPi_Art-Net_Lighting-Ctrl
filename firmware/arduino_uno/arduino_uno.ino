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
#include <avr/io.h>

// Capture hardware reset flags before C runtime clears MCUSR
uint8_t mcusr_mirror __attribute__((section(".noinit")));
void get_mcusr(void) __attribute__((naked)) __attribute__((section(".init3")));
void get_mcusr(void) {
    mcusr_mirror = MCUSR;
    MCUSR = 0;
    wdt_disable();
}
#else
static uint8_t mcusr_mirror = 0;
#endif

// Default configuration
#define DEFAULT_PIXEL_COUNT 60
#define DEFAULT_TIMEOUT_MS  1500
#define PIN_STATUS_LED      13

// Static parser and unified buffer model (saves 510 bytes SRAM)
static PxPacketParser parser;
static uint16_t num_pixels = DEFAULT_PIXEL_COUNT;
static uint8_t failsafe_policy = PX_FAILSAFE_HOLD;
static uint16_t link_timeout_ms = DEFAULT_TIMEOUT_MS;

// Stream frame synchronization: decoupled from RX to eliminate cli() UART FIFO overruns
static bool stream_frame_dirty = false;
static bool dmx_frame_dirty = false;
static uint16_t stream_frame_len = 0;
static uint32_t last_rx_byte_us = 0;

// Hardware Automatic Brightness / Current Limiter (ABL) to prevent 5V rail brownout (< 4.3V),
// PTC polyfuse/regulator overheating, and USB CH340 disconnects (os error 5) when driving 100..300 LEDs.
// Caps total RGB channel sum to 4200 (~260mA strip current, keeping total USB draw < 300mA continuous).
#define PX_MAX_SAFE_RGB_SUM 4200UL

// Unified single strip buffer (GRB / RGB bytes)
static uint8_t strip_buffer[PX_MAX_PIXELS * 3];

static void apply_hardware_abl(uint8_t* buf, uint16_t byte_len) {
    uint32_t total_sum = 0;
    for (uint16_t i = 0; i < byte_len; i++) {
        total_sum += buf[i];
    }
    if (total_sum > PX_MAX_SAFE_RGB_SUM) {
        uint16_t scale = (uint16_t)((PX_MAX_SAFE_RGB_SUM * 256UL) / total_sum);
        for (uint16_t i = 0; i < byte_len; i++) {
            buf[i] = (uint8_t)(((uint16_t)buf[i] * scale) >> 8);
        }
    }
}

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
    160,             // speed 1.0x forward (128 is stop)
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
            if (p->payload != strip_buffer) {
                for (uint16_t i = 0; i < copy_len; i++) {
                    strip_buffer[i] = p->payload[i];
                }
            }
            // Deactivate autonomous DMX to prevent dual-render collision
            dmx_state.has_dmx_command = false;
            // Mark new frame ready for display in loop() when UART is quiet
            stream_frame_dirty = true;
            stream_frame_len = copy_len;
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
                dmx_frame_dirty = true;
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
                dmx_frame_dirty = true;
            }
            break;
        }

        case PX_OP_CONFIG: {
            if (p->expected_len >= 5) {
                uint16_t px = p->payload[0] | ((uint16_t)p->payload[1] << 8);
                if (px > 0 && px <= PX_MAX_PIXELS) {
                    if (px != num_pixels) {
                        for (uint16_t i = 0; i < sizeof(strip_buffer); i++) {
                            strip_buffer[i] = 0;
                        }
                    }
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
                dmx_frame_dirty = true;
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

    // Initialize packet parser with unified single strip buffer (Zero duplicate RAM overhead)
    px_parser_init_with_buffer(&parser, strip_buffer, sizeof(strip_buffer));

    // Note: Unsolicited Serial.print on boot is intentionally omitted to keep USB Bulk-IN
    // completely silent during normal unidirectional operation (prevents CH340 USB/IP -EPIPE stalls).

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

static void drain_serial_before_show(void) {
    uint32_t wait_start_us = micros();
    while (true) {
        while (Serial.available() > 0) {
            uint8_t byte = (uint8_t)Serial.read();
            last_rx_byte_us = micros();
            if (px_parser_feed_byte(&parser, byte)) {
                handle_packet(&parser);
            }
        }
        // If a control packet is mid-flight on the wire, wait up to 2.5ms for it to finish
        // before ws2812_show() disables interrupts via cli().
        if (parser.state == PX_STATE_WAIT_MAGIC0 || (micros() - wait_start_us) > 2500UL) {
            break;
        }
    }
}

void loop() {
#if defined(ARDUINO) && defined(__AVR__)
    // Pet the hardware watchdog
    wdt_reset();
#endif

    // 1. Process incoming serial data stream
    while (Serial.available() > 0) {
        uint8_t byte = (uint8_t)Serial.read();
        last_rx_byte_us = micros();
        if (px_parser_feed_byte(&parser, byte)) {
            handle_packet(&parser);
        }
    }

    uint32_t quiet_us = micros() - last_rx_byte_us;
    if (parser.state != PX_STATE_WAIT_MAGIC0 && quiet_us > 15000UL) {
        // Auto-recover if a partial packet was truncated on the wire
        parser.state = PX_STATE_WAIT_MAGIC0;
    }

    // UART is completely idle only when no packet is mid-parse and line has been quiet >= 200us
    bool uart_idle = (parser.state == PX_STATE_WAIT_MAGIC0) && (Serial.available() == 0) && (quiet_us >= 200UL);

    uint32_t now = millis();
    float t_sec = (float)now * 0.001f;

    // 2. Render streamed pixel frame ONLY during confirmed UART quiet window
    // (uart_idle guarantees ws2812_show()'s cli() will never cause UART RX FIFO overruns)
    if (stream_frame_dirty && uart_idle) {
        stream_frame_dirty = false;
        apply_hardware_abl(strip_buffer, stream_frame_len);
        drain_serial_before_show();
        ws2812_show(strip_buffer, stream_frame_len);
    }

    // 3. Link Timeout & Fail-safe Handling
    bool link_timed_out = (now - last_packet_time) > link_timeout_ms;
    if (link_timed_out && uart_idle) {
        if (!in_failsafe) {
            in_failsafe = true;
            if (failsafe_policy == PX_FAILSAFE_BLACKOUT) {
                for (uint16_t i = 0; i < num_pixels * 3; i++) {
                    strip_buffer[i] = 0;
                }
                drain_serial_before_show();
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
                apply_hardware_abl(strip_buffer, num_pixels * 3);
                drain_serial_before_show();
                ws2812_show(strip_buffer, num_pixels * 3);
            }
        }
    }

    // 4. Autonomous DMX Pattern Rendering (active when dmx_command is received and not blacked out)
    if (dmx_state.has_dmx_command && uart_idle) {
        bool is_blackout = in_failsafe && (failsafe_policy == PX_FAILSAFE_BLACKOUT);
        bool is_static = (dmx_state.pattern_ch6 < 8) && (dmx_state.strobe == 0) && (dmx_state.audio_ch7 < 8);
        uint32_t render_interval_ms = is_static ? 250 : 30;
        if (!is_blackout && (dmx_frame_dirty || (now - last_render_time >= render_interval_ms))) {
            dmx_frame_dirty = false;
            last_render_time = now;
            uint8_t audio_slot = dmx_state.audio_ch7 / 8;
            if (audio_slot > 0) {
                px_render_audio_pattern(dmx_state.audio_ch7, &audio_state, dmx_state.base_color, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            } else {
                px_render_pattern(dmx_state.pattern_ch6, dmx_state.base_color, dmx_state.speed, dmx_state.size, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            }

            px_apply_dimmer_and_strobe(dmx_state.master_dimmer, dmx_state.strobe, t_sec, (PxRgbColor*)strip_buffer, num_pixels);
            pack_render_to_strip();
            apply_hardware_abl(strip_buffer, num_pixels * 3);
            drain_serial_before_show();
            ws2812_show(strip_buffer, num_pixels * 3);
        }
    }

    // Note: Unsolicited periodic send_status_reply() in loop() has been removed so that
    // normal operation is 100% unidirectional (Host -> Arduino), eliminating CH340 Bulk-IN
    // collisions and WSL2 vhci_hcd -EPIPE resets. Status replies are sent only on OP_HEARTBEAT.

    // 5. Status LED indication
    if (in_failsafe) {
        digitalWrite(PIN_STATUS_LED, (now / 250) % 2 == 0 ? HIGH : LOW); // Fast blink warning
    } else {
        digitalWrite(PIN_STATUS_LED, (now / 1000) % 2 == 0 ? HIGH : LOW); // Heartbeat normal
    }
}
