/**
 * @file pixelnode_patterns.h
 * @brief Exact lighting animation patterns (Ch 6) & audio-reactive patterns (Ch 7) for PixelNode MCU.
 *
 * Implements the exact same 24 autonomous patterns and 24 audio sync patterns as
 * Raspberry Pi's Rust RenderEngine.
 *
 * Zero heap allocations, bounded execution time, deterministic.
 */

#ifndef PIXELNODE_PATTERNS_H
#define PIXELNODE_PATTERNS_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <math.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

#ifndef TAU
#define TAU (2.0 * M_PI)
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint8_t r;
    uint8_t g;
    uint8_t b;
} PxRgbColor;

typedef struct {
    uint8_t energy;       // Overall RMS 0..255
    uint8_t peak;         // Peak level 0..255
    uint8_t bass;         // Low band energy 0..255
    uint8_t mid;          // Mid band energy 0..255
    uint8_t treble;       // High band energy 0..255
    uint8_t beat_phase;   // 0 = on beat, 128 = off beat (0..255)
    uint8_t is_kick;      // 1 if kick drum onset detected
    uint8_t is_snare;     // 1 if snare drum onset detected
    uint8_t is_hihat;     // 1 if hi-hat onset detected
    uint8_t bpm;          // BPM estimate (e.g. 120)
} PxAudioFeatures;

static inline float px_clamp_f(float val, float min_v, float max_v) {
    if (val < min_v) return min_v;
    if (val > max_v) return max_v;
    return val;
}

static inline float px_rem_euclid(float a, float b) {
    float r = fmodf(a, b);
    return (r < 0.0f) ? (r + b) : r;
}

static inline PxRgbColor px_scale_color(PxRgbColor c, float factor) {
    float f = px_clamp_f(factor, 0.0f, 1.0f);
    PxRgbColor out;
    out.r = (uint8_t)((float)c.r * f);
    out.g = (uint8_t)((float)c.g * f);
    out.b = (uint8_t)((float)c.b * f);
    return out;
}

static inline PxRgbColor px_lerp_color(PxRgbColor c1, PxRgbColor c2, float t) {
    float factor = px_clamp_f(t, 0.0f, 1.0f);
    float inv = 1.0f - factor;
    PxRgbColor out;
    out.r = (uint8_t)(((float)c1.r * inv) + ((float)c2.r * factor));
    out.g = (uint8_t)(((float)c1.g * inv) + ((float)c2.g * factor));
    out.b = (uint8_t)(((float)c1.b * inv) + ((float)c2.b * factor));
    return out;
}

static inline PxRgbColor px_add_color(PxRgbColor c1, PxRgbColor c2) {
    PxRgbColor out;
    uint16_t r = (uint16_t)c1.r + (uint16_t)c2.r;
    uint16_t g = (uint16_t)c1.g + (uint16_t)c2.g;
    uint16_t b = (uint16_t)c1.b + (uint16_t)c2.b;
    out.r = (r > 255) ? 255 : (uint8_t)r;
    out.g = (g > 255) ? 255 : (uint8_t)g;
    out.b = (b > 255) ? 255 : (uint8_t)b;
    return out;
}

static inline uint32_t px_pseudo_rand(uint32_t seed) {
    uint32_t x = seed + 0x9E3779B9u;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    return x;
}

static inline float px_spatial_noise(size_t px_idx, uint32_t time_step) {
    uint32_t seed = ((uint32_t)px_idx * 374761393u) ^ (time_step * 668265263u);
    uint32_t r = px_pseudo_rand(seed);
    return (float)(r & 0xFFFFu) / 65535.0f;
}

static inline PxRgbColor px_hue_to_rgb(uint8_t hue) {
    uint8_t region = hue / 43;
    uint8_t remainder = (hue - (region * 43)) * 6;
    uint8_t p = 0;
    uint8_t q = 255 - remainder;
    uint8_t t = remainder;
    PxRgbColor c;
    switch (region) {
        case 0: c.r = 255; c.g = t; c.b = p; break;
        case 1: c.r = q; c.g = 255; c.b = p; break;
        case 2: c.r = p; c.g = 255; c.b = t; break;
        case 3: c.r = p; c.g = q; c.b = 255; break;
        case 4: c.r = t; c.g = p; c.b = 255; break;
        default: c.r = 255; c.g = p; c.b = q; break;
    }
    return c;
}

/**
 * @brief Render autonomous lighting patterns (Ch 6) matching Raspberry Pi RenderEngine
 */
