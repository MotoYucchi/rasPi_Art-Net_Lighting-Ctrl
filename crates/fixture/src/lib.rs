//! Lighting fixture personalities and channel interpretations.
//!
//! Includes Preset 7ch (DMX address conservation mode), Standard 12ch,
//! and Pixel Direct RGB modes with multi-universe support.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dmx_universe::UniverseStore;

/// Strobe mode interpreted from DMX channel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrobeMode {
    Open,
    /// Linear strobe from min (e.g. 1Hz) to max (e.g. 25Hz)
    Rate(f32),
}

/// Base color RGBW
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ColorRgbw {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
}

/// Strategy for handling the White channel on pure RGB strips (like WS2811)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteMode {
    /// Blend White into R, G, B components evenly or with color temperature
    RgbBlend,
    /// Ignore the White channel
    Ignore,
}

/// Boundary configuration for multi-universe pixel direct mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniverseSpanMode {
    /// Standard: 170 pixels per universe (510 channels used, 511-512 skipped)
    Per170Px,
    /// Packed: fully pack 512 channels, pixel splits across universe boundary
    Packed,
}

/// Fixture personality type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Personality {
    /// 7 channels: Dimmer/Strobe, R, G, B, W, Pattern, AudioSync
    Preset7ch,
    /// 12 channels: Dimmer, Strobe, R, G, B, W, Pattern, Speed, Size, AudioMode, Gain, Ctrl
    Standard12ch,
    /// Direct 3 channels per pixel (RGB), spanning across universes
    PixelDirectRgb {
        start_universe: u16,
        start_address: u16,
        pixel_count: usize,
        span_mode: UniverseSpanMode,
    },
}

/// High-level control command output for rendering engine
#[derive(Debug, Clone)]
pub enum RenderCommand<'a> {
    /// Parametric preset rendering (Preset 7ch / Standard 12ch)
    Preset {
        master_dimmer: u8,
        strobe: StrobeMode,
        color: ColorRgbw,
        pattern_id: u8,
        audio_mode_id: u8,
        speed: u8,
        size: u8,
        white_mode: WhiteMode,
    },
    /// Direct per-pixel RGB stream reading from universe store
    DirectRgb {
        pixel_count: usize,
        master_dimmer: u8,
        strobe: StrobeMode,
        start_universe: u16,
        start_address: u16,
        span_mode: UniverseSpanMode,
        store: &'a UniverseStore,
    },
}

/// Fixture definition binding personality and DMX addressing
#[derive(Debug, Clone)]
pub struct FixtureConfig {
    pub personality: Personality,
    pub universe: u16,
    pub address: u16, // 1..=512
    pub white_mode: WhiteMode,
}

impl FixtureConfig {
    pub fn interpret<'a>(
        &self,
        store: &'a UniverseStore,
        pixel_count: usize,
    ) -> RenderCommand<'a> {
        match self.personality {
            Personality::Preset7ch => {
                let u = store.get(self.universe);
                let get_ch = |offset: u16| -> u8 {
                    if let Some(state) = u {
                        let ch = self.address.saturating_add(offset);
                        state.get_channel(ch)
                    } else {
                        0
                    }
                };

                let ch1 = get_ch(0);
                let (master_dimmer, strobe) = if ch1 <= 7 {
                    (0u8, StrobeMode::Open)
                } else if ch1 <= 134 {
                    let val = (((ch1 - 8) as u32 * 255) / 126) as u8;
                    (val, StrobeMode::Open)
                } else if ch1 <= 239 {
                    let rate = 1.0 + ((ch1 - 135) as f32 / 104.0) * 24.0;
                    (255u8, StrobeMode::Rate(rate))
                } else {
                    (255u8, StrobeMode::Open)
                };

                let r = get_ch(1);
                let g = get_ch(2);
                let b = get_ch(3);
                let w = get_ch(4);
                let pattern_id = get_ch(5);
                let audio_mode_id = get_ch(6);

                RenderCommand::Preset {
                    master_dimmer,
                    strobe,
                    color: ColorRgbw { r, g, b, w },
                    pattern_id,
                    audio_mode_id,
                    speed: 128,
                    size: 128,
                    white_mode: self.white_mode,
                }
            }
            Personality::Standard12ch => {
                let u = store.get(self.universe);
                let get_ch = |offset: u16| -> u8 {
                    if let Some(state) = u {
                        let ch = self.address.saturating_add(offset);
                        state.get_channel(ch)
                    } else {
                        0
                    }
                };

                let master_dimmer = get_ch(0);
                let ch2 = get_ch(1);
                let strobe = if ch2 >= 10 && ch2 <= 239 {
                    let rate = 0.5 + ((ch2 - 10) as f32 / 229.0) * 24.5;
                    StrobeMode::Rate(rate)
                } else {
                    StrobeMode::Open
                };

                let r = get_ch(2);
                let g = get_ch(3);
                let b = get_ch(4);
                let w = get_ch(5);
                let pattern_id = get_ch(6);
                let speed = get_ch(7);
                let size = get_ch(8);
                let audio_mode_id = get_ch(9);

                RenderCommand::Preset {
                    master_dimmer,
                    strobe,
                    color: ColorRgbw { r, g, b, w },
                    pattern_id,
                    audio_mode_id,
                    speed,
                    size,
                    white_mode: self.white_mode,
                }
            }
            Personality::PixelDirectRgb {
                start_universe,
                start_address,
                pixel_count: config_px_count,
                span_mode,
            } => {
                let effective_px = if config_px_count > 0 {
                    config_px_count.min(pixel_count)
                } else {
                    pixel_count
                };

                RenderCommand::DirectRgb {
                    pixel_count: effective_px,
                    master_dimmer: 255,
                    strobe: StrobeMode::Open,
                    start_universe,
                    start_address,
                    span_mode,
                    store,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_preset_7ch_interpretation() {
        let mut store = UniverseStore::new();
        store.subscribe(1);

        // Dimmer: 134 (Full master dimmer, no strobe)
        // Red: 255, Green: 128, Blue: 0, White: 50
        // Pattern: 5, Audio: 0
        let channels = [134, 255, 128, 0, 50, 5, 0];
        store.update_artnet(1, &channels, Instant::now());

        let fixture = FixtureConfig {
            personality: Personality::Preset7ch,
            universe: 1,
            address: 1,
            white_mode: WhiteMode::RgbBlend,
        };

        let cmd = fixture.interpret(&store, 100);
        match cmd {
            RenderCommand::Preset {
                master_dimmer,
                strobe,
                color,
                pattern_id,
                audio_mode_id,
                ..
            } => {
                assert_eq!(master_dimmer, 255);
                assert_eq!(strobe, StrobeMode::Open);
                assert_eq!(color.r, 255);
                assert_eq!(color.g, 128);
                assert_eq!(color.b, 0);
                assert_eq!(color.w, 50);
                assert_eq!(pattern_id, 5);
                assert_eq!(audio_mode_id, 0);
            }
            _ => panic!("Expected Preset command"),
        }
    }
}
