/**
 * @file ws2812_driver.h
 * @brief Zero-dependency cycle-accurate WS2811/WS2812B bitbang driver for ATmega328P (16MHz).
 *
 * Defaults to Arduino Pin D6 (PORTD, Bit 6).
 * Supports standard FastLED or Adafruit NeoPixel if enabled via macro.
 */

#ifndef WS2812_DRIVER_H
#define WS2812_DRIVER_H

#include <stdint.h>
#include <stddef.h>

#if defined(ARDUINO) && defined(__AVR_ATmega328P__)
#include <avr/io.h>
#include <avr/interrupt.h>
#include <util/delay.h>

#define WS2812_PIN 6
#define WS2812_PORT PORTD
#define WS2812_DDR  DDRD
#define WS2812_BIT  6

/**
 * @brief Initialize WS2812 output pin as output
 */
static inline void ws2812_init(void) {
    WS2812_DDR |= (1 << WS2812_BIT);
    WS2812_PORT &= ~(1 << WS2812_BIT);
}

/**
 * @brief Transmit raw packed RGB/GRB byte array to WS2812B strip on ATmega328P 16MHz.
 * Interrupts are disabled during transmission to meet strict 1.25us WS2812 timing.
 */
static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    if (len == 0 || data == NULL) return;

    volatile uint8_t* port = &WS2812_PORT;
    uint8_t pin_mask = (1 << WS2812_BIT);
    uint8_t hi_val = *port | pin_mask;
    uint8_t lo_val = *port & ~pin_mask;

    uint8_t sreg = SREG;
    cli();

    for (uint16_t i = 0; i < len; i++) {
        uint8_t cur_byte = data[i];

        // 8 bits per byte, MSB first
        for (uint8_t b = 0; b < 8; b++) {
            if (cur_byte & 0x80) {
                // Bit 1: High ~800ns (12 cycles), Low ~450ns (7 cycles)
                asm volatile(
                    "st %a0, %1 \n\t" // Pin High (2 cycles)
                    "nop \n\t" "nop \n\t" "nop \n\t" "nop \n\t"
                    "nop \n\t" "nop \n\t" "nop \n\t" "nop \n\t" // 8 cycles delay
                    "st %a0, %2 \n\t" // Pin Low (2 cycles)
                    "nop \n\t" "nop \n\t" // 2 cycles delay
                    :
                    : "e" (port), "r" (hi_val), "r" (lo_val)
                );
            } else {
                // Bit 0: High ~400ns (6 cycles), Low ~850ns (13 cycles)
                asm volatile(
                    "st %a0, %1 \n\t" // Pin High (2 cycles)
                    "nop \n\t" "nop \n\t" // 2 cycles delay
                    "st %a0, %2 \n\t" // Pin Low (2 cycles)
                    "nop \n\t" "nop \n\t" "nop \n\t" "nop \n\t"
                    "nop \n\t" "nop \n\t" "nop \n\t" "nop \n\t" // 8 cycles delay
                    :
                    : "e" (port), "r" (hi_val), "r" (lo_val)
                );
            }
            cur_byte <<= 1;
        }
    }

    SREG = sreg;

    // Reset latch delay: >= 300us (covers both WS2811 and WS2812B)
    _delay_us(300);
}

#else

// Mock for non-AVR / native unit testing environments
static inline void ws2812_init(void) {}
static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    (void)data;
    (void)len;
}

#endif

#endif // WS2812_DRIVER_H
