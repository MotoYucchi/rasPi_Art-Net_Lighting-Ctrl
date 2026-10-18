//! USB-Serial MCU Output Sink (`SerialMcuSink`) for Arduino Uno (ATmega328P) and compatible MCUs.
//!
//! Provides deterministic framing, sequence tracking, and CRC-8 integrity verification
//! over USB-UART links (`/dev/ttyACM*`, `/dev/ttyUSB*`).
//! Bounded memory, allocation-free in hot rendering loops.

// Allow unsafe code only for libc termios FFI when opening Linux serial port.

use crate::{OutputError, PixelSink};
use std::io::Write;

/// Magic header bytes: 'P', 'X' (ASCII PixelNode)
pub const MAGIC_0: u8 = 0x50; // 'P'
pub const MAGIC_1: u8 = 0x58; // 'X'

/// Protocol Operation Codes
pub const OP_STREAM_PIXELS: u8 = 0x01;
pub const OP_DMX_CHANNELS: u8 = 0x02;
pub const OP_HEARTBEAT: u8 = 0x03;
pub const OP_CONFIG: u8 = 0x04;
pub const OP_AUDIO_FEATURES: u8 = 0x05;
pub const OP_STATUS_REPLY: u8 = 0x06;

/// MCU Telemetry and Health Status reported from Arduino Uno
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct McuStatus {
    pub packets_received: u16,
    pub crc_errors: u8,
    pub num_pixels: u16,
    pub active_pattern: u8,
    pub in_failsafe: bool,
}

pub const HEADER_SIZE: usize = 6; // MAGIC(2) + OPCODE(1) + SEQ(1) + LEN(2)
pub const FOOTER_SIZE: usize = 1; // CRC-8(1)

/// Maximum payload supported for Arduino Uno (ATmega328P: 2KB SRAM -> max 170 pixels = 510 bytes)
pub const MAX_PAYLOAD_LEN: usize = 512;

/// Calculate CRC-8 (Dallas/Maxim polynomial: x^8 + x^5 + x^4 + 1 => 0x31)
#[inline]
pub fn calculate_crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x31;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Pack a command or pixel buffer into a framed packet
pub fn frame_packet(opcode: u8, seq: u8, payload: &[u8], out_buf: &mut Vec<u8>) {
    out_buf.clear();
    out_buf.push(MAGIC_0);
    out_buf.push(MAGIC_1);
    out_buf.push(opcode);
    out_buf.push(seq);
    let len_bytes = (payload.len() as u16).to_le_bytes();
    out_buf.push(len_bytes[0]);
    out_buf.push(len_bytes[1]);
    out_buf.extend_from_slice(payload);

    // CRC covers opcode, seq, length, and payload (everything after MAGIC bytes)
    let crc = calculate_crc8(&out_buf[2..]);
    out_buf.push(crc);
}

/// Parsed packet representation
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedPacket {
    pub opcode: u8,
    pub seq: u8,
    pub payload: Vec<u8>,
}

/// Parse a raw packet buffer and verify CRC-8
pub fn parse_packet(buf: &[u8]) -> Result<ParsedPacket, &'static str> {
    if buf.len() < HEADER_SIZE + FOOTER_SIZE {
        return Err("Buffer too short");
    }
    if buf[0] != MAGIC_0 || buf[1] != MAGIC_1 {
        return Err("Invalid magic bytes");
    }
    let opcode = buf[2];
    let seq = buf[3];
    let len = u16::from_le_bytes([buf[4], buf[5]]) as usize;

    if buf.len() != HEADER_SIZE + len + FOOTER_SIZE {
        return Err("Payload length mismatch");
    }

    let payload = &buf[HEADER_SIZE..HEADER_SIZE + len];
    let received_crc = buf[buf.len() - 1];

    let computed_crc = calculate_crc8(&buf[2..buf.len() - 1]);
    if received_crc != computed_crc {
        return Err("CRC-8 verification failed");
    }

    Ok(ParsedPacket {
        opcode,
        seq,
        payload: payload.to_vec(),
    })
}

/// USB-Serial output sink connecting to Arduino Uno or compatible MCU
pub struct SerialMcuSink {
    writer: Box<dyn Write + Send>,
    reader: Option<Box<dyn std::io::Read + Send>>,
    rx_buf: Vec<u8>,
    seq: u8,
    frame_buf: Vec<u8>,
    device_path: Option<String>,
    baud_rate: u32,
    last_frame_sent: Option<std::time::Instant>,
}

