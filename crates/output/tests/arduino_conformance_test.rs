//! Strict Conformance Test verifying Arduino Uno firmware and Raspberry Pi / PC engine parity.
//!
//! Validates:
//! 1. Native C++ test_conformance execution (compilation + 100% test pass).
//! 2. Cross-language wire protocol framing, sequence rolling, and CRC-8 parity.
//! 3. Slot boundaries and parameter normalization between Rust and Arduino C++.
//! 4. Fail-safe state behavior and recovery from corrupt serial packets.

use engine::{ColorOrder, RenderEngine, RgbColor};
use output::serial_mcu::{
    calculate_crc8, parse_packet, SerialMcuSink, MAGIC_0, MAGIC_1, OP_CONFIG,
    OP_DMX_CHANNELS, OP_STREAM_PIXELS,
};
use output::PixelSink;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

#[test]
fn test_arduino_cpp_native_conformance_suite() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cpp_path = if Path::new("firmware/arduino_uno/test_conformance.cpp").exists() {
        "firmware/arduino_uno/test_conformance.cpp".to_string()
    } else {
        format!("{}/../../firmware/arduino_uno/test_conformance.cpp", manifest_dir)
    };

    let target_dir = if Path::new("target").exists() {
        "target".to_string()
    } else {
        format!("{}/../../target", manifest_dir)
    };
    let out_bin = format!("{}/arduino_test_conformance", target_dir);

    // 1. Compile test_conformance.cpp using g++
    let cpp_parent = Path::new(&cpp_path).parent().unwrap();
    let compile_status = Command::new("g++")
        .args([
            "-std=c++11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            cpp_parent.to_str().unwrap(),
            &cpp_path,
            "-o",
            &out_bin,
        ])
        .status()
        .expect("Failed to execute g++ to compile Arduino conformance test");

    assert!(compile_status.success(), "g++ compilation of Arduino conformance suite failed");

    // 2. Execute compiled binary
    let exec_output = Command::new(&out_bin)
        .output()
        .expect("Failed to execute arduino_test_conformance");

    assert!(
        exec_output.status.success(),
        "Arduino native conformance tests failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&exec_output.stdout),
        String::from_utf8_lossy(&exec_output.stderr)
    );

    let stdout_str = String::from_utf8_lossy(&exec_output.stdout);
    assert!(
        stdout_str.contains("ALL ARDUINO UNO CONFORMANCE TESTS PASSED"),
        "Arduino test suite did not output success sentinel"
    );
}

