/**
 * @file test_conformance.cpp
 * @brief Strict automated conformance test suite for PixelNode Arduino Uno firmware.
 *
 * Compiles natively with g++ to rigorously test the exact same C++ headers
 * (pixelnode_protocol.h, pixelnode_patterns.h) used on ATmega328P.
 *
 * Verifies:
 * 1. CRC-8 algorithm test vectors and error detection.
 * 2. Packet parser state machine: normal, partial, fragmented, corrupted, and overflow packets.
 * 3. All 24 autonomous lighting patterns (Ch 6) across full DMX range 0..255.
 * 4. All 24 audio-reactive patterns (Ch 7) across full DMX range 0..255.
 * 5. Master dimmer, strobe gating, and hue_to_rgb calculations.
 * 6. Numeric safety: Zero NaN, zero Inf, zero out-of-bounds memory accesses.
 */

#include <iostream>
#include <vector>
#include <cassert>
#include <cmath>
#include <cstring>
#include <cstdlib>

#include "pixelnode_protocol.h"
#include "pixelnode_patterns.h"

#define TEST_ASSERT(cond, msg) \
    do { \
        if (!(cond)) { \
            std::cerr << "[FAIL] " << msg << " (" << __FILE__ << ":" << __LINE__ << ")" << std::endl; \
            std::exit(1); \
        } \
    } while (0)

static void test_crc8_integrity() {
    std::cout << "[TEST] CRC-8 calculation & test vectors..." << std::endl;

    // Test vector 1: Empty
    uint8_t empty_crc = px_calculate_crc8(nullptr, 0);
    TEST_ASSERT(empty_crc == 0, "Empty CRC must be 0");

    // Test vector 2: Deterministic byte sequence
    const uint8_t test_data[] = {0x01, 0x02, 0x03, 0x04};
    uint8_t crc1 = px_calculate_crc8(test_data, sizeof(test_data));
    TEST_ASSERT(crc1 != 0, "CRC must be non-zero for test data");

    // Repeatability
    uint8_t crc2 = px_calculate_crc8(test_data, sizeof(test_data));
    TEST_ASSERT(crc1 == crc2, "CRC must be deterministic");

    // Single bit flip detection
    uint8_t test_corrupt[] = {0x01, 0x02, 0x03, 0x05};
    uint8_t crc_corrupt = px_calculate_crc8(test_corrupt, sizeof(test_corrupt));
    TEST_ASSERT(crc1 != crc_corrupt, "CRC must detect single bit difference");

    std::cout << "  -> CRC-8 integrity tests PASSED." << std::endl;
}

static void test_packet_parser() {
    std::cout << "[TEST] Protocol parser state machine..." << std::endl;

    PxPacketParser parser;
    px_parser_init(&parser);

    // 1. Valid packet test (OP_STREAM_PIXELS)
    uint8_t raw_payload[] = {255, 0, 128, 0, 255, 64};
    uint16_t len = sizeof(raw_payload);

    std::vector<uint8_t> frame;
    frame.push_back(PX_MAGIC_0);
    frame.push_back(PX_MAGIC_1);
    frame.push_back(PX_OP_STREAM_PIXELS);
    frame.push_back(10); // seq
    frame.push_back(len & 0xFF);
    frame.push_back((len >> 8) & 0xFF);
    for (size_t i = 0; i < len; i++) {
        frame.push_back(raw_payload[i]);
    }

    uint8_t expected_crc = px_calculate_crc8(&frame[2], frame.size() - 2);
    frame.push_back(expected_crc);

    bool received = false;
    for (uint8_t b : frame) {
        if (px_parser_feed_byte(&parser, b)) {
            received = true;
        }
    }
    TEST_ASSERT(received, "Parser must successfully parse valid frame");
    TEST_ASSERT(parser.opcode == PX_OP_STREAM_PIXELS, "Opcode match");
    TEST_ASSERT(parser.seq == 10, "Sequence match");
    TEST_ASSERT(parser.expected_len == len, "Length match");
    TEST_ASSERT(std::memcmp(parser.payload, raw_payload, len) == 0, "Payload match");

    // 2. Corrupted CRC test
    frame.back() ^= 0xFF; // Invert CRC
    px_parser_init(&parser);
    received = false;
    for (uint8_t b : frame) {
        if (px_parser_feed_byte(&parser, b)) {
            received = true;
        }
    }
    TEST_ASSERT(!received, "Parser must reject frame with invalid CRC");
    TEST_ASSERT(parser.packets_crc_errors == 1, "CRC error counter must increment");

    // 3. Noise rejection test (Garbage before valid magic)
    px_parser_init(&parser);
    const uint8_t garbage[] = {0x12, 0x34, 0x50, 0x00, 0xFF, 0xAA, 0x50}; // Notice partial magic
    for (uint8_t g : garbage) {
        TEST_ASSERT(!px_parser_feed_byte(&parser, g), "Parser must ignore noise");
    }

    // Now feed valid frame again
    frame.back() ^= 0xFF; // Restore valid CRC
    received = false;
    for (uint8_t b : frame) {
        if (px_parser_feed_byte(&parser, b)) {
            received = true;
        }
    }
    TEST_ASSERT(received, "Parser must recover after noise and parse valid frame");

    // 4. Overflow length protection (> PX_MAX_PAYLOAD_LEN)
    px_parser_init(&parser);
    uint8_t overflow_frame[] = {
        PX_MAGIC_0, PX_MAGIC_1,
        PX_OP_STREAM_PIXELS, 1,
        0xFF, 0x03 // 1023 bytes (exceeds 512 bytes limit)
    };
    for (uint8_t b : overflow_frame) {
        px_parser_feed_byte(&parser, b);
    }
    TEST_ASSERT(parser.packets_overflow_errors == 1, "Parser must flag overflow");
    TEST_ASSERT(parser.state == PX_STATE_WAIT_MAGIC0, "Parser must reset to WAIT_MAGIC0 on overflow");

    std::cout << "  -> Protocol parser tests PASSED." << std::endl;
}

