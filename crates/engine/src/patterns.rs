//! Autonomous Lighting Animation Patterns (DMX ch 6)
//!
//! Provides 24 professional lighting animation patterns spanning fluid physics,
//! organic/nature phenomena, and stage motion graphics.
//!
//! Guarantee: Zero allocations (`no_alloc` in hot loops), panic-free, deterministic.

use crate::{hue_to_rgb, RgbColor};
use core::time::Duration;

#[inline]
fn scale_color(c: RgbColor, factor: f32) -> RgbColor {
    let f = factor.clamp(0.0, 1.0);
    RgbColor {
        r: (c.r as f32 * f) as u8,
        g: (c.g as f32 * f) as u8,
        b: (c.b as f32 * f) as u8,
    }
}

#[inline]
fn lerp_color(c1: RgbColor, c2: RgbColor, t: f32) -> RgbColor {
    let factor = t.clamp(0.0, 1.0);
    let inv = 1.0 - factor;
    RgbColor {
        r: ((c1.r as f32 * inv) + (c2.r as f32 * factor)) as u8,
        g: ((c1.g as f32 * inv) + (c2.g as f32 * factor)) as u8,
        b: ((c1.b as f32 * inv) + (c2.b as f32 * factor)) as u8,
    }
}

#[inline]
fn add_color(c1: RgbColor, c2: RgbColor) -> RgbColor {
    RgbColor {
        r: c1.r.saturating_add(c2.r),
        g: c1.g.saturating_add(c2.g),
        b: c1.b.saturating_add(c2.b),
    }
}

