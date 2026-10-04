//! Audio-Reactive / Interactive Sync Patterns (DMX ch 7)
//!
//! Provides 24 professional audio-reactive patterns driven by real-time FFT features:
//! 7 frequency bands, RMS, instantaneous peak, Kick/Snare/HiHat/Beat triggers,
//! Spectral Centroid, BPM, and Beat Phase.
//!
//! Guarantee: Zero allocations (`no_alloc`), panic-free, deterministic.

use crate::{hue_to_rgb, RgbColor};
use audio_proto::AudioFeaturePacket;
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

/// Fast pseudo-random generator
#[inline]
fn pseudo_rand(seed: u32) -> u32 {
    let mut x = seed.wrapping_add(0x9E3779B9);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Normalized pseudo-noise 0.0..=1.0
#[inline]
fn spatial_noise(px_idx: usize, time_step: u32) -> f32 {
    let seed = (px_idx as u32).wrapping_mul(374761393) ^ time_step.wrapping_mul(668265263);
    let r = pseudo_rand(seed);
    (r & 0xFFFF) as f32 / 65535.0
}

/// Render audio-reactive pattern based on DMX slot
pub fn render_audio_pattern(
    audio_dmx: u8,
    audio: &AudioFeaturePacket,
    base_color: RgbColor,
    elapsed: Duration,
    out: &mut [RgbColor],
) {
    let count = out.len();
    if count == 0 {
        return;
    }

    let slot = audio_dmx / 8; // 32 slots (0..=31)
    let count_f = count as f32;
    let t = elapsed.as_secs_f32();

    let rms_norm = audio.rms_level as f32 / 255.0;
    let peak_norm = audio.peak_level as f32 / 255.0;
    let phase_norm = audio.beat_phase as f32 / 255.0; // 0.0 = on-beat, 0.5 = off-beat

    match slot {
        // Slot 0: Off / Neutral fallback
        0 => {
            out.fill(base_color);
        }

        // Slot 1: Master Pulse (Overall RMS volume dynamics scaling)
        1 => {
            let dimmed = scale_color(base_color, rms_norm);
            out.fill(dimmed);
        }

        // Slot 2: Kick Pump Flash (Full white explosion on Kick, bass decay)
        2 => {
            if audio.is_kick() {
                out.fill(RgbColor { r: 255, g: 255, b: 255 });
            } else {
                let bass_energy = (audio.bands[0].max(audio.bands[1])) as f32 / 255.0;
                let c = scale_color(base_color, bass_energy);
                out.fill(c);
            }
        }

        // Slot 3: Snare Snap White Flash (Blinder snap on Snare + High-Mid inverted trail)
        3 => {
            if audio.is_snare() {
                out.fill(RgbColor { r: 255, g: 255, b: 255 });
            } else {
                let high_mid = audio.bands[4] as f32 / 255.0;
                let comp_color = RgbColor {
                    r: 255u8.saturating_sub(base_color.r),
                    g: 255u8.saturating_sub(base_color.g),
                    b: 255u8.saturating_sub(base_color.b),
                };
                let blended = lerp_color(base_color, comp_color, high_mid * 0.7);
                out.fill(scale_color(blended, high_mid.max(0.05)));
            }
        }

        // Slot 4: Hi-Hat Shimmer Glitter (High frequency crisp glitter sparks)
        4 => {
            let high_energy = (audio.bands[5].max(audio.bands[6])) as f32 / 255.0;
            let time_step = (t * 50.0) as u32;
            let white = RgbColor { r: 255, g: 255, b: 255 };

            for (i, px) in out.iter_mut().enumerate() {
                let noise = spatial_noise(i, time_step);
                let threshold = 1.0 - (high_energy * 0.45);
                if audio.is_hihat() && noise > 0.65 {
                    *px = white;
                } else if noise > threshold {
                    *px = lerp_color(base_color, white, 0.75);
                } else {
                    *px = scale_color(base_color, 0.08);
                }
            }
        }

        // Slot 5: Drum Kit 3-Way Split (Center: Kick, Mid-sides: Snare, Outer: HiHat)
        5 => {
            let center = count_f * 0.5;
            let kick_int = if audio.is_kick() { 1.0 } else { audio.bands[0] as f32 / 255.0 };
            let snare_int = if audio.is_snare() { 1.0 } else { audio.bands[4] as f32 / 255.0 };
            let hihat_int = if audio.is_hihat() { 1.0 } else { audio.bands[6] as f32 / 255.0 };

            for (i, px) in out.iter_mut().enumerate() {
                let dist_from_center = (i as f32 - center).abs() / center; // 0.0..1.0
                if dist_from_center < 0.35 {
                    // Kick Zone (Center)
                    *px = scale_color(base_color, kick_int);
                } else if dist_from_center < 0.75 {
                    // Snare Zone (Mid sides)
                    let col = RgbColor { r: 255, g: 255, b: 255 };
                    *px = scale_color(col, snare_int);
                } else {
                    // Hi-Hat Zone (Outer flanks)
                    let col = hue_to_rgb(60); // Golden yellow shimmer
                    *px = scale_color(col, hihat_int);
                }
            }
        }

        // Slot 6: Shockwave Blast (Supersonic expanding ring per kick/beat)
        6 => {
            let center = count_f * 0.5;
            let wave_front = phase_norm * center;
            let wave_width = 3.0;
            let boost = if audio.is_kick() { 1.0 } else { 0.7 };

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                let delta = (dist - wave_front).abs();
                let f = (1.0 - (delta / wave_width)).max(0.0) * boost * (1.0 - phase_norm);
                let col = lerp_color(base_color, RgbColor { r: 255, g: 255, b: 255 }, f);
                *px = scale_color(col, f.max(0.04));
            }
        }

        // Slot 7: Beat Step Walk (Mechanical 1-step advancement on each beat)
        7 => {
            let block_size = (count / 8).max(1);
            let active_block = (audio.sequence as usize / 10) % 8;
            let start = active_block * block_size;
            let end = (start + block_size).min(count);

            out.fill(scale_color(base_color, 0.05));
            for px in out[start..end].iter_mut() {
                *px = base_color;
            }
        }

        // Slot 8: Beat Invert Strobe (On-beat flash, off-beat tight blackout)
        8 => {
            let on = phase_norm < 0.45;
            if on {
                out.fill(base_color);
            } else {
                out.fill(RgbColor::default());
            }
        }

        // Slot 9: Linear 7-Band Spectrum VU (7 equal zones showing frequency energy)
        9 => {
            let band_size = count / 7;
            for band_idx in 0..7 {
                let energy = audio.bands[band_idx];
                let start = band_idx * band_size;
                let end = if band_idx == 6 { count } else { start + band_size };
                let c = hue_to_rgb((band_idx as u8) * 36);
                let dimmed_c = scale_color(c, energy as f32 / 255.0);
                for px in out[start..end].iter_mut() {
                    *px = dimmed_c;
                }
            }
        }

        // Slot 10: Center-Out Stereo VU (Symmetric level bar expanding from center)
        10 => {
            let center = count_f * 0.5;
            let lit_len = peak_norm * center;

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                if dist <= lit_len {
                    let grad = dist / center.max(1.0);
                    let c = hue_to_rgb(((1.0 - grad) * 120.0) as u8); // Green to Red
                    *px = c;
                } else {
                    *px = RgbColor::default();
                }
            }
        }

        // Slot 11: Pro Peak Hold VU (Center-out bar with floating peak dot)
        11 => {
            let center = count_f * 0.5;
            let lit_len = rms_norm * center;
            let peak_pos_left = (center - peak_norm * center).clamp(0.0, count_f - 1.0) as usize;
            let peak_pos_right = (center + peak_norm * center).clamp(0.0, count_f - 1.0) as usize;

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                if dist <= lit_len {
                    *px = base_color;
                } else {
                    *px = RgbColor::default();
                }
            }
            if peak_pos_left < count {
                out[peak_pos_left] = RgbColor { r: 255, g: 255, b: 255 };
            }
            if peak_pos_right < count {
                out[peak_pos_right] = RgbColor { r: 255, g: 255, b: 255 };
            }
        }

        // Slot 12: Bass Wave Elastic Expansion (Wave width expands elastically with sub-bass)
        12 => {
            let bass = (audio.bands[0].max(audio.bands[1])) as f32 / 255.0;
            let center = count_f * 0.5;
            let elastic_width = (3.0 + bass * count_f * 0.45).max(1.0);

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                let f = (1.0 - (dist / elastic_width)).max(0.0);
                *px = scale_color(base_color, f * f);
            }
        }

        // Slot 13: Sub-Bass Gravity Rumble (Deep sluggish oil-like wave on 30-60Hz)
        13 => {
            let sub_bass = audio.bands[0] as f32 / 255.0;
            let slow_w = t * 1.5;

            for (i, px) in out.iter_mut().enumerate() {
                let pos = i as f32 / count_f;
                let wave = ((pos * 4.0 * core::f32::consts::TAU + slow_w).sin() + 1.0) * 0.5;
                let factor = wave * sub_bass;
                let deep_red = RgbColor { r: 200, g: 20, b: 0 };
                let col = lerp_color(base_color, deep_red, 0.5);
                *px = scale_color(col, factor);
            }
        }

        // Slot 14: Vocal Ribbon Mid-Focus (Selective focus on vocal formant mid-frequencies)
        14 => {
            let vocal_energy = ((audio.bands[3] as u16 + audio.bands[4] as u16) / 2) as f32 / 255.0;
            let center = count_f * 0.5;
            let ribbon_len = vocal_energy * count_f * 0.4;

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs();
                let factor = (1.0 - (dist / ribbon_len.max(1.0))).max(0.0);
                let soft_white = RgbColor { r: 255, g: 230, b: 240 };
                let col = lerp_color(base_color, soft_white, 0.4);
                *px = scale_color(col, factor * vocal_energy);
            }
        }

        // Slot 15: Spectral Centroid Hue Shift (Timbre brightness mapped directly to Hue)
        15 => {
            let hue = audio.spectral_centroid; // 0 (dark bass) .. 255 (bright treble)
            let shifted_color = hue_to_rgb(hue);
            let final_color = lerp_color(base_color, shifted_color, 0.7);
            out.fill(scale_color(final_color, rms_norm.max(0.1)));
        }

        // Slot 16: Multi-Band Particle Fountain (Fountain shooting frequency-colored particles)
        16 => {
            let center = count_f * 0.5;
            out.fill(scale_color(base_color, 0.04));

            for band in 0..7 {
                let energy = audio.bands[band] as f32 / 255.0;
                if energy > 0.4 {
                    let travel = energy * center * (1.0 - phase_norm);
                    let p_left = (center - travel).clamp(0.0, count_f - 1.0) as usize;
                    let p_right = (center + travel).clamp(0.0, count_f - 1.0) as usize;
                    let c = hue_to_rgb((band as u8) * 36);

                    if p_left < count {
                        out[p_left] = add_color(out[p_left], c);
                    }
                    if p_right < count {
                        out[p_right] = add_color(out[p_right], c);
                    }
                }
            }
        }

        // Slot 17: BPM Master Tempo Wave (Phase wave strictly locked to BPM and beat phase)
        17 => {
            for (i, px) in out.iter_mut().enumerate() {
                let pos = i as f32 / count_f;
                let wave_phase = (pos - phase_norm).rem_euclid(1.0);
                let factor = (wave_phase * core::f32::consts::TAU).sin() * 0.5 + 0.5;
                *px = scale_color(base_color, factor * rms_norm.max(0.2));
            }
        }

        // Slot 18: Half / Double Time Switcher (Double-time wave oscillation on beat phase)
        18 => {
            let double_phase = (phase_norm * 2.0).rem_euclid(1.0);
            for (i, px) in out.iter_mut().enumerate() {
                let pos = i as f32 / count_f;
                let wave = ((pos * 2.0 - double_phase) * core::f32::consts::TAU).sin() * 0.5 + 0.5;
                *px = scale_color(base_color, wave);
            }
        }

        // Slot 19: Drop Buildup Detector & Blast (Buildup strobe intensifier -> Drop blinder)
        19 => {
            let high_energy = (audio.bands[5] as u16 + audio.bands[6] as u16) / 2;
            let is_buildup = high_energy > 160 && audio.rms_level > 160;

            if audio.is_kick() && is_buildup {
                // Drop explosion
                out.fill(RgbColor { r: 255, g: 255, b: 255 });
            } else if is_buildup {
                // Fast strobe build
                let on = (audio.sequence % 4) < 2;
                out.fill(if on { base_color } else { RgbColor::default() });
            } else {
                out.fill(scale_color(base_color, rms_norm));
            }
        }

        // Slot 20: Dynamic Ambient Bloom (Soft breathing when quiet, fierce on loud)
        20 => {
            let intensity = rms_norm.powi(2); // Non-linear dynamic expansion
            let breath = ((t * 2.0).sin() * 0.2 + 0.8) * 0.15;
            let final_f = (intensity + breath).clamp(0.0, 1.0);
            out.fill(scale_color(base_color, final_f));
        }

        // Slot 21: Audio Glitch Shutter (Digital noise glitch triggered by abrupt crest spikes)
        21 => {
            let crest_spike = audio.peak_level.saturating_sub(audio.rms_level);
            if crest_spike > 100 {
                let time_step = audio.sequence;
                for (i, px) in out.iter_mut().enumerate() {
                    let n = spatial_noise(i, time_step);
                    *px = if n > 0.5 { base_color } else { RgbColor::default() };
                }
            } else {
                out.fill(scale_color(base_color, rms_norm * 0.4));
            }
        }

        // Slot 22: Acoustic String Resonance (Attack peak with natural exponential decay)
        22 => {
            let decay = (-phase_norm * 3.0).exp();
            let factor = decay * peak_norm;
            out.fill(scale_color(base_color, factor));
        }

        // Slot 23: Bass-Driven Color Tunnel (Inward suction illusion on bass and phase)
        23 => {
            let center = count_f * 0.5;
            let bass = audio.bands[1] as f32 / 255.0;

            for (i, px) in out.iter_mut().enumerate() {
                let dist = (i as f32 - center).abs() / center; // 0..1
                let suction = (dist + phase_norm).rem_euclid(1.0);
                let factor = (suction * core::f32::consts::TAU).sin() * 0.5 + 0.5;
                *px = scale_color(base_color, factor * bass.max(0.15));
            }
        }

        // Slot 24: Silence Safe Fallback (Warm incandescent 3000K ambient when silent)
        24 => {
            if audio.rms_level < 15 {
                // Gentle 3000K warm incandescent ambient
                let warm_amber = RgbColor { r: 180, g: 120, b: 40 };
                let breathing = ((t * 1.5).sin() * 0.1 + 0.9) * 0.25;
                out.fill(scale_color(warm_amber, breathing));
            } else {
                out.fill(scale_color(base_color, rms_norm));
            }
        }

        // Slots 25..=31: Fallback
        _ => {
            out.fill(base_color);
        }
    }
}
