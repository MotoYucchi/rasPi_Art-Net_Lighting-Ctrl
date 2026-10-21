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

/// Maximum payload supported for Arduino Uno (ATmega328P: unified single-buffer architecture -> max 300 pixels = 900 bytes)
pub const MAX_PAYLOAD_LEN: usize = 900;

/// Hardware-safe RGB byte sum budget (~260mA LED + ~40mA MCU = <300mA total) to prevent
/// Arduino Uno 500mA PTC polyfuse / linear regulator overheating and CH340 disconnects (os error 5)
pub const MAX_SAFE_MCU_RGB_SUM: u32 = 4_200;

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
    last_status: Option<McuStatus>,
    seq: u8,
    frame_buf: Vec<u8>,
    device_path: Option<String>,
    baud_rate: u32,
    last_frame_sent: Option<std::time::Instant>,
    last_dmx_payload: Option<[u8; 9]>,
    dropped_frames: u64,
    is_connected: bool,
    last_config: Option<(u16, u8, u16, u8)>,
    last_reconnect_attempt: Option<std::time::Instant>,
}

impl SerialMcuSink {
    /// Create sink from an arbitrary writer (used for unit tests and virtual serial mocks)
    pub fn from_writer(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer,
            reader: None,
            rx_buf: Vec::new(),
            last_status: None,
            seq: 0,
            frame_buf: Vec::with_capacity(HEADER_SIZE + MAX_PAYLOAD_LEN + FOOTER_SIZE),
            device_path: None,
            baud_rate: 115_200,
            last_frame_sent: None,
            last_dmx_payload: None,
            dropped_frames: 0,
            is_connected: true,
            last_config: None,
            last_reconnect_attempt: None,
        }
    }

    /// Get total count of dropped frames due to bandwidth limit or buffer backpressure
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Get the configured device path if opened from hardware serial port
    pub fn device_path(&self) -> Option<&str> {
        self.device_path.as_deref()
    }

    /// Get configured baud rate
    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    /// Non-blockingly drain all pending incoming bytes from kernel serial buffer and parse telemetry
    fn drain_rx(&mut self) {
        if let Some(ref mut reader) = self.reader {
            let mut temp_buf = [0u8; 128];
            while let Ok(n) = reader.read(&mut temp_buf) {
                if n == 0 {
                    break;
                }
                self.rx_buf.extend_from_slice(&temp_buf[..n]);
                if self.rx_buf.len() > 512 {
                    let excess = self.rx_buf.len() - 128;
                    self.rx_buf.drain(0..excess);
                }
            }
        }

        // Parse all complete status frames in rx_buf, keeping the freshest telemetry
        while self.rx_buf.len() >= HEADER_SIZE + FOOTER_SIZE {
            if self.rx_buf[0] != MAGIC_0 || self.rx_buf[1] != MAGIC_1 {
                self.rx_buf.remove(0);
                continue;
            }

            let len = u16::from_le_bytes([self.rx_buf[4], self.rx_buf[5]]) as usize;
            if len > 64 {
                // Invalid status packet length, discard false header
                self.rx_buf.remove(0);
                continue;
            }
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
                    self.last_status = Some(McuStatus {
                        packets_received: rx_cnt,
                        crc_errors: crc_err,
                        num_pixels: px_cnt,
                        active_pattern: patt,
                        in_failsafe: failsafe,
                    });
                }
            }
        }
    }

    /// Poll incoming telemetry status packet from Arduino Uno MCU (non-blocking)
    pub fn poll_status(&mut self) -> Option<McuStatus> {
        self.drain_rx();
        self.last_status.take()
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
            rx_buf: Vec::with_capacity(256),
            last_status: None,
            seq: 0,
            frame_buf: Vec::with_capacity(HEADER_SIZE + MAX_PAYLOAD_LEN + FOOTER_SIZE),
            device_path: Some(device_path.to_string()),
            baud_rate,
            last_frame_sent: None,
            last_dmx_payload: None,
            dropped_frames: 0,
            is_connected: true,
            last_config: None,
            last_reconnect_attempt: None,
        };

        sink.drain_rx();

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
        self.last_dmx_payload = Some(payload);
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
        let clamped_px = pixel_count.min((MAX_PAYLOAD_LEN / 3) as u16);
        self.last_config = Some((clamped_px, failsafe_mode, failsafe_timeout_ms, color_order));
        self.last_dmx_payload = None;
        let px_bytes = clamped_px.to_le_bytes();
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
        self.drain_rx();
        frame_packet(opcode, self.seq, payload, &mut self.frame_buf);
        self.seq = self.seq.wrapping_add(1);

        write_all_with_timeout(&mut *self.writer, &self.frame_buf, std::time::Duration::from_millis(250))?;
        self.last_frame_sent = Some(std::time::Instant::now());
        Ok(())
    }

    /// Immediately release dead serial file descriptors so Linux kernel can free `/dev/ttyUSB0`
    fn disconnect_and_release_fd(&mut self) {
        self.writer = Box::new(std::io::sink());
        self.reader = None;
        self.rx_buf.clear();
        self.last_dmx_payload = None;
        self.is_connected = false;
        self.last_reconnect_attempt = Some(std::time::Instant::now());
    }

    /// Attempt to re-open the serial port after a physical disconnect and re-sync MCU config
    #[cfg(target_os = "linux")]
    fn try_reconnect(&mut self) -> bool {
        let Some(configured_path) = self.device_path.clone() else {
            return false;
        };

        let candidates = [
            configured_path.as_str(),
            "/dev/ttyUSB0",
            "/dev/ttyUSB1",
            "/dev/ttyACM0",
        ];

        for candidate in candidates {
            if std::path::Path::new(candidate).exists() {
                if let Ok(new_sink) = Self::open(candidate, self.baud_rate) {
                    self.writer = new_sink.writer;
                    self.reader = new_sink.reader;
                    self.device_path = Some(candidate.to_string());
                    self.rx_buf.clear();
                    self.last_dmx_payload = None;
                    self.is_connected = true;
                    if let Some((px, fs, to, co)) = self.last_config {
                        let _ = self.send_config_with_color_order(px, fs, to, co);
                    }
                    return true;
                }
            }
        }
        false
    }
}

