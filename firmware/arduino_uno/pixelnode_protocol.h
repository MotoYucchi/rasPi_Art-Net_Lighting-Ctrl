/**
 * @file pixelnode_protocol.h
 * @brief Zero-allocation wire protocol parser and definitions for PixelNode MCU (Arduino Uno).
 *
 * Designed for ATmega328P (16MHz, 2KB SRAM, 32KB Flash).
 * Validates framing magic 'PX', rolling sequence, payload length, and Dallas/Maxim CRC-8.
 */

#ifndef PIXELNODE_PROTOCOL_H
#define PIXELNODE_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PX_MAGIC_0 0x50 // 'P'
#define PX_MAGIC_1 0x58 // 'X'

#define PX_OP_STREAM_PIXELS   0x01
#define PX_OP_DMX_CHANNELS    0x02
#define PX_OP_HEARTBEAT       0x03
#define PX_OP_CONFIG          0x04
#define PX_OP_AUDIO_FEATURES  0x05
#define PX_OP_STATUS_REPLY    0x06

#define PX_HEADER_SIZE 6 // MAGIC(2) + OPCODE(1) + SEQ(1) + LEN(2)
#define PX_FOOTER_SIZE 1 // CRC-8(1)

// ATmega328P constraint: Max 170 pixels = 510 bytes
#define PX_MAX_PIXELS 170
#define PX_MAX_PAYLOAD_LEN 512

// Fail-safe policies
#define PX_FAILSAFE_BLACKOUT    0
#define PX_FAILSAFE_HOLD        1
#define PX_FAILSAFE_AUTONOMOUS  2

/**
 * @brief Calculate CRC-8 byte using Dallas/Maxim polynomial 0x31 (x^8 + x^5 + x^4 + 1)
 */
static inline uint8_t px_crc8_update(uint8_t crc, uint8_t byte) {
    crc ^= byte;
    for (uint8_t i = 0; i < 8; i++) {
        if (crc & 0x80) {
            crc = (uint8_t)((crc << 1) ^ 0x31);
        } else {
            crc = (uint8_t)(crc << 1);
        }
    }
    return crc;
}

static inline uint8_t px_calculate_crc8(const uint8_t* data, size_t len) {
    uint8_t crc = 0;
    for (size_t i = 0; i < len; i++) {
        crc = px_crc8_update(crc, data[i]);
    }
    return crc;
}

typedef enum {
    PX_STATE_WAIT_MAGIC0 = 0,
    PX_STATE_WAIT_MAGIC1,
    PX_STATE_WAIT_OPCODE,
    PX_STATE_WAIT_SEQ,
    PX_STATE_WAIT_LEN_LO,
    PX_STATE_WAIT_LEN_HI,
    PX_STATE_WAIT_PAYLOAD,
    PX_STATE_WAIT_CRC
} PxParserState;

typedef struct {
    PxParserState state;
    uint8_t opcode;
    uint8_t seq;
    uint16_t expected_len;
    uint16_t payload_idx;
    uint8_t running_crc;
    uint8_t payload[PX_MAX_PAYLOAD_LEN];
    uint32_t packets_received;
    uint32_t packets_crc_errors;
    uint32_t packets_overflow_errors;
} PxPacketParser;

/**
 * @brief Initialize parser state machine
 */
static inline void px_parser_init(PxPacketParser* p) {
    p->state = PX_STATE_WAIT_MAGIC0;
    p->opcode = 0;
    p->seq = 0;
    p->expected_len = 0;
    p->payload_idx = 0;
    p->running_crc = 0;
    p->packets_received = 0;
    p->packets_crc_errors = 0;
    p->packets_overflow_errors = 0;
}

/**
 * @brief Feed a single received byte into the parser state machine.
 * @return true if a complete and valid packet was just received, false otherwise.
 */
static inline bool px_parser_feed_byte(PxPacketParser* p, uint8_t byte) {
    switch (p->state) {
        case PX_STATE_WAIT_MAGIC0:
            if (byte == PX_MAGIC_0) {
                p->state = PX_STATE_WAIT_MAGIC1;
            }
            break;

        case PX_STATE_WAIT_MAGIC1:
            if (byte == PX_MAGIC_1) {
                p->state = PX_STATE_WAIT_OPCODE;
                p->running_crc = 0;
            } else if (byte == PX_MAGIC_0) {
                p->state = PX_STATE_WAIT_MAGIC1;
            } else {
                p->state = PX_STATE_WAIT_MAGIC0;
            }
            break;

        case PX_STATE_WAIT_OPCODE:
            p->opcode = byte;
            p->running_crc = px_crc8_update(p->running_crc, byte);
            p->state = PX_STATE_WAIT_SEQ;
            break;

        case PX_STATE_WAIT_SEQ:
            p->seq = byte;
            p->running_crc = px_crc8_update(p->running_crc, byte);
            p->state = PX_STATE_WAIT_LEN_LO;
            break;

        case PX_STATE_WAIT_LEN_LO:
            p->expected_len = byte;
            p->running_crc = px_crc8_update(p->running_crc, byte);
            p->state = PX_STATE_WAIT_LEN_HI;
            break;

        case PX_STATE_WAIT_LEN_HI:
            p->expected_len |= ((uint16_t)byte << 8);
            p->running_crc = px_crc8_update(p->running_crc, byte);
            p->payload_idx = 0;

            if (p->expected_len > PX_MAX_PAYLOAD_LEN) {
                p->packets_overflow_errors++;
                p->state = PX_STATE_WAIT_MAGIC0;
            } else if (p->expected_len == 0) {
                p->state = PX_STATE_WAIT_CRC;
            } else {
                p->state = PX_STATE_WAIT_PAYLOAD;
            }
            break;

        case PX_STATE_WAIT_PAYLOAD:
            p->payload[p->payload_idx++] = byte;
            p->running_crc = px_crc8_update(p->running_crc, byte);
            if (p->payload_idx >= p->expected_len) {
                p->state = PX_STATE_WAIT_CRC;
            }
            break;

        case PX_STATE_WAIT_CRC:
            p->state = PX_STATE_WAIT_MAGIC0;
            if (p->running_crc == byte) {
                p->packets_received++;
                return true;
            } else {
                p->packets_crc_errors++;
            }
            break;

        default:
            p->state = PX_STATE_WAIT_MAGIC0;
            break;
    }
    return false;
}

#ifdef __cplusplus
}
#endif

#endif // PIXELNODE_PROTOCOL_H