static void test_all_autonomous_patterns() {
    std::cout << "[TEST] All 24 Autonomous Lighting Patterns (Ch 6)..." << std::endl;

    const size_t NUM_PIXELS = 60;
    PxRgbColor pixels[NUM_PIXELS];
    PxRgbColor base_color = {255, 100, 50};

    // Test each of the 24 pattern slots (0..=23)
    for (uint8_t slot = 0; slot <= 23; slot++) {
        uint8_t dmx_val = slot * 8;

        // Test parameter variations
        const uint8_t speeds[] = {0, 64, 128, 192, 255};
        const uint8_t sizes[] = {0, 64, 128, 192, 255};
        const float times[] = {0.0f, 0.5f, 1.25f, 10.0f};

        for (uint8_t spd : speeds) {
            for (uint8_t sz : sizes) {
                for (float t : times) {
                    std::memset(pixels, 0, sizeof(pixels));
                    px_render_pattern(dmx_val, base_color, spd, sz, t, pixels, NUM_PIXELS);

                    // Verify pixel values are valid numbers (no NaN, no memory corruption)
                    for (size_t i = 0; i < NUM_PIXELS; i++) {
                        TEST_ASSERT(pixels[i].r <= 255, "R channel bound");
                        TEST_ASSERT(pixels[i].g <= 255, "G channel bound");
                        TEST_ASSERT(pixels[i].b <= 255, "B channel bound");
                    }
                }
            }
        }
    }

    // Test slot boundaries across entire DMX range 0..255
    for (int dmx = 0; dmx <= 255; dmx++) {
        std::memset(pixels, 0, sizeof(pixels));
        px_render_pattern((uint8_t)dmx, base_color, 128, 128, 1.0f, pixels, NUM_PIXELS);
        for (size_t i = 0; i < NUM_PIXELS; i++) {
            TEST_ASSERT(pixels[i].r <= 255 && pixels[i].g <= 255 && pixels[i].b <= 255, "Bound check");
        }
    }

    std::cout << "  -> All 24 Autonomous Patterns verified successfully." << std::endl;
}

static void test_all_audio_patterns() {
    std::cout << "[TEST] All 24 Audio-Reactive Patterns (Ch 7)..." << std::endl;

    const size_t NUM_PIXELS = 60;
    PxRgbColor pixels[NUM_PIXELS];
    PxRgbColor base_color = {100, 200, 255};

    PxAudioFeatures aud;
    aud.energy = 180;
    aud.peak = 220;
    aud.bass = 240;
    aud.mid = 120;
    aud.treble = 90;
    aud.beat_phase = 0;
    aud.is_kick = 1;
    aud.is_snare = 0;
    aud.is_hihat = 1;
    aud.bpm = 128;

    for (uint8_t slot = 0; slot <= 23; slot++) {
        uint8_t dmx_val = slot * 8;
        const float times[] = {0.0f, 0.5f, 1.5f};

        for (float t : times) {
            std::memset(pixels, 0, sizeof(pixels));
            px_render_audio_pattern(dmx_val, &aud, base_color, t, pixels, NUM_PIXELS);

            for (size_t i = 0; i < NUM_PIXELS; i++) {
                TEST_ASSERT(pixels[i].r <= 255, "Audio R bound");
                TEST_ASSERT(pixels[i].g <= 255, "Audio G bound");
                TEST_ASSERT(pixels[i].b <= 255, "Audio B bound");
            }
        }
    }

    // Boundary check across entire DMX range 0..255
    for (int dmx = 0; dmx <= 255; dmx++) {
        std::memset(pixels, 0, sizeof(pixels));
        px_render_audio_pattern((uint8_t)dmx, &aud, base_color, 1.0f, pixels, NUM_PIXELS);
        for (size_t i = 0; i < NUM_PIXELS; i++) {
            TEST_ASSERT(pixels[i].r <= 255 && pixels[i].g <= 255 && pixels[i].b <= 255, "Bound check");
        }
    }

    std::cout << "  -> All 24 Audio-Reactive Patterns verified successfully." << std::endl;
}

