//! Output abstraction layer: `PixelSink` trait, SPI WS2811/WS2812B driver, and Virtual Simulator.
//!
//! Hot path is bounded and allocation-free after initial setup.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::fmt;

#[derive(Debug)]
pub enum OutputError {
    Io(String),
    DeviceNotFound(String),
    BufferTooSmall,
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputError::Io(s) => write!(f, "Output I/O error: {}", s),
            OutputError::DeviceNotFound(s) => write!(f, "Device not found: {}", s),
            OutputError::BufferTooSmall => write!(f, "Output buffer too small"),
        }
    }
}

impl std::error::Error for OutputError {}

/// Abstract pixel sink for hardware pins, USB-serial, or virtual displays
pub trait PixelSink: Send {
    /// Write raw RGB byte stream (already packed according to color order) to the hardware
    fn write_pixels(&mut self, packed_rgb_bytes: &[u8]) -> Result<(), OutputError>;
}

/// Virtual Simulator Sink (in-memory, console or headless test)
pub struct VirtualSimSink {
    pub last_frame: Vec<u8>,
    pub frame_counter: u64,
}

impl VirtualSimSink {
    pub fn new() -> Self {
        Self {
            last_frame: Vec::new(),
            frame_counter: 0,
        }
    }
}

impl Default for VirtualSimSink {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelSink for VirtualSimSink {
    fn write_pixels(&mut self, packed_rgb_bytes: &[u8]) -> Result<(), OutputError> {
        self.last_frame.clear();
        self.last_frame.extend_from_slice(packed_rgb_bytes);
        self.frame_counter = self.frame_counter.wrapping_add(1);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub mod spi_ws281x {
    use super::{OutputError, PixelSink};
    use spidev::{Spidev, SpidevOptions, SpiModeFlags};
    use std::io::Write;

    /// SPI WS2811/WS2812B driver using Linux standard spidev
    pub struct SpiWs281xSink {
        spi: Spidev,
        spi_buffer: Vec<u8>,
        latch_bytes: usize,
    }

    impl SpiWs281xSink {
        /// Open SPI device (e.g. "/dev/spidev0.0") at 3.2MHz
        pub fn open(device_path: &str, max_pixels: usize) -> Result<Self, OutputError> {
            let mut spi = Spidev::open(device_path)
                .map_err(|e| OutputError::DeviceNotFound(format!("{}: {}", device_path, e)))?;

            let mut options = SpidevOptions::new();
            options
                .bits_per_word(8)
                .max_speed_hz(3_200_000) // 3.2 MHz for WS2811/WS2812B 4-bit encoding
                .mode(SpiModeFlags::SPI_MODE_0);

            spi.configure(&options)
                .map_err(|e| OutputError::Io(format!("SPI configure failed: {}", e)))?;

            // 1 pixel = 3 bytes RGB = 12 bytes SPI encoded data
            // 300us latch at 3.2MHz = 960 bits = 120 bytes
            let latch_bytes = 150;
            let total_capacity = (max_pixels * 12) + latch_bytes;
            let mut spi_buffer = Vec::with_capacity(total_capacity);
            spi_buffer.resize(total_capacity, 0);

            Ok(Self {
                spi,
                spi_buffer,
                latch_bytes,
            })
        }
    }

    impl PixelSink for SpiWs281xSink {
        fn write_pixels(&mut self, packed_rgb_bytes: &[u8]) -> Result<(), OutputError> {
            // Encode each 1-bit into a 4-bit SPI symbol:
            // Bit 0 => 1000b (0x8)
            // Bit 1 => 1100b (0xC)
            let needed_encoded_len = packed_rgb_bytes.len() * 4;
            let total_len = needed_encoded_len + self.latch_bytes;

            if self.spi_buffer.len() < total_len {
                self.spi_buffer.resize(total_len, 0);
            }

            // High-speed lookup table for nibbles (each 2-bit maps to 1 byte in SPI)
            // bit0=1000 (0x8), bit1=1100 (0xC)
            // 00 -> 0x88, 01 -> 0x8C, 10 -> 0xC8, 11 -> 0xCC
            const NIBBLE_LOOKUP: [u8; 4] = [0x88, 0x8C, 0xC8, 0xCC];

            let mut out_idx = 0;
            for &byte in packed_rgb_bytes.iter() {
                self.spi_buffer[out_idx] = NIBBLE_LOOKUP[((byte >> 6) & 0x03) as usize];
                self.spi_buffer[out_idx + 1] = NIBBLE_LOOKUP[((byte >> 4) & 0x03) as usize];
                self.spi_buffer[out_idx + 2] = NIBBLE_LOOKUP[((byte >> 2) & 0x03) as usize];
                self.spi_buffer[out_idx + 3] = NIBBLE_LOOKUP[(byte & 0x03) as usize];
                out_idx += 4;
            }

            // Latch reset period (zeros)
            self.spi_buffer[needed_encoded_len..total_len].fill(0);

            self.spi.write_all(&self.spi_buffer[..total_len])
                .map_err(|e| OutputError::Io(format!("SPI write failed: {}", e)))?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_sim_sink() {
        let mut sim = VirtualSimSink::new();
        let frame = [255, 0, 128];
        sim.write_pixels(&frame).unwrap();

        assert_eq!(sim.last_frame, frame);
        assert_eq!(sim.frame_counter, 1);
    }
}