static inline void px_render_pattern(
    uint8_t pattern_dmx,
    PxRgbColor base_color,
    uint8_t speed,
    uint8_t size,
    float t,
    PxRgbColor* out,
    size_t count
) {
    if (count == 0 || out == NULL) return;

    uint8_t slot = pattern_dmx / 8; // Slots 0..=23
    float count_f = (float)count;

    // Speed normalization: 128 is center (1.0x). >128 positive, <128 reverse.
    float speed_mult;
    if (speed == 128) {
        speed_mult = 1.0f;
    } else if (speed > 128) {
        speed_mult = 1.0f + (((float)(speed - 128) / 127.0f) * 4.0f);
    } else {
        speed_mult = -(((float)(128 - speed) / 128.0f) * 4.0f);
        if (speed_mult < -4.0f) speed_mult = -4.0f;
    }

    // Size normalization: 128 is 1.0x
    float size_norm = px_clamp_f((float)size / 128.0f, 0.1f, 4.0f);

    switch (slot) {
        // Slot 0: Static (Base color fill)
        case 0: {
            for (size_t i = 0; i < count; i++) out[i] = base_color;
            break;
        }

        // Slot 1: Single Chase
        case 1: {
            float width = size_norm * (count_f * 0.15f);
            if (width < 1.0f) width = 1.0f;
            float pos = px_rem_euclid(t * speed_mult * 2.0f, 1.0f) * count_f;
            for (size_t i = 0; i < count; i++) {
                float d = fabsf((float)i - pos);
                float dist = (d < (count_f - d)) ? d : (count_f - d);
                float factor = px_clamp_f(1.0f - (dist / width), 0.0f, 1.0f);
                out[i] = px_scale_color(base_color, factor * factor);
            }
            break;
        }

        // Slot 2: Dual Meteor / Comet
        case 2: {
            float tail_len = size_norm * (count_f * 0.25f);
            if (tail_len < 2.0f) tail_len = 2.0f;
            float head1 = px_rem_euclid(t * speed_mult * 1.5f, 1.0f) * count_f;
            float head2 = px_rem_euclid(-t * speed_mult * 1.5f, 1.0f) * count_f;

            for (size_t i = 0; i < count; i++) {
                float i_f = (float)i;
                float d1 = px_rem_euclid(head1 - i_f, count_f);
                float f1 = (d1 < tail_len) ? expf(-d1 / (tail_len * 0.35f)) : 0.0f;
                float d2 = px_rem_euclid(i_f - head2, count_f);
                float f2 = (d2 < tail_len) ? expf(-d2 / (tail_len * 0.35f)) : 0.0f;

                PxRgbColor c1 = px_scale_color(base_color, f1);
                PxRgbColor c2 = px_scale_color(base_color, f2);
                PxRgbColor merged = px_add_color(c1, c2);

                uint8_t spark = (d1 < 1.0f || d2 < 1.0f) ? 100 : 0;
                uint16_t r = (uint16_t)merged.r + spark;
                uint16_t g = (uint16_t)merged.g + spark;
                uint16_t b = (uint16_t)merged.b + spark;
                out[i].r = (r > 255) ? 255 : (uint8_t)r;
                out[i].g = (g > 255) ? 255 : (uint8_t)g;
                out[i].b = (b > 255) ? 255 : (uint8_t)b;
            }
            break;
        }

        // Slot 3: Gravity Bounce
        case 3: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(2.0f / spd, 0.5f, 5.0f);
            float phase = fmodf(t, period) / period;
            float p_term = 2.0f * phase - 1.0f;
            float height = 1.0f - (p_term * p_term);
            float ball_pos = height * (count_f - 1.0f);
            float ball_radius = size_norm * 2.0f;
            if (ball_radius < 1.0f) ball_radius = 1.0f;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - ball_pos);
                float factor = px_clamp_f(1.0f - (dist / ball_radius), 0.0f, 1.0f);
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 4: Newton's Cradle
        case 4: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(1.5f / spd, 0.4f, 4.0f);
            float phase = fmodf(t, period) / period;
            float ball_size = size_norm * 2.0f;
            if (ball_size < 1.0f) ball_size = 1.0f;

            float active_pos, rest_start, rest_end;
            if (phase < 0.5f) {
                float swing = sinf(phase * (float)TAU * 2.0f);
                if (swing < 0.0f) swing = 0.0f;
                active_pos = swing * (count_f * 0.35f);
                rest_start = count_f * 0.35f;
                rest_end = count_f - 1.0f;
            } else {
                float swing = sinf((phase - 0.5f) * (float)TAU * 2.0f);
                if (swing < 0.0f) swing = 0.0f;
                active_pos = (count_f - 1.0f) - (swing * (count_f * 0.35f));
                rest_start = 0.0f;
                rest_end = count_f * 0.65f;
            }

            for (size_t i = 0; i < count; i++) {
                float i_f = (float)i;
                bool is_resting = (i_f >= rest_start && i_f <= rest_end);
                float active_dist = fabsf(i_f - active_pos);
                float factor;
                if (active_dist < ball_size) {
                    factor = 1.0f - (active_dist / ball_size);
                } else if (is_resting) {
                    factor = 0.25f;
                } else {
                    factor = 0.0f;
                }
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 5: Liquid Ripple
        case 5: {
            float w1_center = count_f * 0.25f;
            float w2_center = count_f * 0.75f;
            float freq = 4.0f / size_norm;
            float omega = t * speed_mult * 6.0f;

            for (size_t i = 0; i < count; i++) {
                float i_f = (float)i;
                float d1 = fabsf(i_f - w1_center) / count_f;
                float d2 = fabsf(i_f - w2_center) / count_f;
                float wave1 = sinf(d1 * freq * (float)TAU - omega);
                float wave2 = sinf(d2 * freq * (float)TAU - omega);
                float interference = ((wave1 + wave2) * 0.5f + 1.0f) * 0.5f;
                out[i] = px_scale_color(base_color, px_clamp_f(interference, 0.0f, 1.0f));
            }
            break;
        }

        // Slot 6: Plasma Flow
        case 6: {
            float k1 = (4.0f / size_norm) / count_f;
            float k2 = (7.0f / size_norm) / count_f;
            float k3 = (11.0f / size_norm) / count_f;
            float w = t * speed_mult * 2.5f;

            for (size_t i = 0; i < count; i++) {
                float i_f = (float)i;
                float v1 = sinf(i_f * k1 * (float)TAU + w);
                float v2 = sinf(i_f * k2 * (float)TAU - w * 1.3f);
                float v3 = cosf((i_f + w * 2.0f) * k3 * (float)TAU);

                float plasma = (v1 + v2 + v3 + 3.0f) / 6.0f;
                uint8_t hue_shift = (uint8_t)((plasma * 60.0f) + 128.0f);
                PxRgbColor col = px_lerp_color(base_color, px_hue_to_rgb(hue_shift), 0.35f);
                out[i] = px_scale_color(col, px_clamp_f(plasma, 0.0f, 1.0f));
            }
            break;
        }

        // Slot 7: Sand Cascade
        case 7: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float cycle_time = px_clamp_f(6.0f / spd, 1.5f, 12.0f);
            float progress = fmodf(t, cycle_time) / cycle_time;
            size_t filled_len = (size_t)(progress * count_f);
            uint32_t time_step = (uint32_t)(t * 20.0f * spd);

            for (size_t i = 0; i < count; i++) {
                if (i >= (count > filled_len ? count - filled_len : 0)) {
                    out[i] = base_color;
                } else {
                    float noise = px_spatial_noise(i, time_step);
                    if (noise > 0.88f) {
                        out[i] = px_scale_color(base_color, 0.9f);
                    } else {
                        out[i] = (PxRgbColor){0, 0, 0};
                    }
                }
            }
            break;
        }

        // Slot 8: Sine Wave Breath
        case 8: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(4.0f / spd, 1.0f, 8.0f);
            float phase = fmodf(t, period) / period;
            float breath = (phase < 0.35f) ?
                sinf(phase / 0.35f * (float)(M_PI * 0.5)) :
                sinf((1.0f - (phase - 0.35f) / 0.65f) * (float)(M_PI * 0.5));

            float spatial_k = ((float)TAU / count_f) / size_norm;
            for (size_t i = 0; i < count; i++) {
                float spatial_wave = (sinf((float)i * spatial_k) * 0.15f) + 0.85f;
                float brightness = px_clamp_f(breath * spatial_wave, 0.0f, 1.0f);
                out[i] = px_scale_color(base_color, brightness);
            }
            break;
        }

        // Slot 9: Heartbeat Pulse
        case 9: {
            float spd = fabsf(speed_mult);
            if (spd < 0.3f) spd = 0.3f;
            float bpm = px_clamp_f(72.0f * spd, 40.0f, 180.0f);
            float beat_period = 60.0f / bpm;
            float beat_phase = fmodf(t, beat_period) / beat_period;
            float center = count_f * 0.5f;

            float pulse_intensity;
            if (beat_phase < 0.12f) {
                pulse_intensity = sinf(beat_phase / 0.12f * (float)M_PI);
            } else if (beat_phase > 0.18f && beat_phase < 0.32f) {
                pulse_intensity = sinf((beat_phase - 0.18f) / 0.14f * (float)M_PI) * 0.65f;
            } else {
                pulse_intensity = 0.02f;
            }

            float wave_front = pulse_intensity * center;
            float wave_width = size_norm * 3.0f;
            if (wave_width < 1.0f) wave_width = 1.0f;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                float wave_dist = fabsf(dist - wave_front);
                float factor = px_clamp_f(1.0f - (wave_dist / wave_width), 0.0f, 1.0f) * pulse_intensity;
                out[i] = px_scale_color(base_color, px_clamp_f(factor, 0.03f, 1.0f));
            }
            break;
        }

        // Slot 10: Campfire Organic Flicker
        case 10: {
            uint32_t time_step = (uint32_t)(t * 25.0f * fabsf(speed_mult));
            PxRgbColor hot_white = {255, 240, 200};

            for (size_t i = 0; i < count; i++) {
                float pos_ratio = (float)i / count_f;
                float noise = px_spatial_noise(i, time_step);
                float flame_h = px_clamp_f(1.0f - pos_ratio * (1.2f / size_norm), 0.0f, 1.0f);
                float intensity = px_clamp_f(flame_h * (0.6f + 0.4f * noise), 0.0f, 1.0f);
                PxRgbColor col = (pos_ratio < 0.25f) ?
                    px_lerp_color(base_color, hot_white, 1.0f - pos_ratio * 4.0f) :
                    base_color;
                out[i] = px_scale_color(col, intensity);
            }
            break;
        }

        // Slot 11: Aurora Curtain
        case 11: {
            float w1 = t * speed_mult * 0.8f;
            float w2 = t * speed_mult * 1.4f;
            PxRgbColor comp = {
                (uint8_t)(255 - base_color.r),
                (uint8_t)(255 - base_color.g),
                (uint8_t)(255 - base_color.b)
            };

            for (size_t i = 0; i < count; i++) {
                float pos = (float)i / count_f;
                float wave1 = (sinf(pos * 5.0f / size_norm + w1) + 1.0f) * 0.5f;
                float wave2 = (sinf(pos * 9.0f / size_norm - w2) + 1.0f) * 0.5f;
                float blend = (wave1 + wave2) * 0.5f;
                PxRgbColor col = px_lerp_color(base_color, comp, blend * 0.4f);
                out[i] = px_scale_color(col, blend * blend);
            }
            break;
        }

        // Slot 12: Electric Arc / Lightning
        case 12: {
            uint32_t strike_rate = (uint32_t)(t * 12.0f * fabsf(speed_mult));
            float noise = px_spatial_noise(0, strike_rate);
            bool has_strike = (noise > 0.82f);

            if (has_strike) {
                size_t strike_pos = (size_t)(px_spatial_noise(1, strike_rate) * count_f);
                size_t arc_radius = (size_t)(size_norm * 4.0f);
                PxRgbColor white = {255, 255, 255};

                for (size_t i = 0; i < count; i++) {
                    int dist = (int)i - (int)strike_pos;
                    size_t d = (size_t)abs(dist);
                    if (d <= arc_radius) {
                        float f = 1.0f - ((float)d / ((float)arc_radius + 1.0f));
                        out[i] = px_lerp_color(base_color, white, f);
                    } else {
                        out[i] = (PxRgbColor){0, 0, 0};
                    }
                }
            } else {
                for (size_t i = 0; i < count; i++) {
                    out[i] = px_scale_color(base_color, 0.05f);
                }
            }
            break;
        }

        // Slot 13: Firefly Swarm
        case 13: {
            for (size_t i = 0; i < count; i++) {
                uint32_t seed = (uint32_t)i * 1234567u;
                float freq = 0.5f + ((float)(seed & 0xFF) / 255.0f) * 1.5f * fabsf(speed_mult);
                float phase_offset = ((float)((seed >> 8) & 0xFF) / 255.0f) * (float)TAU;
                float sine = sinf(t * freq + phase_offset);
                float pos_s = (sine > 0.0f) ? sine : 0.0f;
                float intensity = pos_s * pos_s * pos_s * pos_s;
                out[i] = px_scale_color(base_color, intensity);
            }
            break;
        }

        // Slot 14: Neural Synapse
        case 14: {
            uint32_t burst_step = (uint32_t)(t * 4.0f * fabsf(speed_mult));
            size_t node_pos = (size_t)(px_spatial_noise(2, burst_step) * count_f);
            float pulse_prog = fmodf(t * 8.0f * fabsf(speed_mult), 1.0f) * count_f * 0.4f;

            for (size_t i = 0; i < count; i++) {
                float dist = (float)abs((int)i - (int)node_pos);
                float d_pulse = fabsf(dist - pulse_prog);
                float factor = (d_pulse < (size_norm * 2.0f)) ?
                    (1.0f - (d_pulse / (size_norm * 2.0f))) : 0.0f;
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 15: Jellyfish Bioluminescence
        case 15: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(3.0f / spd, 1.0f, 6.0f);
            float phase = fmodf(t, period) / period;
            float center = count_f * 0.5f;

            float bloom_pos, brightness;
            if (phase < 0.3f) {
                bloom_pos = (1.0f - (phase / 0.3f)) * center;
                brightness = 0.4f + (phase / 0.3f) * 0.6f;
            } else {
                bloom_pos = ((phase - 0.3f) / 0.7f) * center;
                float fade = 1.0f - ((phase - 0.3f) / 0.7f);
                brightness = fade * fade;
            }

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                float delta = fabsf(dist - bloom_pos);
                float factor = (delta < (size_norm * 4.0f)) ?
                    (1.0f - (delta / (size_norm * 4.0f))) * brightness : 0.0f;
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 16: Rainbow Spectrum Flow
        case 16: {
            uint32_t offset = (uint32_t)(t * speed_mult * 80.0f);
            float span = (count_f * size_norm > 1.0f) ? count_f * size_norm : 1.0f;
            uint32_t step = (uint32_t)((256.0f / span) * 256.0f);
            for (size_t i = 0; i < count; i++) {
                uint8_t hue = (uint8_t)(((uint32_t)i * step / 256u) + offset);
                out[i] = px_hue_to_rgb(hue);
            }
            break;
        }

        // Slot 17: Knight Rider / Scanner
        case 17: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(2.0f / spd, 0.5f, 5.0f);
            float phase = fmodf(t, period) / period;
            float head = (sinf(phase * (float)TAU) * 0.5f + 0.5f) * (count_f - 1.0f);
            float tail_width = size_norm * 3.5f;
            if (tail_width < 1.0f) tail_width = 1.0f;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - head);
                float factor = px_clamp_f(1.0f - (dist / tail_width), 0.0f, 1.0f);
                out[i] = px_scale_color(base_color, factor * factor);
            }
            break;
        }

        // Slot 18: Curtain Center Wipe
        case 18: {
            float spd = fabsf(speed_mult);
            if (spd < 0.2f) spd = 0.2f;
            float period = px_clamp_f(3.0f / spd, 0.8f, 6.0f);
            float phase = fmodf(t, period) / period;
            float center = count_f * 0.5f;
            float open_len = (phase < 0.5f) ?
                (phase * 2.0f) * center :
                (1.0f - (phase - 0.5f) * 2.0f) * center;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                out[i] = (dist <= open_len) ? base_color : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 19: Strobe Dot Matrix / Cinema Flicker
        case 19: {
            uint32_t freq = (uint32_t)(t * 20.0f * fabsf(speed_mult));
            size_t block_size = (size_t)(size_norm * 2.0f);
            if (block_size < 1) block_size = 1;
            bool is_even_frame = ((freq % 2) == 0);

            for (size_t i = 0; i < count; i++) {
                size_t block_idx = i / block_size;
                bool is_on = ((block_idx % 2 == 0) == is_even_frame);
                out[i] = is_on ? base_color : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 20: Diamond Sparkle / Shimmer
        case 20: {
            uint32_t time_step = (uint32_t)(t * 30.0f * fabsf(speed_mult));
            PxRgbColor white = {255, 255, 255};
            float threshold = 1.0f - (0.08f * size_norm);

            for (size_t i = 0; i < count; i++) {
                float noise = px_spatial_noise(i, time_step);
                out[i] = (noise > threshold) ? white : px_scale_color(base_color, 0.15f);
            }
            break;
        }

        // Slot 21: Sawtooth Treadmill
        case 21: {
            float pitch = count_f * 0.25f * size_norm;
            if (pitch < 2.0f) pitch = 2.0f;
            float offset = t * speed_mult * 3.0f * pitch;

            for (size_t i = 0; i < count; i++) {
                float pos = px_rem_euclid((float)i + offset, pitch);
                float ramp = pos / pitch;
                out[i] = px_scale_color(base_color, ramp);
            }
            break;
        }

        // Slot 22: Digital Matrix Rain
        case 22: {
            for (size_t i = 0; i < count; i++) out[i] = (PxRgbColor){0, 0, 0};
            float tail_len = size_norm * count_f * 0.12f;
            if (tail_len < 2.0f) tail_len = 2.0f;

            for (int s = 0; s < 4; s++) {
                float stream_speed = 0.8f + (px_spatial_noise(s, 0) * 0.8f);
                float drop_pos = px_rem_euclid(t * speed_mult * stream_speed * 1.5f + ((float)s * 0.25f), 1.0f) * count_f;

                for (size_t i = 0; i < count; i++) {
                    float d = px_rem_euclid(drop_pos - (float)i, count_f);
                    if (d < tail_len) {
                        float f = 1.0f - (d / tail_len);
                        PxRgbColor col = px_scale_color(base_color, f * f);
                        out[i] = px_add_color(out[i], col);
                    }
                }
            }
            break;
        }

        // Slot 23: Moiré Interference Grid
        case 23: {
            float k1 = (8.0f / size_norm) / count_f;
            float k2 = (9.5f / size_norm) / count_f;
            float w = t * speed_mult * 4.0f;

            for (size_t i = 0; i < count; i++) {
                float i_f = (float)i;
                float g1 = (sinf(i_f * k1 * (float)TAU + w) + 1.0f) * 0.5f;
                float g2 = (sinf(i_f * k2 * (float)TAU - w) + 1.0f) * 0.5f;
                float moire = g1 * g2;
                out[i] = px_scale_color(base_color, moire);
            }
            break;
        }

        // Slot 24: Center Ripple Pulse (Multi-wave expanding ripple with inner decay)
        case 24: {
            float wavelength = (count_f * 0.25f * size_norm);
            if (wavelength < 3.0f) wavelength = 3.0f;
            float inv_wavelength = 1.0f / wavelength;
            float speed = t * speed_mult * 30.0f;
            float center = (count_f - 1.0f) * 0.5f;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                float phase = px_rem_euclid(dist - speed, wavelength) * inv_wavelength;
                float crest = (1.0f - phase) * (1.0f - phase);
                float intensity = crest * (1.0f - phase);
                out[i] = px_scale_color(base_color, intensity);
            }
            break;
        }

        // Slot 25: Dual-Tone Color Wave (Smooth continuous transition between two vivid hues)
        case 25: {
            PxRgbColor color2;
            if (base_color.r == base_color.g && base_color.g == base_color.b) {
                color2.r = 255 - base_color.r;
                color2.g = 128;
                color2.b = base_color.r;
            } else {
                color2.r = base_color.g;
                color2.g = base_color.b;
                color2.b = base_color.r;
            }

            float width_val = count_f * 0.35f * size_norm;
            if (width_val < 4.0f) width_val = 4.0f;
            float spatial_freq = (float)TAU / width_val;
            float temporal_phase = t * speed_mult * 3.5f;

            for (size_t i = 0; i < count; i++) {
                float wave = (sinf((float)i * spatial_freq - temporal_phase) + 1.0f) * 0.5f;
                out[i] = px_lerp_color(base_color, color2, wave);
            }
            break;
        }

        // Slot 26: Directional Wave Pulse (Unidirectional cascaded attack pulses with exponential decay)
        case 26: {
            float interval = count_f * 0.3f * size_norm;
            if (interval < 3.0f) interval = 3.0f;
            float inv_interval = 1.0f / interval;
            float offset = t * speed_mult * 30.0f;

            for (size_t i = 0; i < count; i++) {
                float pos = px_rem_euclid((float)i - offset, interval) * inv_interval;
                float f = (1.0f - pos) * (1.0f - pos);
                float intensity = f * f;
                out[i] = px_scale_color(base_color, intensity);
            }
            break;
        }

        // Slot 27: Center Inward Pull (Converging waves moving inward from both ends to center)
        case 27: {
            float wavelength = count_f * 0.25f * size_norm;
            if (wavelength < 3.0f) wavelength = 3.0f;
            float inv_wavelength = 1.0f / wavelength;
            float speed = t * speed_mult * 30.0f;
            float center = (count_f - 1.0f) * 0.5f;

            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                float phase = px_rem_euclid(dist + speed, wavelength) * inv_wavelength;
                float intensity = phase * phase * phase;
                out[i] = px_scale_color(base_color, intensity);
            }
            break;
        }

        // Fallback slots 28..31
        default: {
            for (size_t i = 0; i < count; i++) out[i] = base_color;
            break;
        }
    }
}