fn write_all_with_timeout(writer: &mut (dyn Write + Send), mut data: &[u8], timeout: std::time::Duration) -> Result<(), OutputError> {
    let deadline = std::time::Instant::now() + timeout;
    while !data.is_empty() {
        // Write full remaining slice in a single syscall so Linux usb-serial submits 1 URB per packet
        match writer.write(data) {
            Ok(0) => {
                return Err(OutputError::Io("Serial write returned zero bytes".into()));
            }
            Ok(n) => {
                data = &data[n..];
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                continue;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(OutputError::Io("Serial write timed out (buffer full)".into()));
                }
                std::thread::sleep(std::time::Duration::from_micros(400));
            }
            Err(e) => {
                return Err(OutputError::Io(format!("Serial write failed: {}", e)));
            }
        }
    }
    let _ = writer.flush();
    Ok(())
}

impl PixelSink for SerialMcuSink {
    fn write_dmx_command(
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
    ) -> Result<bool, OutputError> {
        self.drain_rx();

        if !self.is_connected {
            #[cfg(target_os = "linux")]
            {
                let should_retry = self
                    .last_reconnect_attempt
                    .map(|t| t.elapsed() >= std::time::Duration::from_millis(1500))
                    .unwrap_or(true);
                if should_retry {
                    self.last_reconnect_attempt = Some(std::time::Instant::now());
                    let _ = self.try_reconnect();
                }
            }
            if !self.is_connected {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                return Ok(true);
            }
        }

        let payload = [dimmer, r, g, b, strobe, pattern_id, audio_id, speed, size];
        let payload_changed = self.last_dmx_payload != Some(payload);

        // Event-driven transmission + 250ms (4 Hz) keepalive:
        // - When parameters change: send with a 35ms minimum interval (~28 Hz max)
        // - When parameters are unchanged: send a 16-byte keepalive every 250ms (4 Hz)
        let min_interval = if payload_changed {
            std::time::Duration::from_millis(35)
        } else {
            std::time::Duration::from_millis(250)
        };

        if let Some(last) = self.last_frame_sent {
            if last.elapsed() < min_interval {
                return Ok(true);
            }
        }

        frame_packet(OP_DMX_CHANNELS, self.seq, &payload, &mut self.frame_buf);
        self.seq = self.seq.wrapping_add(1);

        match self.writer.write(&self.frame_buf) {
            Ok(0) => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(true)
            }
            Ok(n) => {
                if n < self.frame_buf.len() {
                    if let Err(e) = write_all_with_timeout(
                        &mut *self.writer,
                        &self.frame_buf[n..],
                        std::time::Duration::from_millis(150),
                    ) {
                        self.disconnect_and_release_fd();
                        return Err(e);
                    }
                } else {
                    let _ = self.writer.flush();
                }
                self.last_dmx_payload = Some(payload);
                self.last_frame_sent = Some(std::time::Instant::now());
                Ok(true)
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(true)
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(true)
            }
            Err(e) => {
                self.disconnect_and_release_fd();
                Err(OutputError::Io(format!("Serial write failed: {}", e)))
            }
        }
    }

    fn write_pixels(&mut self, packed_rgb_bytes: &[u8]) -> Result<(), OutputError> {
        // 1. Always drain incoming serial RX bytes so kernel tty & CH340 IN buffers never overflow
        self.drain_rx();
        self.last_dmx_payload = None;

        // 2. If currently disconnected after an I/O error, periodically attempt self-healing reconnect
        if !self.is_connected {
            #[cfg(target_os = "linux")]
            {
                let should_retry = self
                    .last_reconnect_attempt
                    .map(|t| t.elapsed() >= std::time::Duration::from_millis(1500))
                    .unwrap_or(true);
                if should_retry {
                    self.last_reconnect_attempt = Some(std::time::Instant::now());
                    let _ = self.try_reconnect();
                }
            }
            if !self.is_connected {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                return Ok(());
            }
        }

        // 3. Gracefully clamp to hardware maximum capacity (300 pixels = 900 bytes)
        let payload = if packed_rgb_bytes.len() > MAX_PAYLOAD_LEN {
            &packed_rgb_bytes[..MAX_PAYLOAD_LEN]
        } else {
            packed_rgb_bytes
        };

        // 4. Accurate hardware frame pacing:
        //    Total frame time = UART transmission time + WS2812 bit-bang cli() time + USB/IP guard.
        //    Prevents host from transmitting the next frame while MCU interrupts are disabled in ws2812_show().
        let frame_bytes = HEADER_SIZE + payload.len() + FOOTER_SIZE;
        let num_pixels = payload.len() / 3;
        let uart_sec = (frame_bytes as f64 * 10.0) / (self.baud_rate as f64);
        let ws2812_sec = (num_pixels as f64 * 0.000_030) + 0.001;
        let usb_guard_sec = if self.device_path.is_some() { 0.012 } else { 0.0 };
        let min_frame_sec = (uart_sec + ws2812_sec + usb_guard_sec).max(0.015);
        let min_interval = std::time::Duration::from_secs_f64(min_frame_sec);

        if let Some(last) = self.last_frame_sent {
            if last.elapsed() < min_interval {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                return Ok(());
            }
        }

        // 5. Frame packet and apply hardware-safe ABL cap on physical MCU links to prevent 5V brownout (< 4.3V)
        frame_packet(OP_STREAM_PIXELS, self.seq, payload, &mut self.frame_buf);
        if self.device_path.is_some() && payload.len() > 0 {
            let payload_slice = &mut self.frame_buf[HEADER_SIZE..HEADER_SIZE + payload.len()];
            let mut total_sum: u32 = 0;
            for &b in payload_slice.iter() {
                total_sum += b as u32;
            }
            if total_sum > MAX_SAFE_MCU_RGB_SUM {
                let scale = (MAX_SAFE_MCU_RGB_SUM * 256) / total_sum;
                for b in payload_slice.iter_mut() {
                    *b = ((*b as u32 * scale) >> 8) as u8;
                }
                let new_crc = calculate_crc8(&self.frame_buf[2..HEADER_SIZE + payload.len()]);
                let last_idx = self.frame_buf.len() - 1;
                self.frame_buf[last_idx] = new_crc;
            }
        }
        self.seq = self.seq.wrapping_add(1);

        // 6. Write packet in a single write() syscall so Linux usb-serial submits 1 URB per frame
        match self.writer.write(&self.frame_buf) {
            Ok(0) => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(())
            }
            Ok(n) => {
                if n < self.frame_buf.len() {
                    if let Err(e) = write_all_with_timeout(
                        &mut *self.writer,
                        &self.frame_buf[n..],
                        std::time::Duration::from_millis(150),
                    ) {
                        self.disconnect_and_release_fd();
                        return Err(e);
                    }
                } else {
                    let _ = self.writer.flush();
                }
                self.last_frame_sent = Some(std::time::Instant::now());
                Ok(())
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(())
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {
                self.dropped_frames = self.dropped_frames.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                // Physical I/O error (e.g. os error 5 / EIO):
                // Immediately release dead file descriptor so kernel frees /dev/ttyUSB0!
                self.disconnect_and_release_fd();
                Err(OutputError::Io(format!("Serial write failed: {}", e)))
            }
        }
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
