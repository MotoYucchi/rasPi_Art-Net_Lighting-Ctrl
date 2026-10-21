/**
 * @file ws2812_driver.h
 * @brief Zero-dependency cycle-accurate WS2811/WS2812B driver for Arduino Uno (ATmega328P @ 16MHz).
 *
 * Pin: Arduino Digital Pin D6 (PORTD, Bit 6).
 *
 * Driver Selection Hierarchy:
 * 1. Adafruit_NeoPixel library (automatic if installed/included or #define USE_ADAFRUIT_NEOPIXEL)
 * 2. FastLED library (if #define USE_FASTLED or FastLED.h included)
 * 3. Pure cycle-accurate 16MHz AVR unrolled assembly driver (Zero external dependencies).
 *    Uses 20-cycle bit timing (800 kHz) with byte-loading during Bit 0 low time.
 *    Eliminates C compiler loop gaps (which previously caused WS2811 reset aborts).
 */

#ifndef WS2812_DRIVER_H
#define WS2812_DRIVER_H

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#define WS2812_PIN 6
#define WS2812_MAX_LEDS 300

#if defined(ARDUINO)

// Check for Adafruit_NeoPixel or FastLED
#if defined(USE_ADAFRUIT_NEOPIXEL) || __has_include(<Adafruit_NeoPixel.h>)
#define PIXELNODE_USE_NEOPIXEL 1
#include <Adafruit_NeoPixel.h>
static Adafruit_NeoPixel px_strip(WS2812_MAX_LEDS, WS2812_PIN, NEO_GRB + NEO_KHZ800);

static inline void ws2812_init(void) {
    px_strip.begin();
    px_strip.show();
}

static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    if (len == 0 || data == NULL) return;
    uint8_t* px = px_strip.getPixels();
    if (px != NULL) {
        uint16_t max_bytes = px_strip.numPixels() * 3;
        uint16_t copy_len = (len > max_bytes) ? max_bytes : len;
        memcpy(px, data, copy_len);
        px_strip.show();
    }
}

#elif defined(USE_FASTLED) || __has_include(<FastLED.h>)
#define PIXELNODE_USE_FASTLED 1
#include <FastLED.h>
static CRGB px_leds[WS2812_MAX_LEDS];

static inline void ws2812_init(void) {
    FastLED.addLeds<WS2812B, WS2812_PIN, GRB>(px_leds, WS2812_MAX_LEDS);
    FastLED.show();
}

static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    if (len == 0 || data == NULL) return;
    uint16_t max_bytes = WS2812_MAX_LEDS * 3;
    uint16_t copy_len = (len > max_bytes) ? max_bytes : len;
    memcpy((uint8_t*)px_leds, data, copy_len);
    FastLED.show();
}

#elif defined(__AVR_ATmega328P__)
#include <avr/io.h>
#include <avr/interrupt.h>
#include <util/delay.h>

#define WS2812_PORT PORTD
#define WS2812_DDR  DDRD
#define WS2812_BIT  6

/**
 * @brief Initialize WS2812 output pin (D6) as digital output LOW.
 */
static inline void ws2812_init(void) {
    WS2812_DDR |= (1 << WS2812_BIT);
    WS2812_PORT &= ~(1 << WS2812_BIT);
}

/**
 * @brief Transmit raw packed RGB/GRB byte array to WS2812/WS2811 on ATmega328P @ 16MHz.
 *
 * Implements cycle-accurate 20-cycle (1.25us = 800kHz) timing:
 *   Bit 0: High ~375ns (6 cycles), Low ~875ns (14 cycles)
 *   Bit 1: High ~812ns (13 cycles), Low ~438ns (7 cycles)
 *
 * Crucial optimization: Bit 0 low time executes `ld` (next byte), `sbiw` (count),
 * and `brne` (loop), guaranteeing ZERO inter-byte delay and preventing WS2811 reset timeouts.
 */
static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    if (len == 0 || data == NULL) return;

    WS2812_DDR |= (1 << WS2812_BIT);

    volatile uint8_t* port = &WS2812_PORT;
    uint8_t pinMask = (1 << WS2812_BIT);

    volatile uint16_t i = len; // Loop counter
    const uint8_t *ptr = data; // Pointer to next byte
    volatile uint8_t b = *ptr++; // Current byte value
    volatile uint8_t hi = *port | pinMask;
    volatile uint8_t lo = *port & ~pinMask;
    volatile uint8_t next = lo;
    volatile uint8_t bit = 8;

    uint8_t sreg = SREG;
    cli();

    asm volatile(
        "head20%=:\n\t"            // Clk  Pseudocode    (T =  0)
        "st   %a[port],  %[hi]\n\t"// 2    PORT = hi     (T =  2)
        "sbrc %[byte],  7\n\t"     // 1-2  if(b & 128)
        "mov  %[next], %[hi]\n\t"  // 0-1   next = hi    (T =  4)
        "dec  %[bit]\n\t"          // 1    bit--         (T =  5)
        "st   %a[port],  %[next]\n\t"// 2  PORT = next   (T =  7)
        "mov  %[next] ,  %[lo]\n\t"// 1    next = lo     (T =  8)
        "breq nextbyte20%=\n\t"    // 1-2  if(bit == 0) (from dec above)
        "rol  %[byte]\n\t"         // 1    b <<= 1       (T = 10)
        "rjmp .+0\n\t"             // 2    nop nop       (T = 12)
        "nop\n\t"                  // 1    nop           (T = 13)
        "st   %a[port],  %[lo]\n\t"// 2    PORT = lo     (T = 15)
        "nop\n\t"                  // 1    nop           (T = 16)
        "rjmp .+0\n\t"             // 2    nop nop       (T = 18)
        "rjmp head20%=\n\t"        // 2    -> head20 (next bit out)
        "nextbyte20%=:\n\t"        //                    (T = 10)
        "ldi  %[bit]  ,  8\n\t"    // 1    bit = 8       (T = 11)
        "ld   %[byte] ,  %a[ptr]+\n\t"// 2 b = *ptr++    (T = 13)
        "st   %a[port], %[lo]\n\t" // 2    PORT = lo     (T = 15)
        "nop\n\t"                  // 1    nop           (T = 16)
        "sbiw %[count], 1\n\t"     // 2    i--           (T = 18)
        "brne head20%=\n\t"        // 2    if(i != 0) -> (next byte)
        : [port] "+e"(port),
          [byte] "+r"(b),
          [bit] "+r"(bit),
          [next] "+&r"(next),
          [count] "+w"(i)
        : [ptr] "e"(ptr),
          [hi] "r"(hi),
          [lo] "r"(lo)
    );

    SREG = sreg;

    // Reset latch delay: >= 300us (WS2811/WS2812 standard)
    _delay_us(300);
}

#else
// Generic Arduino non-AVR fallback (e.g. SAMD, ESP)
static inline void ws2812_init(void) {
    pinMode(WS2812_PIN, OUTPUT);
    digitalWrite(WS2812_PIN, LOW);
}
static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    (void)data;
    (void)len;
}
#endif

#else

// Mock for non-Arduino native unit testing environments (g++, cargo test)
static inline void ws2812_init(void) {}
static inline void ws2812_show(const uint8_t* data, uint16_t len) {
    (void)data;
    (void)len;
}

#endif // ARDUINO

#endif // WS2812_DRIVER_H