/**
 * @brief Render audio-reactive patterns (Ch 7) matching Raspberry Pi RenderEngine
 */
static inline void px_render_audio_pattern(
    uint8_t audio_dmx,
    const PxAudioFeatures* audio,
    PxRgbColor base_color,
    float t,
    PxRgbColor* out,
    size_t count
) {
    if (count == 0 || out == NULL) return;
    if (audio == NULL) {
        for (size_t i = 0; i < count; i++) out[i] = base_color;
        return;
    }

    uint8_t slot = audio_dmx / 8;
    float count_f = (float)count;
    float rms_norm = (float)audio->energy / 255.0f;
    float peak_norm = (float)audio->peak / 255.0f;
    float bass_norm = (float)audio->bass / 255.0f;
    float mid_norm = (float)audio->mid / 255.0f;
    float treble_norm = (float)audio->treble / 255.0f;
    float phase_norm = (float)audio->beat_phase / 255.0f;

    switch (slot) {
        // Slot 0: Off / Neutral fallback
        case 0:
            for (size_t i = 0; i < count; i++) out[i] = base_color;
            break;

        // Slot 1: Master Pulse (Overall RMS dynamics)
        case 1: {
            PxRgbColor dimmed = px_scale_color(base_color, rms_norm);
            for (size_t i = 0; i < count; i++) out[i] = dimmed;
            break;
        }

        // Slot 2: Kick Pump Flash
        case 2: {
            if (audio->is_kick) {
                for (size_t i = 0; i < count; i++) out[i] = (PxRgbColor){255, 255, 255};
            } else {
                PxRgbColor c = px_scale_color(base_color, bass_norm);
                for (size_t i = 0; i < count; i++) out[i] = c;
            }
            break;
        }

        // Slot 3: Snare Crack Transient
        case 3: {
            if (audio->is_snare) {
                PxRgbColor cold_cyan = {180, 240, 255};
                for (size_t i = 0; i < count; i++) out[i] = cold_cyan;
            } else {
                PxRgbColor c = px_scale_color(base_color, mid_norm * 0.4f);
                for (size_t i = 0; i < count; i++) out[i] = c;
            }
            break;
        }

        // Slot 4: Hi-Hat Shimmer Sparkle
        case 4: {
            uint32_t step = (uint32_t)(t * 60.0f);
            for (size_t i = 0; i < count; i++) {
                float noise = px_spatial_noise(i, step);
                if (noise < treble_norm) {
                    out[i] = (PxRgbColor){255, 255, 255};
                } else {
                    out[i] = px_scale_color(base_color, 0.1f);
                }
            }
            break;
        }

        // Slot 5: Spectrum VU Meter (Center-out bar)
        case 5: {
            float center = count_f * 0.5f;
            float bar_len = peak_norm * center;
            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                if (dist <= bar_len) {
                    float pos = dist / (center > 0.0f ? center : 1.0f);
                    uint8_t hue = (uint8_t)((1.0f - pos) * 85.0f); // Green to Red
                    out[i] = px_hue_to_rgb(hue);
                } else {
                    out[i] = (PxRgbColor){0, 0, 0};
                }
            }
            break;
        }

        // Slot 6: Bass Shockwave Runner
        case 6: {
            float center = count_f * 0.5f;
            float wave_front = bass_norm * center;
            for (size_t i = 0; i < count; i++) {
                float dist = fabsf((float)i - center);
                float delta = fabsf(dist - wave_front);
                float factor = px_clamp_f(1.0f - (delta / 4.0f), 0.0f, 1.0f) * bass_norm;
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 7: Frequency Band Split EQ (Low/Mid/High)
        case 7: {
            size_t p1 = count / 3;
            size_t p2 = (count * 2) / 3;
            for (size_t i = 0; i < count; i++) {
                if (i < p1) {
                    out[i] = (PxRgbColor){(uint8_t)(bass_norm * 255.0f), 0, 0};
                } else if (i < p2) {
                    out[i] = (PxRgbColor){0, (uint8_t)(mid_norm * 255.0f), 0};
                } else {
                    out[i] = (PxRgbColor){0, 0, (uint8_t)(treble_norm * 255.0f)};
                }
            }
            break;
        }

        // Slot 8: Tempo Beat Strobe
        case 8: {
            bool strobe_on = (phase_norm < 0.15f);
            for (size_t i = 0; i < count; i++) {
                out[i] = strobe_on ? base_color : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 9: Waveform Oscilloscope
        case 9: {
            float omega = t * 10.0f;
            for (size_t i = 0; i < count; i++) {
                float wave = (sinf((float)i * 0.2f + omega) * 0.5f + 0.5f) * rms_norm;
                out[i] = px_scale_color(base_color, wave);
            }
            break;
        }

        // Slot 10: Kick Expanding Shockwave
        case 10: {
            float exp_pos = fmodf(t * 4.0f, 1.0f) * count_f;
            for (size_t i = 0; i < count; i++) {
                float d = fabsf((float)i - exp_pos);
                float factor = px_clamp_f(1.0f - (d / 3.0f), 0.0f, 1.0f) * bass_norm;
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 11: Treble Crystalline Shimmer
        case 11: {
            uint32_t step = (uint32_t)(t * 40.0f);
            for (size_t i = 0; i < count; i++) {
                float noise = px_spatial_noise(i, step);
                float factor = (noise > (1.0f - treble_norm * 0.5f)) ? 1.0f : 0.05f;
                out[i] = px_scale_color(base_color, factor);
            }
            break;
        }

        // Slot 12: Sub-Bass Warm Glow
        case 12: {
            PxRgbColor warm_amber = {255, 120, 20};
            PxRgbColor blend = px_lerp_color(base_color, warm_amber, 0.7f);
            PxRgbColor glow = px_scale_color(blend, bass_norm);
            for (size_t i = 0; i < count; i++) out[i] = glow;
            break;
        }

        // Slot 13: Audio Flame / Fire
        case 13: {
            uint32_t step = (uint32_t)(t * 25.0f);
            for (size_t i = 0; i < count; i++) {
                float pos_ratio = (float)i / count_f;
                float noise = px_spatial_noise(i, step);
                float h = px_clamp_f((1.0f - pos_ratio) * (rms_norm * 1.5f), 0.0f, 1.0f);
                out[i] = px_scale_color(base_color, h * noise);
            }
            break;
        }

        // Slot 14: Dual Sided VU Meter
        case 14: {
            float half = count_f * 0.5f;
            float left_bar = bass_norm * half;
            float right_bar = treble_norm * half;
            for (size_t i = 0; i < count; i++) {
                if ((float)i < half) {
                    out[i] = ((float)i <= left_bar) ? base_color : (PxRgbColor){0, 0, 0};
                } else {
                    float right_d = (float)i - half;
                    out[i] = (right_d <= right_bar) ? base_color : (PxRgbColor){0, 0, 0};
                }
            }
            break;
        }

        // Slot 15: Audio Sparkle Burst
        case 15: {
            uint32_t step = (uint32_t)(t * 50.0f);
            float thresh = 1.0f - (peak_norm * 0.4f);
            for (size_t i = 0; i < count; i++) {
                float noise = px_spatial_noise(i, step);
                out[i] = (noise > thresh) ? (PxRgbColor){255, 255, 255} : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 16: Beat Jump Chase
        case 16: {
            size_t step_pos = (size_t)(phase_norm * count_f);
            for (size_t i = 0; i < count; i++) {
                float d = fabsf((float)i - (float)step_pos);
                out[i] = (d < 3.0f) ? base_color : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 17: Vocal Glow
        case 17: {
            PxRgbColor magenta = {255, 40, 180};
            PxRgbColor col = px_lerp_color(base_color, magenta, 0.5f);
            PxRgbColor scaled = px_scale_color(col, mid_norm);
            for (size_t i = 0; i < count; i++) out[i] = scaled;
            break;
        }

        // Slot 18: Audio Color Shift
        case 18: {
            uint8_t hue = (uint8_t)(bass_norm * 120.0f + treble_norm * 120.0f);
            PxRgbColor c = px_hue_to_rgb(hue);
            for (size_t i = 0; i < count; i++) out[i] = c;
            break;
        }

        // Slot 19: Bass Drop Whiteout
        case 19: {
            if (bass_norm > 0.85f || audio->is_kick) {
                for (size_t i = 0; i < count; i++) out[i] = (PxRgbColor){255, 255, 255};
            } else {
                for (size_t i = 0; i < count; i++) out[i] = (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 20: Treble Lightning
        case 20: {
            if (treble_norm > 0.8f && (audio->is_hihat || audio->is_snare)) {
                for (size_t i = 0; i < count; i++) out[i] = (PxRgbColor){220, 240, 255};
            } else {
                for (size_t i = 0; i < count; i++) out[i] = px_scale_color(base_color, 0.05f);
            }
            break;
        }

        // Slot 21: Ambient Chill Ripple
        case 21: {
            float wave = (sinf(t * 2.0f) * 0.3f + 0.7f) * rms_norm;
            PxRgbColor c = px_scale_color(base_color, wave);
            for (size_t i = 0; i < count; i++) out[i] = c;
            break;
        }

        // Slot 22: Glitch Stutter
        case 22: {
            uint32_t step = (uint32_t)(t * 40.0f);
            bool is_on = (step % 3 == 0);
            for (size_t i = 0; i < count; i++) {
                out[i] = is_on ? px_scale_color(base_color, peak_norm) : (PxRgbColor){0, 0, 0};
            }
            break;
        }

        // Slot 23: Full Spectrum Overdrive
        case 23: {
            uint8_t hue = (uint8_t)(t * 80.0f);
            PxRgbColor c = px_hue_to_rgb(hue);
            PxRgbColor boosted = px_scale_color(c, peak_norm);
            for (size_t i = 0; i < count; i++) out[i] = boosted;
            break;
        }

        default:
            for (size_t i = 0; i < count; i++) out[i] = base_color;
            break;
    }
}

/**
 * @brief Apply master dimmer and strobe gating to pixel buffer
 */
static inline void px_apply_dimmer_and_strobe(
    uint8_t master_dimmer,
    uint8_t strobe_dmx,
    float t,
    PxRgbColor* out,
    size_t count
) {
    if (count == 0 || out == NULL) return;

    // Strobe gate: DMX 0..15 Open (always on), 16..255 rate 1..25 Hz
    bool strobe_open = true;
    if (strobe_dmx >= 16) {
        float hz = 1.0f + (((float)(strobe_dmx - 16) / 239.0f) * 24.0f); // 1.0 .. 25.0 Hz
        float period = 1.0f / hz;
        float cycle_pos = fmodf(t, period) / period;
        strobe_open = (cycle_pos < 0.5f);
    }

    uint8_t effective_dimmer = strobe_open ? master_dimmer : 0;
    for (size_t i = 0; i < count; i++) {
        out[i].r = (uint8_t)(((uint16_t)out[i].r * (uint16_t)effective_dimmer) / 255u);
        out[i].g = (uint8_t)(((uint16_t)out[i].g * (uint16_t)effective_dimmer) / 255u);
        out[i].b = (uint8_t)(((uint16_t)out[i].b * (uint16_t)effective_dimmer) / 255u);
    }
}

#ifdef __cplusplus
}
#endif

#endif // PIXELNODE_PATTERNS_H