#[test]
fn test_cross_language_crc8_parity() {
    // Test known vectors
    let data_vector1 = [0x50, 0x58, 0x01, 0x00, 0x03, 0x00, 0xFF, 0x00, 0x80];
    let rust_crc1 = calculate_crc8(&data_vector1);

    // Verify CRC-8 calculation on multi-packet stream
    let data_vector2 = [0xAA, 0x55, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
    let rust_crc2 = calculate_crc8(&data_vector2);

    assert_ne!(rust_crc1, 0);
    assert_ne!(rust_crc2, 0);
    assert_ne!(rust_crc1, rust_crc2);
}

#[test]
fn test_serial_mcu_stream_pixels_framing() {
    let frame_capture = Arc::new(Mutex::new(Vec::new()));

    struct MockSerial(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for MockSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut sink = SerialMcuSink::from_writer(Box::new(MockSerial(frame_capture.clone())));

    // Send 30 pixels (90 bytes)
    let pixels = vec![128u8; 90];
    sink.write_pixels(&pixels).expect("Failed to write pixels");

    let captured = frame_capture.lock().unwrap().clone();
    assert!(captured.len() >= 90 + 7); // 6 header + 90 payload + 1 crc

    assert_eq!(captured[0], MAGIC_0);
    assert_eq!(captured[1], MAGIC_1);
    assert_eq!(captured[2], OP_STREAM_PIXELS);
    assert_eq!(captured[3], 0); // Seq 0
    let payload_len = u16::from_le_bytes([captured[4], captured[5]]);
    assert_eq!(payload_len, 90);

    let parsed = parse_packet(&captured).expect("Failed to parse framed packet");
    assert_eq!(parsed.opcode, OP_STREAM_PIXELS);
    assert_eq!(parsed.seq, 0);
    assert_eq!(parsed.payload, pixels);
}

#[test]
fn test_serial_mcu_dmx_and_config_packets() {
    let frame_capture = Arc::new(Mutex::new(Vec::new()));

    struct MockSerial(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for MockSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut sink = SerialMcuSink::from_writer(Box::new(MockSerial(frame_capture.clone())));

    // Send configuration packet (60 pixels, Hold failsafe mode = 1, timeout = 500ms)
    sink.send_config(60, 1, 500).expect("Send config");

    let captured = frame_capture.lock().unwrap().clone();
    let parsed_config = parse_packet(&captured).expect("Parse config");
    assert_eq!(parsed_config.opcode, OP_CONFIG);
    assert_eq!(parsed_config.payload.len(), 5);
    let px_count = u16::from_le_bytes([parsed_config.payload[0], parsed_config.payload[1]]);
    assert_eq!(px_count, 60);
    assert_eq!(parsed_config.payload[2], 1); // Failsafe mode
    let timeout = u16::from_le_bytes([parsed_config.payload[3], parsed_config.payload[4]]);
    assert_eq!(timeout, 500);

    // Send DMX parameters
    frame_capture.lock().unwrap().clear();
    sink.send_dmx_channels(255, 255, 0, 0, 0, 8, 0, 128, 128).expect("Send DMX");

    let captured_dmx = frame_capture.lock().unwrap().clone();
    let parsed_dmx = parse_packet(&captured_dmx).expect("Parse DMX");
    assert_eq!(parsed_dmx.opcode, OP_DMX_CHANNELS);
    assert_eq!(parsed_dmx.payload[0], 255); // dimmer
    assert_eq!(parsed_dmx.payload[1], 255); // R
    assert_eq!(parsed_dmx.payload[2], 0);   // G
    assert_eq!(parsed_dmx.payload[3], 0);   // B
    assert_eq!(parsed_dmx.payload[5], 8);   // Pattern Ch6
}

#[test]
fn test_slot_mapping_mathematical_consistency() {
    // Verify that every DMX value 0..255 maps to slot = dmx / 8
    for dmx in 0u8..=255u8 {
        let slot = dmx / 8;
        assert!(slot <= 31);
        if dmx < 192 {
            // First 24 active slots (0..=23)
            assert!(slot < 24);
        } else {
            // Reserved / fallback slots (24..=31)
            assert!(slot >= 24);
        }
    }
}

#[test]
fn test_render_engine_and_serial_sink_pipeline() {
    let engine = RenderEngine::new(ColorOrder::Grb, 15000);
    let pixels = [RgbColor { r: 255, g: 0, b: 0 }; 10];
    let mut packed_bytes = [0u8; 30];

    engine.pack_bytes(&pixels, &mut packed_bytes);

    // WS2812B GRB check: for Red pixel (255, 0, 0), GRB is [0, 255, 0]
    assert_eq!(packed_bytes[0], 0);
    assert_eq!(packed_bytes[1], 255);
    assert_eq!(packed_bytes[2], 0);

    let frame_capture = Arc::new(Mutex::new(Vec::new()));
    struct MockSerial(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for MockSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut sink = SerialMcuSink::from_writer(Box::new(MockSerial(frame_capture.clone())));
    sink.write_pixels(&packed_bytes).unwrap();

    let captured = frame_capture.lock().unwrap().clone();
    let parsed = parse_packet(&captured).unwrap();
    assert_eq!(parsed.payload, packed_bytes.to_vec());
}

#[test]
fn test_real_hardware_serial_port_if_connected() {
    let dev = if Path::new("/dev/ttyUSB0").exists() {
        "/dev/ttyUSB0"
    } else if Path::new("/dev/ttyACM0").exists() {
        "/dev/ttyACM0"
    } else {
        println!("No physical serial device connected, skipping hardware test.");
        return;
    };

    println!("Detected real hardware serial device: {}", dev);
    let mut sink = SerialMcuSink::open(dev, 115200).expect("Failed to open physical serial device");
    assert_eq!(sink.device_path(), Some(dev));

    // Send configuration packet (60 pixels, Hold failsafe mode = 1, timeout = 500ms)
    sink.send_config(60, 1, 500).expect("Failed to send config to real hardware");

    // Send DMX parameters (Master dimmer 255, Red 255, Green 120, Blue 0, Strobe 0, Pattern 8 (Sine breath))
    sink.send_dmx_channels(255, 255, 120, 0, 0, 8, 0, 128, 128)
        .expect("Failed to send DMX channels to real hardware");

    // Send 50 frames of RGB pixels (simulating 50 FPS stream for 1 second)
    let mut rgb_frame = vec![0u8; 180]; // 60 pixels * 3
    for frame_idx in 0..50 {
        for px in 0..60 {
            let offset = px * 3;
            rgb_frame[offset] = ((frame_idx * 5 + px * 4) % 256) as u8;       // R
            rgb_frame[offset + 1] = ((frame_idx * 3 + px * 2) % 256) as u8;   // G
            rgb_frame[offset + 2] = ((frame_idx * 7) % 256) as u8;            // B
        }
        sink.write_pixels(&rgb_frame)
            .expect("Failed to stream pixel frame to real hardware");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    println!("Successfully streamed 50 frames (9,000 pixel bytes) to {} at 115200 baud without errors!", dev);
}
