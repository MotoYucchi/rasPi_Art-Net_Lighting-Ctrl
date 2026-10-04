//! Rendering engine: pattern generators, audio-modulation, dimmer curves,
//! strobe gates, power limiter (ABL), gamma correction, color order transformation,
//! and standalone rig-check test patterns.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod audio_patterns;
pub mod patterns;

use audio_proto::AudioFeaturePacket;
use core::time::Duration;
use fixture::{RenderCommand, StrobeMode, WhiteMode};

/// 8-bit RGB color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Color order mapping for WS2811 / WS2812B strips
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorOrder {
    Rgb,
    Rbg,
    Grb, // WS2812B standard
    Gbr,
    Brg, // WS2811 common variant
    Bgr,
}

impl ColorOrder {
    #[inline]
    pub fn pack(&self, color: RgbColor) -> [u8; 3] {
        match self {
            ColorOrder::Rgb => [color.r, color.g, color.b],
            ColorOrder::Rbg => [color.r, color.b, color.g],
            ColorOrder::Grb => [color.g, color.r, color.b],
            ColorOrder::Gbr => [color.g, color.b, color.r],
            ColorOrder::Brg => [color.b, color.r, color.g],
            ColorOrder::Bgr => [color.b, color.g, color.r],
        }
    }
}

/// Rig-check self-test pattern mode for standalone inspection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestPatternMode {
    /// Cycle: Red 2s -> Green 2s -> Blue 2s -> White 2s
    RgbwCycle,
    /// Single pixel white running from first to last pixel
    AddressWalker,
    /// 100% Full White soak (checks power supply, voltage drops, fuses)
    PowerSoak100,
    /// 40Hz High rate strobe (checks data signal noise margin)
    HighRateStrobe40Hz,
}

/// Precomputed 8-bit gamma 2.2 correction table (256 entries)
pub const GAMMA_TABLE: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2,
    3, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 6,
    6, 7, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10, 11, 11, 11, 12,
    12, 13, 13, 13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19,
    20, 20, 21, 22, 22, 23, 23, 24, 25, 25, 26, 26, 27, 28, 28, 29,
    30, 30, 31, 32, 33, 33, 34, 35, 35, 36, 37, 38, 39, 39, 40, 41,
    42, 43, 43, 44, 45, 46, 47, 48, 49, 49, 50, 51, 52, 53, 54, 55,
    56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71,
    73, 74, 75, 76, 77, 78, 79, 81, 82, 83, 84, 85, 87, 88, 89, 90,
    91, 93, 94, 95, 97, 98, 99, 100, 102, 103, 105, 106, 107, 109, 110, 111,
    113, 114, 116, 117, 119, 120, 121, 123, 124, 126, 127, 129, 130, 132, 133, 135,
    137, 138, 140, 141, 143, 145, 146, 148, 149, 151, 153, 154, 156, 158, 159, 161,
    163, 165, 166, 168, 170, 172, 173, 175, 177, 179, 181, 182, 184, 186, 188, 190,
    192, 194, 196, 197, 199, 201, 203, 205, 207, 209, 211, 213, 215, 217, 219, 221,
    223, 225, 227, 229, 231, 234, 236, 238, 240, 242, 244, 246, 248, 251, 253, 255,
];

/// Pixel rendering pipeline
pub struct RenderEngine {
    color_order: ColorOrder,
    enable_gamma: bool,
    max_current_ma: u32,
    ma_per_color_channel: u32, // e.g. 20mA for WS2811 / WS2812B at 255
}

impl RenderEngine {
    pub fn new(color_order: ColorOrder, max_current_ma: u32) -> Self {
        Self {
            color_order,
            enable_gamma: true,
            max_current_ma,
            ma_per_color_channel: 20,
        }
    }