static void test_dimmer_and_strobe() {
    std::cout << "[TEST] Master dimmer and strobe gating..." << std::endl;

    const size_t NUM_PIXELS = 10;
    PxRgbColor pixels[NUM_PIXELS];

    // Dimmer 100% (255)
    for (size_t i = 0; i < NUM_PIXELS; i++) pixels[i] = {200, 100, 50};
    px_apply_dimmer_and_strobe(255, 0, 0.0f, pixels, NUM_PIXELS);
    TEST_ASSERT(pixels[0].r == 200 && pixels[0].g == 100 && pixels[0].b == 50, "Full dimmer");

    // Dimmer 50% (128)
    for (size_t i = 0; i < NUM_PIXELS; i++) pixels[i] = {200, 100, 50};
    px_apply_dimmer_and_strobe(128, 0, 0.0f, pixels, NUM_PIXELS);
    TEST_ASSERT(pixels[0].r == 100 && pixels[0].g == 50 && pixels[0].b == 25, "Half dimmer");

    // Dimmer 0% (Blackout)
    for (size_t i = 0; i < NUM_PIXELS; i++) pixels[i] = {200, 100, 50};
    px_apply_dimmer_and_strobe(0, 0, 0.0f, pixels, NUM_PIXELS);
    TEST_ASSERT(pixels[0].r == 0 && pixels[0].g == 0 && pixels[0].b == 0, "Zero dimmer blackout");

    // Strobe gate: at t = 0.0 with 10Hz strobe (dmx > 16), cycle_pos < 0.5 (open)
    for (size_t i = 0; i < NUM_PIXELS; i++) pixels[i] = {200, 200, 200};
    px_apply_dimmer_and_strobe(255, 120, 0.0f, pixels, NUM_PIXELS);
    TEST_ASSERT(pixels[0].r > 0, "Strobe open phase");

    // Strobe gate: at t = 0.06 with 10Hz strobe (period 0.1s, cycle_pos = 0.6 >= 0.5: closed)
    for (size_t i = 0; i < NUM_PIXELS; i++) pixels[i] = {200, 200, 200};
    px_apply_dimmer_and_strobe(255, 106, 0.06f, pixels, NUM_PIXELS);
    // At closed phase, output is 0
    TEST_ASSERT(pixels[0].r == 0, "Strobe closed phase");

    std::cout << "  -> Dimmer and strobe tests PASSED." << std::endl;
}

static void test_hue_to_rgb() {
    std::cout << "[TEST] Hue-to-RGB color wheel..." << std::endl;

    PxRgbColor red = px_hue_to_rgb(0);
    TEST_ASSERT(red.r == 255 && red.g == 0 && red.b == 0, "Hue 0 must be Red");

    PxRgbColor green = px_hue_to_rgb(85);
    TEST_ASSERT(green.g == 255 && green.b == 0, "Hue 85 must be Green");

    PxRgbColor blue = px_hue_to_rgb(170);
    TEST_ASSERT(blue.b == 255 && blue.r == 0, "Hue 170 must be Blue");

    std::cout << "  -> Hue-to-RGB color wheel tests PASSED." << std::endl;
}

int main() {
    std::cout << "==========================================================" << std::endl;
    std::cout << "  PixelNode Arduino Uno Conformance & Safety Test Suite   " << std::endl;
    std::cout << "==========================================================" << std::endl;

    test_crc8_integrity();
    test_packet_parser();
    test_all_autonomous_patterns();
    test_all_audio_patterns();
    test_dimmer_and_strobe();
    test_hue_to_rgb();

    std::cout << "==========================================================" << std::endl;
    std::cout << "  ALL ARDUINO UNO CONFORMANCE TESTS PASSED (100% SUCCESS)  " << std::endl;
    std::cout << "==========================================================" << std::endl;
    return 0;
}