impl SerialMcuSink {
    /// Create sink from an arbitrary writer (used for unit tests and virtual serial mocks)
    pub fn from_writer(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer,
            reader: None,
            rx_buf: Vec::new(),
            seq: 0,
            frame_buf: Vec::with_capacity(HEADER_SIZE + MAX_PAYLOAD_LEN + FOOTER_SIZE),
            device_path: None,
            baud_rate: 115_200,
            last_frame_sent: None,
        }
    }

    /// Get the configured device path if opened from hardware serial port
    pub fn device_path(&self) -> Option<&str> {
        self.device_path.as_deref()
    }

    /// Get configured baud rate
    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    /// Poll incoming telemetry status packet from Arduino Uno MCU (non-blocking)
    pub fn poll_status(&mut self) -> Option<McuStatus> {
        let reader = self.reader.as_mut()?;
        let mut temp_buf = [0u8; 64];

        // Read available non-blocking bytes from kernel buffer
        if let Ok(n) = reader.read(&mut temp_buf) {
            if n > 0 {
                self.rx_buf.extend_from_slice(&temp_buf[..n]);
            }
        }

        // Search for framed packet
        while self.rx_buf.len() >= HEADER_SIZE + FOOTER_SIZE {
            // Find MAGIC_0 and MAGIC_1
            if self.rx_buf[0] != MAGIC_0 || self.rx_buf[1] != MAGIC_1 {
                self.rx_buf.remove(0);
                continue;
            }

            let len = u16::from_le_bytes([self.rx_buf[4], self.rx_buf[5]]) as usize;
            let full_len = HEADER_SIZE + len + FOOTER_SIZE;

            if self.rx_buf.len() < full_len {
                break; // Incomplete packet, wait for more data
            }

            let packet_bytes: Vec<u8> = self.rx_buf.drain(0..full_len).collect();
            if let Ok(parsed) = parse_packet(&packet_bytes) {
                if parsed.opcode == OP_STATUS_REPLY && parsed.payload.len() >= 7 {
                    let rx_cnt = u16::from_le_bytes([parsed.payload[0], parsed.payload[1]]);
                    let crc_err = parsed.payload[2];
                    let px_cnt = u16::from_le_bytes([parsed.payload[3], parsed.payload[4]]);
                    let patt = parsed.payload[5];
                    let failsafe = parsed.payload[6] != 0;
                    return Some(McuStatus {
                        packets_received: rx_cnt,
                        crc_errors: crc_err,
                        num_pixels: px_cnt,
                        active_pattern: patt,
                        in_failsafe: failsafe,
                    });
                }
            }
        }

        None
    }

    /// Open serial device (e.g. `/dev/ttyACM0` or `/dev/ttyUSB0`)
    #[cfg(target_os = "linux")]
    pub fn open(device_path: &str, baud_rate: u32) -> Result<Self, OutputError> {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(device_path)
            .map_err(|e| OutputError::DeviceNotFound(format!("{}: {}", device_path, e)))?;

        let fd = file.as_raw_fd();
        let mut tio: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut tio) } != 0 {
            return Err(OutputError::Io("tcgetattr failed on serial port".into()));
        }

        unsafe {
            libc::cfmakeraw(&mut tio);
        }

        tio.c_cflag |= libc::CREAD | libc::CLOCAL;
        tio.c_cflag &= !(libc::CRTSCTS);

        let speed = match baud_rate {
            115_200 => libc::B115200,
            230_400 => libc::B230400,
            500_000 => libc::B500000,
            576_000 => libc::B576000,
            _ => libc::B115200,
        };

        unsafe {
            libc::cfsetispeed(&mut tio, speed);
            libc::cfsetospeed(&mut tio, speed);
            if libc::tcsetattr(fd, libc::TCSANOW, &tio) != 0 {
                return Err(OutputError::Io("tcsetattr failed on serial port".into()));
            }
        }

        // Wait for Arduino Optiboot bootloader auto-reset delay (2.2s) so MCU fully completes setup()
        std::thread::sleep(std::time::Duration::from_millis(2200));

        // Flush any reset garbage or bootloader echoes from kernel buffers
        unsafe {
            libc::tcflush(fd, libc::TCIOFLUSH);
        }

        let reader_file = file.try_clone().ok().map(|f| Box::new(f) as Box<dyn std::io::Read + Send>);

        let mut sink = Self {
            writer: Box::new(file),
            reader: reader_file,
            rx_buf: Vec::with_capacity(128),
            seq: 0,
            frame_buf: Vec::with_capacity(HEADER_SIZE + MAX_PAYLOAD_LEN + FOOTER_SIZE),
            device_path: Some(device_path.to_string()),
            baud_rate,
            last_frame_sent: None,
        };

        // Send a heartbeat ping to initialize link
        let _ = sink.send_heartbeat();

        Ok(sink)
    }

    /// Send DMX channel parameters directly to Arduino Uno
    pub fn send_dmx_channels(
        &mut self,
        dimmer: u8,
        r: u8,
        g: u8,
        b: u8,
        strobe: u8,
        pattern_id: u8,
        audio_id: u8,
        speed: u8,
        size: u8,
    ) -> Result<(), OutputError> {
        let payload = [dimmer, r, g, b, strobe, pattern_id, audio_id, speed, size];
        self.send_packet(OP_DMX_CHANNELS, &payload)
    }

    /// Send hardware configuration parameters to MCU
    pub fn send_config(
        &mut self,
        pixel_count: u16,
        failsafe_mode: u8,
        failsafe_timeout_ms: u16,
    ) -> Result<(), OutputError> {
        self.send_config_with_color_order(pixel_count, failsafe_mode, failsafe_timeout_ms, 0)
    }

    /// Send hardware configuration including strip color order to MCU
    pub fn send_config_with_color_order(
        &mut self,
        pixel_count: u16,
        failsafe_mode: u8,
        failsafe_timeout_ms: u16,
        color_order: u8,
    ) -> Result<(), OutputError> {
        let px_bytes = pixel_count.to_le_bytes();
        let timeout_bytes = failsafe_timeout_ms.to_le_bytes();
        let payload = [
            px_bytes[0],
            px_bytes[1],
            failsafe_mode,
            timeout_bytes[0],
            timeout_bytes[1],
            color_order,
        ];
        self.send_packet(OP_CONFIG, &payload)
    }

    /// Send heartbeat ping to keep MCU watchdog alive
    pub fn send_heartbeat(&mut self) -> Result<(), OutputError> {
        self.send_packet(OP_HEARTBEAT, &[])
    }

    fn send_packet(&mut self, opcode: u8, payload: &[u8]) -> Result<(), OutputError> {
        frame_packet(opcode, self.seq, payload, &mut self.frame_buf);
        self.seq = self.seq.wrapping_add(1);

        self.writer
            .write_all(&self.frame_buf)
            .map_err(|e| OutputError::Io(format!("Serial write failed: {}", e)))?;
        self.writer
            .flush()
            .map_err(|e| OutputError::Io(format!("Serial flush failed: {}", e)))?;

        Ok(())
    }
}

