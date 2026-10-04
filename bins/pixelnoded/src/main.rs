//! `pixelnoded`: High-reliability Art-Net / sACN to Pixel LED daemon.
//!
//! Hot path operates allocation-free and panic-free.

use audio_proto::AudioFeaturePacket;
use dmx_universe::{FailsafeMode, UniverseStore};
use engine::{ColorOrder, RenderEngine, RgbColor};
use fixture::{FixtureConfig, Personality, UniverseSpanMode, WhiteMode};
use output::PixelSink;
use serde::Deserialize;
use std::fs;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Config {
    node: NodeConfig,
    network: NetworkConfig,
    audio: Option<AudioConfig>,
    failsafe: FailsafeConfig,
    output: Vec<OutputDeviceConfig>,
    fixture: Vec<FixtureItemConfig>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NodeConfig {
    name: String,
    #[serde(default)]
    show_lock: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NetworkConfig {
    #[serde(default = "default_true")]
    artnet: bool,
    #[serde(default = "default_true")]
    sacn: bool,
    #[serde(default = "default_bind_interface")]
    bind_ip: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AudioConfig {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_audio_port")]
    listen_udp_port: u16,
    #[serde(default = "default_audio_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FailsafeConfig {
    #[serde(default = "default_failsafe_policy")]
    on_data_loss: String, // hold | hold_then_fade | blackout
    #[serde(default = "default_hold_timeout")]
    hold_timeout_sec: u64,
    #[serde(default = "default_fade_duration")]
    fade_duration_sec: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OutputDeviceConfig {
    id: String,
    driver: String, // spi_ws281x | virtual
    #[serde(default = "default_spi_device")]
    device: String,
    #[serde(default = "default_chip")]
    chip: String,
    #[serde(default = "default_color_order")]
    color_order: String,
    pixels: usize,
    #[serde(default = "default_psu_max_ma")]
    psu_max_current_ma: u32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FixtureItemConfig {
    output: String,
    pixel_range: [usize; 2],
    #[serde(default = "default_personality")]
    personality: String, // preset_7ch | standard | pixel_direct
    #[serde(default = "default_white_mode")]
    white_mode: String,  // rgb_blend | ignore
    #[serde(default = "default_universe_span")]
    universe_span: String, // per_170px | packed
    universe: u16,
    address: u16,
}

fn default_true() -> bool { true }
fn default_bind_interface() -> String { "0.0.0.0".to_string() }
fn default_audio_port() -> u16 { 8765 }
fn default_audio_timeout_ms() -> u64 { 500 }
fn default_failsafe_policy() -> String { "hold".to_string() }
fn default_hold_timeout() -> u64 { 10 }
fn default_fade_duration() -> u64 { 3 }
fn default_spi_device() -> String { "/dev/spidev0.0".to_string() }
fn default_chip() -> String { "ws2811".to_string() }
fn default_color_order() -> String { "brg".to_string() }
fn default_psu_max_ma() -> u32 { 15000 }
fn default_personality() -> String { "preset_7ch".to_string() }
fn default_white_mode() -> String { "rgb_blend".to_string() }
fn default_universe_span() -> String { "per_170px".to_string() }

fn parse_color_order(s: &str) -> ColorOrder {
    match s.to_lowercase().as_str() {
        "rgb" => ColorOrder::Rgb,
        "rbg" => ColorOrder::Rbg,
        "grb" => ColorOrder::Grb,
        "gbr" => ColorOrder::Gbr,
        "brg" => ColorOrder::Brg,
        "bgr" => ColorOrder::Bgr,
        _ => ColorOrder::Brg,
    }
}

fn parse_personality(item: &FixtureItemConfig, pixel_count: usize) -> Personality {
    match item.personality.as_str() {
        "standard" => Personality::Standard12ch,
        "pixel_direct" => {
            let span_mode = if item.universe_span == "packed" {
                UniverseSpanMode::Packed
            } else {
                UniverseSpanMode::Per170Px
            };
            Personality::PixelDirectRgb {
                start_universe: item.universe,
                start_address: item.address,
                pixel_count,
                span_mode,
            }
        }
        _ => Personality::Preset7ch,
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting pixelnoded (Pixel LED Art-Net / sACN Node)...");

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/pixelnode/config.toml".to_string());

    let config: Config = if Path::new(&config_path).exists() {
        let content = fs::read_to_string(&config_path).expect("Failed to read config file");
        toml::from_str(&content).expect("Failed to parse config TOML")
    } else {
        warn!("Config file '{}' not found, using default configuration", config_path);
        let default_toml = r#"
[node]
name = "Stage-WS2811-Default"
show_lock = false

[network]
artnet = true
sacn = true
bind_ip = "0.0.0.0"

[failsafe]
on_data_loss = "hold"
hold_timeout_sec = 10
fade_duration_sec = 3

[[output]]
id = "out1"
driver = "virtual"
device = "/dev/spidev0.0"
chip = "ws2811"
color_order = "brg"
pixels = 300
psu_max_current_ma = 12000

[[fixture]]
output = "out1"
pixel_range = [0, 299]
personality = "preset_7ch"
white_mode = "rgb_blend"
universe = 1
address = 1
"#;
        toml::from_str(default_toml).expect("Default config parse error")
    };

    info!("Node initialized: '{}' (Show Lock: {})", config.node.name, config.node.show_lock);

    let failsafe_mode = match config.failsafe.on_data_loss.as_str() {
        "hold_then_fade" => FailsafeMode::HoldThenFade {
            hold_duration: Duration::from_secs(config.failsafe.hold_timeout_sec),
            fade_duration: Duration::from_secs(config.failsafe.fade_duration_sec),
        },
        "blackout" => FailsafeMode::Blackout,
        _ => FailsafeMode::Hold,
    };
    info!("Failsafe policy: {:?}", failsafe_mode);

    // 1. Setup DMX Universe Store and subscribe needed universes
    let mut universe_store = UniverseStore::new();
    for f in &config.fixture {
        universe_store.subscribe(f.universe);
        if f.personality == "pixel_direct" {
            let px_count = (f.pixel_range[1] - f.pixel_range[0]) + 1;
            let univ_count = (px_count + 169) / 170;
            for u in 0..univ_count {
                universe_store.subscribe(f.universe + u as u16);
            }
        }
    }

    // 2. Setup UDP sockets (Art-Net, sACN, Audio)
    let artnet_socket = if config.network.artnet {
        let addr = format!("{}:{}", config.network.bind_ip, proto_artnet::ARTNET_PORT);
        match UdpSocket::bind(&addr) {
            Ok(sock) => {
                sock.set_nonblocking(true).ok();
                info!("Art-Net listening on {}", addr);
                Some(sock)
            }
            Err(e) => {
                warn!("Could not bind Art-Net socket {}: {}", addr, e);
                None
            }
        }
    } else {
        None
    };

    let sacn_socket = if config.network.sacn {
        let addr = format!("{}:{}", config.network.bind_ip, proto_sacn::SACN_PORT);
        match UdpSocket::bind(&addr) {
            Ok(sock) => {
                sock.set_nonblocking(true).ok();
                // Join multicast for subscribed universes
                for f in &config.fixture {
                    let mcast_ip = proto_sacn::universe_to_multicast_ip(f.universe);
                    let mcast_addr = Ipv4Addr::new(mcast_ip[0], mcast_ip[1], mcast_ip[2], mcast_ip[3]);
                    sock.join_multicast_v4(&mcast_addr, &Ipv4Addr::UNSPECIFIED).ok();
                }
                info!("sACN listening on {}", addr);
                Some(sock)
            }
            Err(e) => {
                warn!("Could not bind sACN socket {}: {}", addr, e);
                None
            }
        }
    } else {
        None
    };

    let audio_socket = if let Some(ref aud) = config.audio {
        if aud.enabled {
            let addr = format!("{}:{}", config.network.bind_ip, aud.listen_udp_port);
            match UdpSocket::bind(&addr) {
                Ok(sock) => {
                    sock.set_nonblocking(true).ok();
                    info!("Audio sync feature listener on {}", addr);
                    Some(sock)
                }
                Err(e) => {
                    warn!("Could not bind Audio feature socket {}: {}", addr, e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // 3. Initialize outputs and render engines
    let first_out = &config.output[0];
    let total_pixels = first_out.pixels;
    let color_order = parse_color_order(&first_out.color_order);
    let render_engine = RenderEngine::new(color_order, first_out.psu_max_current_ma);

    let mut output_sink: Box<dyn PixelSink> = match first_out.driver.as_str() {
        #[cfg(target_os = "linux")]
        "spi_ws281x" => match output::spi_ws281x::SpiWs281xSink::open(&first_out.device, total_pixels) {
            Ok(sink) => {
                info!("Initialized SPI output on '{}'", first_out.device);
                Box::new(sink)
            }
            Err(e) => {
                warn!("Failed to open SPI device '{}': {}. Falling back to virtual sink.", first_out.device, e);
                Box::new(output::VirtualSimSink::new())
            }
        },
        _ => {
            info!("Initialized Virtual output sink (headless/sim)");
            Box::new(output::VirtualSimSink::new())
        }
    };

    let fixture_cfg = &config.fixture[0];
    let fixture_px_count = (fixture_cfg.pixel_range[1] - fixture_cfg.pixel_range[0]) + 1;
    let fixture = FixtureConfig {
        personality: parse_personality(fixture_cfg, fixture_px_count),
        universe: fixture_cfg.universe,
        address: fixture_cfg.address,
        white_mode: if fixture_cfg.white_mode == "ignore" {
            WhiteMode::Ignore
        } else {
            WhiteMode::RgbBlend
        },
    };

    let mut pixel_buffer = vec![RgbColor::default(); total_pixels];
    let mut byte_buffer = vec![0u8; total_pixels * 3];
    let mut packet_rx_buf = [0u8; 1500];

    let mut last_audio_frame: Option<AudioFeaturePacket> = None;
    let mut last_audio_seen: Option<Instant> = None;

    let start_time = Instant::now();
    let frame_interval = Duration::from_millis(20); // 50 Hz

    info!("Entering real-time data plane loop at 50 FPS...");

    loop {
        let loop_start = Instant::now();

        // Step A: Poll network sockets
        if let Some(ref sock) = artnet_socket {
            while let Ok((amt, _src)) = sock.recv_from(&mut packet_rx_buf) {
                if let Ok(proto_artnet::ArtNetPacket::Dmx(dmx)) =
                    proto_artnet::parse_artnet(&packet_rx_buf[..amt])
                {
                    universe_store.update_artnet(dmx.port_address, dmx.data, Instant::now());
                }
            }
        }

        if let Some(ref sock) = sacn_socket {
            while let Ok((amt, _src)) = sock.recv_from(&mut packet_rx_buf) {
                if let Ok(proto_sacn::SacnPacket::Data(data)) =
                    proto_sacn::parse_sacn(&packet_rx_buf[..amt])
                {
                    universe_store.update_sacn(
                        data.universe,
                        data.priority,
                        data.data,
                        data.is_stream_terminated(),
                        Instant::now(),
                    );
                }
            }
        }

        if let Some(ref sock) = audio_socket {
            while let Ok((amt, _src)) = sock.recv_from(&mut packet_rx_buf) {
                if let Ok(feat) = AudioFeaturePacket::parse(&packet_rx_buf[..amt]) {
                    last_audio_frame = Some(feat);
                    last_audio_seen = Some(Instant::now());
                }
            }
        }

        // Check audio timeout
        if let Some(seen) = last_audio_seen {
            if loop_start.saturating_duration_since(seen) > Duration::from_millis(500) {
                last_audio_frame = None;
            }
        }

        // Step B: Check DMX Universe timeouts (2.5s)
        universe_store.check_timeouts(loop_start, Duration::from_millis(2500));

        // Step C: Interpret fixture and Render
        let cmd = fixture.interpret(&universe_store, fixture_px_count);
        let elapsed = start_time.elapsed();

        render_engine.render(
            &cmd,
            last_audio_frame.as_ref(),
            elapsed,
            &mut pixel_buffer,
        );

        // Step D: Pack color order and output to hardware
        render_engine.pack_bytes(&pixel_buffer, &mut byte_buffer);
        if let Err(e) = output_sink.write_pixels(&byte_buffer) {
            error!("PixelSink write error: {}", e);
        }

        // Sleep to maintain consistent 50 FPS
        let elapsed_loop = loop_start.elapsed();
        if elapsed_loop < frame_interval {
            sleep(frame_interval - elapsed_loop);
        }
    }
}