/// Ultra-fast pseudo-random noise generator (Xorshift32, deterministic)
#[inline]
fn pseudo_rand(seed: u32) -> u32 {
    let mut x = seed.wrapping_add(0x9E3779B9);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Normalized pseudo-random float 0.0..=1.0 for spatial index and frame step
#[inline]
fn spatial_noise(px_idx: usize, time_step: u32) -> f32 {
    let seed = (px_idx as u32).wrapping_mul(374761393) ^ time_step.wrapping_mul(668265263);
    let r = pseudo_rand(seed);
    (r & 0xFFFF) as f32 / 65535.0
}

/// Render lighting animation pattern based on DMX slot
pub fn render_pattern(
    pattern_dmx: u8,
    base_color: RgbColor,
    speed: u8,
    size: u8,
    elapsed: Duration,
    out: &mut [RgbColor],
) {
    let count = out.len();
    if count == 0 {
        return;
    }

    let slot = pattern_dmx / 8; // 32 slots (0..=31)
    let t = elapsed.as_secs_f32();
    let count_f = count as f32;

    // Normalized parameters:
    // Speed: 128 is center/default. >128 positive, <128 reverse.
    let speed_mult = if speed == 128 {
        1.0
    } else if speed > 128 {
        1.0 + ((speed - 128) as f32 / 127.0) * 4.0
    } else {
        -(((128 - speed) as f32 / 128.0) * 4.0).max(-4.0)
    };

    // Size: 128 is standard width.
    let size_norm = (size as f32 / 128.0).clamp(0.1, 4.0);

    match slot {
        // Slot 0: Static (Base color fill)
        0 => {
            out.fill(base_color);
        }

        // Slot 1: Single Chase (Smooth anti-aliased moving head)
        1 => {
            let width = (size_norm * (count_f * 0.15).max(1.0)).max(1.0);
            let pos = ((t * speed_mult * 2.0).rem_euclid(1.0)) * count_f;
            for (i, px) in out.iter_mut().enumerate() {
                let dist = {
                    let d = (i as f32 - pos).abs();
                    d.min(count_f - d)
                };
                let factor = (1.0 - (dist / width)).max(0.0);
                *px = scale_color(base_color, factor * factor);
            }
        }

        // Slot 2: Dual Meteor / Comet (Bidirectional crossing streams with exponential tail)
        2 => {
            let tail_len = (size_norm * (count_f * 0.25).max(2.0)).max(2.0);
            let head1 = ((t * speed_mult * 1.5).rem_euclid(1.0)) * count_f;
            let head2 = ((-t * speed_mult * 1.5).rem_euclid(1.0)) * count_f;

            for (i, px) in out.iter_mut().enumerate() {
                let i_f = i as f32;
                // Meteor 1 (left to right)
                let d1 = (head1 - i_f).rem_euclid(count_f);
                let f1 = if d1 < tail_len { (-d1 / (tail_len * 0.35)).exp() } else { 0.0 };
                // Meteor 2 (right to left)
                let d2 = (i_f - head2).rem_euclid(count_f);
                let f2 = if d2 < tail_len { (-d2 / (tail_len * 0.35)).exp() } else { 0.0 };

                let c1 = scale_color(base_color, f1);
                let c2 = scale_color(base_color, f2);
                let merged = add_color(c1, c2);

                // Add slight white spark at head
                let head_spark = if d1 < 1.0 || d2 < 1.0 { 100 } else { 0 };
                *px = RgbColor {
                    r: merged.r.saturating_add(head_spark),
                    g: merged.g.saturating_add(head_spark),
                    b: merged.b.saturating_add(head_spark),
                };
            }
        }

        // Slot 3: Gravity Bounce (Ball dropping and bouncing with physical deceleration)
        3 => {
            let cycle_period = (2.0 / speed_mult.abs().max(0.2)).clamp(0.5, 5.0);
            let phase = (t % cycle_period) / cycle_period; // 0.0..1.0
            // Parabolic trajectory: peak at 0.0, hitting ground at 1.0
            let height = 1.0 - (2.0 * phase - 1.0).powi(2);
            let ball_pos = height * (count_f - 1.0);
            let ball_radius = (size_norm * 2.0).max(1.0);

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - ball_pos).abs();
                let factor = (1.0 - (dist / ball_radius)).max(0.0);
                *px = scale_color(base_color, factor);
            }
        }

        // Slot 4: Newton's Cradle (Elastic impact transfer between left and right ends)
        4 => {
            let period = (1.5 / speed_mult.abs().max(0.2)).clamp(0.4, 4.0);
            let phase = (t % period) / period; // 0..1
            let ball_size = (size_norm * 2.0).max(1.0);

            // phase 0.0..0.5: left ball swings out and hits back
            // phase 0.5..1.0: right ball swings out and hits back
            let (active_pos, rest_start, rest_end) = if phase < 0.5 {
                let swing = (phase * core::f32::consts::TAU * 2.0).sin();
                let swing_dist = swing.max(0.0) * (count_f * 0.35);
                (swing_dist, count_f * 0.35, count_f - 1.0)
            } else {
                let swing = ((phase - 0.5) * core::f32::consts::TAU * 2.0).sin();
                let swing_dist = swing.max(0.0) * (count_f * 0.35);
                ((count_f - 1.0) - swing_dist, 0.0, count_f * 0.65)
            };

            for (i, px) in out.iter_mut().enumerate() {
                let i_f = i as f32;
                let is_resting = i_f >= rest_start && i_f <= rest_end;
                let active_dist = (i_f - active_pos).abs();

                let factor = if active_dist < ball_size {
                    1.0 - (active_dist / ball_size)
                } else if is_resting {
                    0.25
                } else {
                    0.0
                };
                *px = scale_color(base_color, factor);
            }
        }

        // Slot 5: Liquid Ripple / Interference (2 wave sources forming constructive interference)
        5 => {
            let w1_center = count_f * 0.25;
            let w2_center = count_f * 0.75;
            let freq = 4.0 / size_norm;
            let omega = t * speed_mult * 6.0;

            for (i, px) in out.iter_mut().enumerate() {
                let i_f = i as f32;
                let d1 = (i_f - w1_center).abs() / count_f;
                let d2 = (i_f - w2_center).abs() / count_f;

                let wave1 = (d1 * freq * core::f32::consts::TAU - omega).sin();
                let wave2 = (d2 * freq * core::f32::consts::TAU - omega).sin();
                let interference = ((wave1 + wave2) * 0.5 + 1.0) * 0.5; // 0..1

                *px = scale_color(base_color, interference.clamp(0.0, 1.0));
            }
        }

        // Slot 6: Plasma Flow (Triple sine wave superposition)
        6 => {
            let k1 = (4.0 / size_norm) / count_f;
            let k2 = (7.0 / size_norm) / count_f;
            let k3 = (11.0 / size_norm) / count_f;
            let w = t * speed_mult * 2.5;

            for (i, px) in out.iter_mut().enumerate() {
                let i_f = i as f32;
                let v1 = (i_f * k1 * core::f32::consts::TAU + w).sin();
                let v2 = (i_f * k2 * core::f32::consts::TAU - w * 1.3).sin();
                let v3 = ((i_f + w * 2.0) * k3 * core::f32::consts::TAU).cos();

                let plasma = (v1 + v2 + v3 + 3.0) / 6.0; // 0..1
                let hue_shift = ((plasma * 60.0) as u8).wrapping_add(128);
                let col = lerp_color(base_color, hue_to_rgb(hue_shift), 0.35);
                *px = scale_color(col, plasma.clamp(0.0, 1.0));
            }
        }

        // Slot 7: Sand Cascade / Pour (Falling particles accumulating at the bottom)
        7 => {
            let cycle_time = (6.0 / speed_mult.abs().max(0.2)).clamp(1.5, 12.0);
            let progress = (t % cycle_time) / cycle_time; // 0..1
            let filled_len = (progress * count_f) as usize;

            let time_step = (t * 20.0 * speed_mult.abs()) as u32;

            for (i, px) in out.iter_mut().enumerate() {
                if i >= count.saturating_sub(filled_len) {
                    // Accumulated pile
                    *px = base_color;
                } else {
                    // Falling stream particles
                    let noise = spatial_noise(i, time_step);
                    if noise > 0.88 {
                        *px = scale_color(base_color, 0.9);
                    } else {
                        *px = RgbColor::default();
                    }
                }
            }
        }

        // Slot 8: Sine Wave Breath (Asymmetric biological inhalation and exhalation)
        8 => {
            let period = (4.0 / speed_mult.abs().max(0.2)).clamp(1.0, 8.0);
            let phase = (t % period) / period; // 0..1
            // Biological breath curve: quick inhale (0..0.35), slow exhale (0.35..1.0)
            let breath = if phase < 0.35 {
                (phase / 0.35 * core::f32::consts::FRAC_PI_2).sin()
            } else {
                ((1.0 - (phase - 0.35) / 0.65) * core::f32::consts::FRAC_PI_2).sin()
            };

            let spatial_k = (core::f32::consts::TAU / count_f) / size_norm;
            for (i, px) in out.iter_mut().enumerate() {
                let spatial_wave = ((i as f32 * spatial_k).sin() * 0.15) + 0.85;
                let brightness = (breath * spatial_wave).clamp(0.0, 1.0);
                *px = scale_color(base_color, brightness);
            }
        }

        // Slot 9: Heartbeat Pulse (Double-beat ECG waveform propagating from center)
        9 => {
            let bpm = (72.0 * speed_mult.abs().max(0.3)).clamp(40.0, 180.0);
            let beat_period = 60.0 / bpm;
            let beat_phase = (t % beat_period) / beat_period; // 0..1
            let center = count_f * 0.5;

            // ECG curve: 1st beat at 0..0.12, 2nd beat at 0.18..0.32, rest 0.32..1.0
            let pulse_intensity = if beat_phase < 0.12 {
                (beat_phase / 0.12 * core::f32::consts::PI).sin()
            } else if beat_phase > 0.18 && beat_phase < 0.32 {
                ((beat_phase - 0.18) / 0.14 * core::f32::consts::PI).sin() * 0.65
            } else {
                0.02
            };

            let wave_front = pulse_intensity * center;
            let wave_width = (size_norm * 3.0).max(1.0);

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                let wave_dist = (dist - wave_front).abs();
                let factor = (1.0 - (wave_dist / wave_width)).max(0.0) * pulse_intensity;
                *px = scale_color(base_color, factor.clamp(0.03, 1.0));
            }
        }

        // Slot 10: Campfire Organic Flicker (1/f chaotic flicker with hot white base)
        10 => {
            let time_step = (t * 25.0 * speed_mult.abs()) as u32;
            let hot_white = RgbColor { r: 255, g: 240, b: 200 };

            for (i, px) in out.iter_mut().enumerate() {
                let pos_ratio = i as f32 / count_f; // 0 is base, 1 is top
                let noise = spatial_noise(i, time_step);
                let flame_height = (1.0 - pos_ratio * (1.2 / size_norm)).max(0.0);
                let intensity = (flame_height * (0.6 + 0.4 * noise)).clamp(0.0, 1.0);

                let color = if pos_ratio < 0.25 {
                    lerp_color(base_color, hot_white, 1.0 - pos_ratio * 4.0)
                } else {
                    base_color
                };

                *px = scale_color(color, intensity);
            }
        }

        // Slot 11: Aurora Curtain (Undulating soft multi-layer curtains)
        11 => {
            let w1 = t * speed_mult * 0.8;
            let w2 = t * speed_mult * 1.4;
            let comp_color = RgbColor {
                r: 255u8.saturating_sub(base_color.r),
                g: 255u8.saturating_sub(base_color.g),
                b: 255u8.saturating_sub(base_color.b),
            };

            for (i, px) in out.iter_mut().enumerate() {
                let pos = i as f32 / count_f;
                let wave1 = ((pos * 5.0 / size_norm + w1).sin() + 1.0) * 0.5;
                let wave2 = ((pos * 9.0 / size_norm - w2).sin() + 1.0) * 0.5;

                let blend = (wave1 + wave2) * 0.5;
                let col = lerp_color(base_color, comp_color, blend * 0.4);
                *px = scale_color(col, blend * blend);
            }
        }

        // Slot 12: Electric Arc / Lightning (Random high-voltage discharge strikes)
        12 => {
            let strike_rate = (t * 12.0 * speed_mult.abs()) as u32;
            let noise = spatial_noise(0, strike_rate);
            let has_strike = noise > 0.82;

            if has_strike {
                let strike_pos = (spatial_noise(1, strike_rate) * count_f) as usize;
                let arc_radius = (size_norm * 4.0) as usize;
                let white = RgbColor { r: 255, g: 255, b: 255 };

                out.fill(RgbColor::default());
                for (i, px) in out.iter_mut().enumerate() {
                    let d = (i as isize - strike_pos as isize).unsigned_abs();
                    if d <= arc_radius {
                        let f = 1.0 - (d as f32 / (arc_radius as f32 + 1.0));
                        *px = lerp_color(base_color, white, f);
                    }
                }
            } else {
                // Dim remnant
                for px in out.iter_mut() {
                    *px = scale_color(base_color, 0.05);
                }
            }
        }

        // Slot 13: Firefly Swarm (Decoupled individual blinking agents)
        13 => {
            for (i, px) in out.iter_mut().enumerate() {
                let agent_seed = (i as u32).wrapping_mul(1234567);
                let freq = 0.5 + ((agent_seed & 0xFF) as f32 / 255.0) * 1.5 * speed_mult.abs();
                let phase_offset = ((agent_seed >> 8) & 0xFF) as f32 / 255.0 * core::f32::consts::TAU;

                let sine = (t * freq + phase_offset).sin();
                let intensity = (sine.max(0.0)).powi(4); // Sharp glow window

                *px = scale_color(base_color, intensity);
            }
        }

        // Slot 14: Neural Synapse (High-speed branching signal propagation)
        14 => {
            let burst_step = (t * 4.0 * speed_mult.abs()) as u32;
            let node_pos = (spatial_noise(2, burst_step) * count_f) as usize;
            let pulse_prog = ((t * 8.0 * speed_mult.abs()) % 1.0) * count_f * 0.4;

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as isize - node_pos as isize).unsigned_abs() as f32;
                let d_pulse = (dist - pulse_prog).abs();
                let factor = if d_pulse < (size_norm * 2.0) {
                    1.0 - (d_pulse / (size_norm * 2.0))
                } else {
                    0.0
                };
                *px = scale_color(base_color, factor);
            }
        }

        // Slot 15: Jellyfish Bioluminescence (Contraction pulse and slow fading bloom)
        15 => {
            let period = (3.0 / speed_mult.abs().max(0.2)).clamp(1.0, 6.0);
            let phase = (t % period) / period; // 0..1
            let center = count_f * 0.5;

            // Inward squeeze (0..0.3) -> Outward bloom (0.3..1.0)
            let (bloom_pos, brightness) = if phase < 0.3 {
                let r = (1.0 - (phase / 0.3)) * center;
                (r, 0.4 + (phase / 0.3) * 0.6)
            } else {
                let r = ((phase - 0.3) / 0.7) * center;
                let fade = 1.0 - ((phase - 0.3) / 0.7);
                (r, fade * fade)
            };

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                let delta = (dist - bloom_pos).abs();
                let factor = if delta < (size_norm * 4.0) {
                    (1.0 - (delta / (size_norm * 4.0))) * brightness
                } else {
                    0.0
                };
                *px = scale_color(base_color, factor);
            }
        }

        // Slot 16: Rainbow Spectrum Flow (Full-spectrum continuous wave)
        16 => {
            let offset = (t * speed_mult * 80.0) as u32;
            let step = ((256.0 / (count_f * size_norm).max(1.0)) * 256.0) as u32;
            for (i, px) in out.iter_mut().enumerate() {
                let hue = (((i as u32 * step) / 256).wrapping_add(offset)) as u8;
                *px = hue_to_rgb(hue);
            }
        }

        // Slot 17: Knight Rider / Scanner (Rebounding beam with deep easing and trails)
        17 => {
            let period = (2.0 / speed_mult.abs().max(0.2)).clamp(0.5, 5.0);
            let phase = (t % period) / period; // 0..1
            // Smooth sinusoidal motion across strip
            let head = ((phase * core::f32::consts::TAU).sin() * 0.5 + 0.5) * (count_f - 1.0);
            let tail_width = (size_norm * 3.5).max(1.0);

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - head).abs();
                let factor = (1.0 - (dist / tail_width)).max(0.0);
                *px = scale_color(base_color, factor * factor);
            }
        }

        // Slot 18: Curtain Center Wipe (Expanding and contracting theatrical curtain)
        18 => {
            let period = (3.0 / speed_mult.abs().max(0.2)).clamp(0.8, 6.0);
            let phase = (t % period) / period; // 0..1
            let center = count_f * 0.5;

            let open_len = if phase < 0.5 {
                (phase * 2.0) * center
            } else {
                (1.0 - (phase - 0.5) * 2.0) * center
            };

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                if dist <= open_len {
                    *px = base_color;
                } else {
                    *px = RgbColor::default();
                }
            }
        }

        // Slot 19: Strobe Dot Matrix / Cinema Flicker (Odd/even interlaced illusion)
        19 => {
            let freq = (t * 20.0 * speed_mult.abs()) as u32;
            let block_size = (size_norm * 2.0) as usize;
            let is_even_frame = (freq % 2) == 0;

            for (i, px) in out.iter_mut().enumerate() {
                let block_idx = i / block_size.max(1);
                let is_on = (block_idx % 2 == 0) == is_even_frame;
                *px = if is_on { base_color } else { RgbColor::default() };
            }
        }

        // Slot 20: Diamond Sparkle / Shimmer (Delicate pinpoint random twinkling)
        20 => {
            let time_step = (t * 30.0 * speed_mult.abs()) as u32;
            let white = RgbColor { r: 255, g: 255, b: 255 };

            for (i, px) in out.iter_mut().enumerate() {
                let noise = spatial_noise(i, time_step);
                let threshold = 1.0 - (0.08 * size_norm);
                if noise > threshold {
                    *px = white;
                } else {
                    *px = scale_color(base_color, 0.15);
                }
            }
        }

        // Slot 21: Sawtooth Treadmill (Accelerating linear conveyor ramps)
        21 => {
            let pitch = (count_f * 0.25 * size_norm).max(2.0);
            let offset = t * speed_mult * 3.0 * pitch;

            for (i, px) in out.iter_mut().enumerate() {
                let pos = (i as f32 + offset).rem_euclid(pitch);
                let ramp = pos / pitch; // 0..1 linear ramp
                *px = scale_color(base_color, ramp);
            }
        }

        // Slot 22: Digital Matrix Rain (Falling digital code drops with trails)
        22 => {
            let stream_count = 4;
            out.fill(RgbColor::default());

            for s in 0..stream_count {
                let stream_speed = 0.8 + (spatial_noise(s, 0) * 0.8);
                let drop_pos = ((t * speed_mult * stream_speed * 1.5 + (s as f32 * 0.25)).rem_euclid(1.0)) * count_f;
                let tail_len = (size_norm * count_f * 0.12).max(2.0);

                for (i, px) in out.iter_mut().enumerate() {
                    let d = (drop_pos - i as f32).rem_euclid(count_f);
                    if d < tail_len {
                        let f = 1.0 - (d / tail_len);
                        let col = scale_color(base_color, f * f);
                        *px = add_color(*px, col);
                    }
                }
            }
        }

        // Slot 23: Moiré Interference Grid (Dual opposing grids creating beating illusion)
        23 => {
            let k1 = (8.0 / size_norm) / count_f;
            let k2 = (9.5 / size_norm) / count_f;
            let w = t * speed_mult * 4.0;

            for (i, px) in out.iter_mut().enumerate() {
                let i_f = i as f32;
                let g1 = ((i_f * k1 * core::f32::consts::TAU + w).sin() + 1.0) * 0.5;
                let g2 = ((i_f * k2 * core::f32::consts::TAU - w).sin() + 1.0) * 0.5;
                let moire = g1 * g2;
                *px = scale_color(base_color, moire);
            }
        }

        // Slots 24..=31: Fallback / Future Reserve
        _ => {
            out.fill(base_color);
        }
    }
}