    /// Primary render method for a fixture into the provided output pixel buffer
    pub fn render(
        &self,
        command: &RenderCommand,
        audio: Option<&AudioFeaturePacket>,
        elapsed: Duration,
        out_pixels: &mut [RgbColor],
    ) {
        let px_count = out_pixels.len();
        if px_count == 0 {
            return;
        }

        match command {
            RenderCommand::Preset {
                master_dimmer,
                strobe,
                color,
                pattern_id,
                audio_mode_id,
                speed,
                size,
                white_mode,
            } => {
                // 1. Resolve base RGB color from RGBW
                let base_rgb = match white_mode {
                    WhiteMode::RgbBlend => {
                        let w = color.w;
                        RgbColor {
                            r: color.r.saturating_add(w),
                            g: color.g.saturating_add(w),
                            b: color.b.saturating_add(w),
                        }
                    }
                    WhiteMode::Ignore => RgbColor {
                        r: color.r,
                        g: color.g,
                        b: color.b,
                    },
                };

                // 2. Render Pattern or Audio Sync Pattern
                // Priority: audio_mode_id slot >= 1 (DMX >= 8) takes precedence if audio is present
                let audio_slot = (*audio_mode_id) / 8;
                if audio_slot > 0 {
                    if let Some(aud) = audio {
                        self.render_audio_pattern(*audio_mode_id, aud, base_rgb, elapsed, out_pixels);
                    } else {
                        self.render_pattern(*pattern_id, base_rgb, *speed, *size, elapsed, out_pixels);
                    }
                } else {
                    self.render_pattern(*pattern_id, base_rgb, *speed, *size, elapsed, out_pixels);
                }

                // 3. Strobe gate
                let strobe_open = match strobe {
                    StrobeMode::Open => true,
                    StrobeMode::Rate(hz) => {
                        let period_secs = 1.0 / hz.max(0.1);
                        let cycle_pos = (elapsed.as_secs_f32() % period_secs) / period_secs;
                        cycle_pos < 0.5
                    }
                };

                // 4. Dimmer scaling + Strobe gating
                let effective_dimmer = if strobe_open { *master_dimmer } else { 0 };
                for px in out_pixels.iter_mut() {
                    px.r = (((px.r as u32) * (effective_dimmer as u32)) / 255) as u8;
                    px.g = (((px.g as u32) * (effective_dimmer as u32)) / 255) as u8;
                    px.b = (((px.b as u32) * (effective_dimmer as u32)) / 255) as u8;
                }
            }
            RenderCommand::DirectRgb {
                pixel_count,
                master_dimmer,
                strobe,
                start_universe,
                start_address,
                span_mode,
                store,
            } => {
                let count = (*pixel_count).min(px_count);
                let strobe_open = match strobe {
                    StrobeMode::Open => true,
                    StrobeMode::Rate(hz) => {
                        let period_secs = 1.0 / hz.max(0.1);
                        let cycle_pos = (elapsed.as_secs_f32() % period_secs) / period_secs;
                        cycle_pos < 0.5
                    }
                };

                let effective_dimmer = if strobe_open { *master_dimmer } else { 0 };

                for (i, px) in out_pixels.iter_mut().take(count).enumerate() {
                    let (r, g, b) = match span_mode {
                        fixture::UniverseSpanMode::Per170Px => {
                            let univ_offset = (i / 170) as u16;
                            let px_in_univ = i % 170;
                            let target_univ = start_universe + univ_offset;
                            let base_ch = (px_in_univ * 3) as u16 + start_address;

                            if let Some(state) = store.get(target_univ) {
                                (
                                    state.get_channel(base_ch),
                                    state.get_channel(base_ch + 1),
                                    state.get_channel(base_ch + 2),
                                )
                            } else {
                                (0, 0, 0)
                            }
                        }
                        fixture::UniverseSpanMode::Packed => {
                            let total_ch_offset = (i * 3) as u16 + (start_address - 1);
                            let univ_offset = total_ch_offset / 512;
                            let ch1 = (total_ch_offset % 512) + 1;
                            let target_univ = start_universe + univ_offset;

                            if let Some(state) = store.get(target_univ) {
                                (
                                    state.get_channel(ch1),
                                    state.get_channel(ch1 + 1),
                                    state.get_channel(ch1 + 2),
                                )
                            } else {
                                (0, 0, 0)
                            }
                        }
                    };

                    px.r = (((r as u32) * (effective_dimmer as u32)) / 255) as u8;
                    px.g = (((g as u32) * (effective_dimmer as u32)) / 255) as u8;
                    px.b = (((b as u32) * (effective_dimmer as u32)) / 255) as u8;
                }
                for px in out_pixels.iter_mut().skip(count) {
                    *px = RgbColor::default();
                }
            }
        }

        // 5. Automatic Brightness Limiter (ABL)
        self.apply_power_limiter(out_pixels);

        // 6. Gamma correction
        if self.enable_gamma {
            for px in out_pixels.iter_mut() {
                px.r = GAMMA_TABLE[px.r as usize];
                px.g = GAMMA_TABLE[px.g as usize];
                px.b = GAMMA_TABLE[px.b as usize];
            }
        }
    }

    /// Internal pattern generator
    pub fn render_pattern(
        &self,
        pattern_id: u8,
        base_color: RgbColor,
        speed: u8,
        size: u8,
        elapsed: Duration,
        out: &mut [RgbColor],
    ) {
        patterns::render_pattern(pattern_id, base_color, speed, size, elapsed, out);
    }

    /// Audio-driven pattern generator
    pub fn render_audio_pattern(
        &self,
        mode_id: u8,
        audio: &AudioFeaturePacket,
        base_color: RgbColor,
        elapsed: Duration,
        out: &mut [RgbColor],
    ) {
        audio_patterns::render_audio_pattern(mode_id, audio, base_color, elapsed, out);
    }