impl PixelSink for SerialMcuSink {
    fn write_pixels(&mut self, packed_rgb_bytes: &[u8]) -> Result<(), OutputError> {
        if packed_rgb_bytes.len() > MAX_PAYLOAD_LEN {
            return Err(OutputError::BufferTooSmall);
        }

        // Throttle frame rate so baud rate bandwidth is never exceeded:
        // Frame bytes = 6 (header) + len + 1 (footer).
        // Target <= 85% of theoretical bandwidth to eliminate UART RX FIFO overruns on MCU.
        let frame_bytes = HEADER_SIZE + packed_rgb_bytes.len() + FOOTER_SIZE;
        let safe_bytes_per_sec = (self.baud_rate as f64 / 10.0) * 0.85;
        let min_frame_sec = (frame_bytes as f64 / safe_bytes_per_sec).max(0.010);
        let min_interval = std::time::Duration::from_secs_f64(min_frame_sec);

        if let Some(last) = self.last_frame_sent {
            let elapsed = last.elapsed();
            if elapsed < min_interval {
                std::thread::sleep(min_interval - elapsed);
            }
        }

        self.send_packet(OP_STREAM_PIXELS, packed_rgb_bytes)?;
        self.last_frame_sent = Some(std::time::Instant::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc8_calculation() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let crc = calculate_crc8(&data);
        assert_ne!(crc, 0);

        // Verification with same data produces identical CRC
        let crc2 = calculate_crc8(&data);
        assert_eq!(crc, crc2);
    }

    #[test]
    fn test_packet_framing_and_parsing_roundtrip() {
        let payload = [255, 128, 64, 32, 16, 8];
        let mut buf = Vec::new();
        frame_packet(OP_STREAM_PIXELS, 42, &payload, &mut buf);

        assert_eq!(buf[0], MAGIC_0);
        assert_eq!(buf[1], MAGIC_1);
        assert_eq!(buf[2], OP_STREAM_PIXELS);
        assert_eq!(buf[3], 42);

        let parsed = parse_packet(&buf).unwrap();
        assert_eq!(parsed.opcode, OP_STREAM_PIXELS);
        assert_eq!(parsed.seq, 42);
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn test_corrupt_packet_rejection() {
        let payload = [1, 2, 3, 4];
        let mut buf = Vec::new();
        frame_packet(OP_STREAM_PIXELS, 1, &payload, &mut buf);

        // Corrupt one payload byte
        buf[HEADER_SIZE + 1] ^= 0xFF;
        assert!(parse_packet(&buf).is_err());
    }

    #[test]
    fn test_serial_mcu_sink_write() {
        let shared_output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct MockWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for MockWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer = Box::new(MockWriter(shared_output.clone()));
        let mut sink = SerialMcuSink::from_writer(writer);

        let rgb = [255, 0, 128];
        sink.write_pixels(&rgb).unwrap();

        let recorded = shared_output.lock().unwrap().clone();
        let parsed = parse_packet(&recorded).unwrap();
        assert_eq!(parsed.opcode, OP_STREAM_PIXELS);
        assert_eq!(parsed.seq, 0);
        assert_eq!(parsed.payload, rgb);
    }
}