    /// Render standalone rig-check test pattern
    pub fn render_test_pattern(
        &self,
        mode: TestPatternMode,
        elapsed: Duration,
        out: &mut [RgbColor],
    ) {
        let count = out.len();
        if count == 0 {
            return;
        }

        match mode {
            TestPatternMode::RgbwCycle => {
                let secs = (elapsed.as_secs() % 8) as usize;
                let c = match secs {
                    0..=1 => RgbColor { r: 255, g: 0, b: 0 },   // Red
                    2..=3 => RgbColor { r: 0, g: 255, b: 0 },   // Green
                    4..=5 => RgbColor { r: 0, g: 0, b: 255 },   // Blue
                    _ => RgbColor { r: 255, g: 255, b: 255 }, // White
                };
                out.fill(c);
            }
            TestPatternMode::AddressWalker => {
                let idx = ((elapsed.as_millis() / 50) as usize) % count;
                out.fill(RgbColor::default());
                out[idx] = RgbColor { r: 255, g: 255, b: 255 };
            }
            TestPatternMode::PowerSoak100 => {
                out.fill(RgbColor { r: 255, g: 255, b: 255 });
            }
            TestPatternMode::HighRateStrobe40Hz => {
                let on = (elapsed.as_millis() % 25) < 12;
                if on {
                    out.fill(RgbColor { r: 255, g: 255, b: 255 });
                } else {
                    out.fill(RgbColor::default());
                }
            }
        }

        self.apply_power_limiter(out);
    }

    /// Automatic Brightness Limiter (ABL)
    fn apply_power_limiter(&self, pixels: &mut [RgbColor]) {
        if self.max_current_ma == 0 {
            return;
        }

        let mut total_channel_sum: u32 = 0;
        for px in pixels.iter() {
            total_channel_sum += px.r as u32 + px.g as u32 + px.b as u32;
        }

        // Estimated current in mA: (sum / 255) * ma_per_channel
        let est_current_ma = (total_channel_sum * self.ma_per_color_channel) / 255;
        if est_current_ma > self.max_current_ma {
            let scale_factor = (self.max_current_ma as u64 * 255) / est_current_ma as u64;
            for px in pixels.iter_mut() {
                px.r = ((px.r as u64 * scale_factor) / 255) as u8;
                px.g = ((px.g as u64 * scale_factor) / 255) as u8;
                px.b = ((px.b as u64 * scale_factor) / 255) as u8;
            }
        }
    }

    /// Convert RGB pixels into byte stream according to strip's ColorOrder
    pub fn pack_bytes(&self, pixels: &[RgbColor], out_bytes: &mut [u8]) {
        for (i, px) in pixels.iter().enumerate() {
            let packed = self.color_order.pack(*px);
            let offset = i * 3;
            if offset + 2 < out_bytes.len() {
                out_bytes[offset] = packed[0];
                out_bytes[offset + 1] = packed[1];
                out_bytes[offset + 2] = packed[2];
            }
        }
    }
}

/// Helper function to generate an RGB color from an 8-bit hue wheel (0..255)
pub fn hue_to_rgb(hue: u8) -> RgbColor {
    let region = hue / 43;
    let remainder = (hue - (region * 43)) * 6;

    let p = 0u8;
    let q = 255 - remainder;
    let t = remainder;

    match region {
        0 => RgbColor { r: 255, g: t, b: p },
        1 => RgbColor { r: q, g: 255, b: p },
        2 => RgbColor { r: p, g: 255, b: t },
        3 => RgbColor { r: p, g: q, b: 255 },
        4 => RgbColor { r: t, g: p, b: 255 },
        _ => RgbColor { r: 255, g: p, b: q },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_order_packing() {
        let px = RgbColor { r: 10, g: 20, b: 30 };
        assert_eq!(ColorOrder::Rgb.pack(px), [10, 20, 30]);
        assert_eq!(ColorOrder::Grb.pack(px), [20, 10, 30]);
        assert_eq!(ColorOrder::Brg.pack(px), [30, 10, 20]);
    }

    #[test]
    fn test_power_limiter_dimming() {
        let engine = RenderEngine::new(ColorOrder::Brg, 1000); // 1000mA max
        let mut pixels = [RgbColor { r: 255, g: 255, b: 255 }; 100]; // 100 * 3 * 20mA = 6000mA
        engine.apply_power_limiter(&mut pixels);

        // Current should be scaled down to approx 1/6th
        assert!(pixels[0].r < 50);
    }

    #[test]
    fn test_all_24_lighting_patterns_sweep() {
        let engine = RenderEngine::new(ColorOrder::Grb, 5000);
        let base_color = RgbColor { r: 200, g: 100, b: 50 };
        let pixel_counts = [1, 5, 60, 170, 300];
        let speeds = [0, 64, 128, 192, 255];
        let sizes = [0, 32, 128, 200, 255];
        let times = [
            Duration::ZERO,
            Duration::from_millis(500),
            Duration::from_secs(120),
            Duration::from_secs(3600), // 1 hour elapsed
        ];

        for &px_count in &pixel_counts {
            let mut out = vec![RgbColor::default(); px_count];
            // Test each slot (0..=31) via representative DMX values
            for slot in 0..=31 {
                let dmx_val = (slot * 8).min(255) as u8;
                for &spd in &speeds {
                    for &sz in &sizes {
                        for &elapsed in &times {
                            engine.render_pattern(dmx_val, base_color, spd, sz, elapsed, &mut out);
                            // Verify output doesn't panic and is valid
                            assert_eq!(out.len(), px_count);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_all_24_audio_patterns_sweep() {
        let engine = RenderEngine::new(ColorOrder::Grb, 5000);
        let base_color = RgbColor { r: 150, g: 75, b: 220 };
        let pixel_counts = [1, 7, 50, 170];
        let times = [Duration::ZERO, Duration::from_millis(250), Duration::from_secs(60)];

        // Test with various audio packets
        let silent_pkt = AudioFeaturePacket::default();
        let mut full_pkt = AudioFeaturePacket::default();
        full_pkt.rms_level = 255;
        full_pkt.peak_level = 255;
        full_pkt.bands = [255; 7];
        full_pkt.triggers = 0xFF; // All triggers active
        full_pkt.spectral_centroid = 200;
        full_pkt.beat_phase = 64;

        let packets = [silent_pkt, full_pkt];

        for &px_count in &pixel_counts {
            let mut out = vec![RgbColor::default(); px_count];
            for slot in 0..=31 {
                let dmx_val = (slot * 8).min(255) as u8;
                for pkt in &packets {
                    for &elapsed in &times {
                        engine.render_audio_pattern(dmx_val, pkt, base_color, elapsed, &mut out);
                        assert_eq!(out.len(), px_count);
                    }
                }
            }
        }
    }

    #[test]
    fn test_audio_priority_and_fallback_mechanism() {
        let engine = RenderEngine::new(ColorOrder::Grb, 5000);
        let mut out = vec![RgbColor::default(); 50];

        let cmd_audio_off = fixture::RenderCommand::Preset {
            master_dimmer: 255,
            strobe: StrobeMode::Open,
            color: fixture::ColorRgbw { r: 255, g: 0, b: 0, w: 0 },
            pattern_id: 0, // Static Red
            audio_mode_id: 0, // Audio OFF (slot 0)
            speed: 128,
            size: 128,
            white_mode: WhiteMode::Ignore,
        };

        // When audio is OFF (slot 0), static red should render
        engine.render(&cmd_audio_off, None, Duration::ZERO, &mut out);
        assert_eq!(out[0], RgbColor { r: 255, g: 0, b: 0 });

        // When audio is ON (slot 2: Kick Pump = DMX 16) with kick active
        let mut kick_audio = AudioFeaturePacket::default();
        kick_audio.triggers = audio_proto::TRIGGER_KICK;
        let cmd_audio_on = fixture::RenderCommand::Preset {
            master_dimmer: 255,
            strobe: StrobeMode::Open,
            color: fixture::ColorRgbw { r: 255, g: 0, b: 0, w: 0 },
            pattern_id: 0, // Static Red
            audio_mode_id: 16, // Slot 2 (Kick Pump Flash)
            speed: 128,
            size: 128,
            white_mode: WhiteMode::Ignore,
        };

        engine.render(&cmd_audio_on, Some(&kick_audio), Duration::ZERO, &mut out);
        // Kick pump flashes full white on kick
        assert_eq!(out[0], RgbColor { r: 255, g: 255, b: 255 });

        // When audio is ON (audio_mode_id = 16) but audio stream is LOST (None):
        // Engine must safely fall back to static red without panic
        engine.render(&cmd_audio_on, None, Duration::ZERO, &mut out);
        assert_eq!(out[0], RgbColor { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_empty_pixel_buffer_safety() {
        let engine = RenderEngine::new(ColorOrder::Grb, 5000);
        let mut empty_out: [RgbColor; 0] = [];
        engine.render_pattern(16, RgbColor::default(), 128, 128, Duration::ZERO, &mut empty_out);
        engine.render_audio_pattern(16, &AudioFeaturePacket::default(), RgbColor::default(), Duration::ZERO, &mut empty_out);
        assert_eq!(empty_out.len(), 0);
    }
}
