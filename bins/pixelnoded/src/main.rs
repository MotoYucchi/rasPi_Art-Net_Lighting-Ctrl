//! `pixelnoded`: High-reliability Art-Net / sACN to Pixel LED daemon.
//!
//! Hot path operates allocation-free and panic-free.

pub mod web;

use audio_proto::AudioFeaturePacket;
use dmx_universe::{FailsafeMode, UniverseStore};
use engine::{ColorOrder, RenderEngine, RgbColor, TestPatternMode};
use fixture::{FixtureConfig, Personality, UniverseSpanMode, WhiteMode};
use output::PixelSink;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::{Ipv4Addr, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use web::{AudioTelemetryInfo, SharedWebState, UniverseStatusInfo, WebCommand};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub node: NodeConfig,
    pub network: NetworkConfig,
    pub web: Option<WebConfig>,
    pub audio: Option<AudioConfig>,
    pub failsafe: FailsafeConfig,
    pub output: Vec<OutputDeviceConfig>,
    pub fixture: Vec<FixtureItemConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    #[serde(default)]
    pub show_lock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_true")]
    pub artnet: bool,
    #[serde(default = "default_true")]
    pub sacn: bool,
    #[serde(default = "default_bind_interface")]
    pub bind_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_web_port")]
    pub port: u16,
    #[serde(default = "default_bind_interface")]
    pub bind_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_audio_port")]
    pub listen_udp_port: u16,
    #[serde(default = "default_audio_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailsafeConfig {
    #[serde(default = "default_failsafe_policy")]
    pub on_data_loss: String, // hold | hold_then_fade | blackout
    #[serde(default = "default_hold_timeout")]
    pub hold_timeout_sec: u64,
    #[serde(default = "default_fade_duration")]
    pub fade_duration_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDeviceConfig {
    pub id: String,
    pub driver: String, // spi_ws281x | virtual
    #[serde(default = "default_spi_device")]
    pub device: String,
    #[serde(default = "default_chip")]
    pub chip: String,
    #[serde(default = "default_color_order")]
    pub color_order: String,
    pub pixels: usize,
    #[serde(default = "default_psu_max_ma")]
    pub psu_max_current_ma: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureItemConfig {
    pub output: String,
    pub pixel_range: [usize; 2],
    #[serde(default = "default_personality")]
    pub personality: String, // preset_7ch | standard | pixel_direct
    #[serde(default = "default_white_mode")]
    pub white_mode: String,  // rgb_blend | ignore
    #[serde(default = "default_universe_span")]
    pub universe_span: String, // per_170px | packed
    pub universe: u16,
    pub address: u16,
}

fn default_true() -> bool { true }
fn default_bind_interface() -> String { "0.0.0.0".to_string() }
fn default_web_port() -> u16 { 8080 }
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

fn build_fixture_config(cfg: &FixtureItemConfig) -> FixtureConfig {
    let fixture_px_count = (cfg.pixel_range[1] - cfg.pixel_range[0]) + 1;
    FixtureConfig {
        personality: parse_personality(cfg, fixture_px_count),
        universe: cfg.universe,
        address: cfg.address,
        white_mode: if cfg.white_mode == "ignore" {
            WhiteMode::Ignore
        } else {
            WhiteMode::RgbBlend
        },
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

[web]
enabled = true
port = 8080
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
    let mut render_engine = RenderEngine::new(color_order, first_out.psu_max_current_ma);

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

    let mut fixture = build_fixture_config(&config.fixture[0]);
    let mut fixture_px_count = (config.fixture[0].pixel_range[1] - config.fixture[0].pixel_range[0]) + 1;

    // 4. Setup Web Management Server & Command Channel
    let (cmd_tx, cmd_rx) = channel::<WebCommand>();
    let shared_state = Arc::new(SharedWebState {
        node_name: RwLock::new(config.node.name.clone()),
        show_lock: AtomicBool::new(config.node.show_lock),
        fps_x10: AtomicU32::new(500),
        frame_count: AtomicU64::new(0),
        artnet_rx: AtomicU64::new(0),
        sacn_rx: AtomicU64::new(0),
        current_ma: AtomicU32::new(0),
        max_psu_ma: AtomicU32::new(first_out.psu_max_current_ma),
        failsafe_active: AtomicBool::new(false),
        test_mode: AtomicU8::new(0),
        custom_rgb: [AtomicU8::new(255), AtomicU8::new(0), AtomicU8::new(0)],
        channels: RwLock::new(vec![0u8; 12]),
        universes: RwLock::new(vec![]),
        audio: RwLock::new(AudioTelemetryInfo::default()),
        config: RwLock::new(config.clone()),
        config_path: config_path.clone(),
        start_time: Instant::now(),
        cmd_tx,
    });

    let web_cfg = config.web.clone().unwrap_or(WebConfig {
        enabled: true,
        port: 8080,
        bind_ip: "0.0.0.0".to_string(),
    });

    let _web_handle = if web_cfg.enabled {
        match web::start_web_server(&web_cfg.bind_ip, web_cfg.port, shared_state.clone()) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!("Could not start Web server on {}:{}: {}", web_cfg.bind_ip, web_cfg.port, e);
                None
            }
        }
    } else {
        None
    };

    let mut pixel_buffer = vec![RgbColor::default(); total_pixels];
    let mut byte_buffer = vec![0u8; total_pixels * 3];
    let mut packet_rx_buf = [0u8; 1500];
    let mut poll_reply_buf = [0u8; 300];

    let mut last_audio_frame: Option<AudioFeaturePacket> = None;
    let mut last_audio_seen: Option<Instant> = None;

    let start_time = Instant::now();
    let frame_interval = Duration::from_millis(20); // 50 Hz

    let mut active_test_mode: u8 = 0; // 0 = off (DMX), 1 = RGBW, 2 = Walker, 3 = Soak, 4 = Strobe, 5 = Color
    let mut active_custom_color = RgbColor { r: 255, g: 0, b: 0 };

    let mut loop_count: u64 = 0;
    let mut last_fps_calc = Instant::now();
    let mut frames_in_second: u32 = 0;

    info!("Entering real-time data plane loop at 50 FPS...");

    loop {
        let loop_start = Instant::now();

        // Step 0: Check web commands (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WebCommand::SetTestMode(mode, rgb) => {
                    active_test_mode = mode;
                    active_custom_color = RgbColor { r: rgb[0], g: rgb[1], b: rgb[2] };
                    info!("Web command: set test mode {} (RGB: {:?})", mode, rgb);
                }
                WebCommand::SetLock(locked) => {
                    info!("Web command: set show lock = {}", locked);
                }
                WebCommand::ApplyConfig(new_cfg) => {
                    info!("Web command: applying updated configuration...");
                    if !new_cfg.fixture.is_empty() {
                        fixture = build_fixture_config(&new_cfg.fixture[0]);
                        fixture_px_count = (new_cfg.fixture[0].pixel_range[1] - new_cfg.fixture[0].pixel_range[0]) + 1;
                    }
                    if !new_cfg.output.is_empty() {
                        let out_cfg = &new_cfg.output[0];
                        let co = parse_color_order(&out_cfg.color_order);
                        render_engine = RenderEngine::new(co, out_cfg.psu_max_current_ma);
                        shared_state.max_psu_ma.store(out_cfg.psu_max_current_ma, Ordering::Relaxed);
                    }
                }
            }
        }

        // Step A: Poll network sockets
        if let Some(ref sock) = artnet_socket {
            while let Ok((amt, src)) = sock.recv_from(&mut packet_rx_buf) {
                match proto_artnet::parse_artnet(&packet_rx_buf[..amt]) {
                    Ok(proto_artnet::ArtNetPacket::Dmx(dmx)) => {
                        universe_store.update_artnet(dmx.port_address, dmx.data, Instant::now());
                        shared_state.artnet_rx.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(proto_artnet::ArtNetPacket::Poll(_poll)) => {
                        // Respond with ArtPollReply for network discovery
                        let node_name_guard = shared_state.node_name.read().ok();
                        let default_name = "PixelNode".to_string();
                        let node_name = node_name_guard.as_deref().unwrap_or(&default_name);
                        let poll_cfg = proto_artnet::ArtPollReplyConfig {
                            ip: [0, 0, 0, 0], // Auto-detected by receiver
                            port: proto_artnet::ARTNET_PORT,
                            vers_info: 0x0100,
                            net_switch: (fixture.universe >> 8) as u8,
                            sub_switch: ((fixture.universe >> 4) & 0x0F) as u8,
                            oem: 0x00FF,
                            status1: 0xD0,
                            esta_man: 0x7FF0,
                            short_name: node_name,
                            long_name: "PixelNode Art-Net WS2811 Controller",
                            node_report: "#0001 [0000] OK - 50 FPS",
                            num_ports: 1,
                            port_types: [0x80, 0, 0, 0],
                            good_input: [0; 4],
                            good_output: [0x80, 0, 0, 0],
                            sw_in: [0; 4],
                            sw_out: [(fixture.universe & 0x0F) as u8, 0, 0, 0],
                            mac: [0; 6],
                            bind_ip: [0, 0, 0, 0],
                        };
                        if let Ok(written) = proto_artnet::write_art_poll_reply(&mut poll_reply_buf, &poll_cfg) {
                            sock.send_to(&poll_reply_buf[..written], src).ok();
                        }
                    }
                    _ => {}
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
                    shared_state.sacn_rx.fetch_add(1, Ordering::Relaxed);
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

        let elapsed = start_time.elapsed();

        // Step C: Test Pattern Override or Normal DMX Render
        if active_test_mode > 0 {
            match active_test_mode {
                1 => render_engine.render_test_pattern(TestPatternMode::RgbwCycle, elapsed, &mut pixel_buffer),
                2 => render_engine.render_test_pattern(TestPatternMode::AddressWalker, elapsed, &mut pixel_buffer),
                3 => render_engine.render_test_pattern(TestPatternMode::PowerSoak100, elapsed, &mut pixel_buffer),
                4 => render_engine.render_test_pattern(TestPatternMode::HighRateStrobe40Hz, elapsed, &mut pixel_buffer),
                5 => pixel_buffer.fill(active_custom_color),
                _ => {}
            }
        } else {
            let cmd = fixture.interpret(&universe_store, fixture_px_count);
            render_engine.render(
                &cmd,
                last_audio_frame.as_ref(),
                elapsed,
                &mut pixel_buffer,
            );
        }

        // Step D: Pack color order and output to hardware
        render_engine.pack_bytes(&pixel_buffer, &mut byte_buffer);
        if let Err(e) = output_sink.write_pixels(&byte_buffer) {
            error!("PixelSink write error: {}", e);
        }

        // Calculate and update telemetry
        loop_count += 1;
        frames_in_second += 1;
        shared_state.frame_count.store(loop_count, Ordering::Relaxed);

        if last_fps_calc.elapsed() >= Duration::from_secs(1) {
            let fps = frames_in_second * 10;
            shared_state.fps_x10.store(fps, Ordering::Relaxed);
            frames_in_second = 0;
            last_fps_calc = Instant::now();
        }

        // Telemetry update every 10 frames (~200ms)
        if loop_count % 10 == 0 {
            // Estimate current draw (approx: sum of RGB channels * 20mA / 255)
            let mut sum_val: u32 = 0;
            for px in &pixel_buffer {
                sum_val += px.r as u32 + px.g as u32 + px.b as u32;
            }
            let estimated_ma = (sum_val * 20) / (255 * 3);
            shared_state.current_ma.store(estimated_ma, Ordering::Relaxed);

            // Channel preview snapshot (first 12 channels)
            if let Some(univ_state) = universe_store.get(fixture.universe) {
                let start_idx = fixture.address.saturating_sub(1) as usize;
                let end_idx = (start_idx + 12).min(univ_state.channels.len());
                if start_idx < univ_state.channels.len() {
                    let slice = &univ_state.channels[start_idx..end_idx];
                    if let Ok(mut ch_guard) = shared_state.channels.try_write() {
                        ch_guard.clear();
                        ch_guard.extend_from_slice(slice);
                    }
                }
            }

            // Universe status snapshot
            let mut univ_list = Vec::with_capacity(2);
            if let Some(univ_state) = universe_store.get(fixture.universe) {
                let age_ms = univ_state
                    .last_seen
                    .map(|t| loop_start.saturating_duration_since(t).as_millis() as u64)
                    .unwrap_or(9999);
                univ_list.push(UniverseStatusInfo {
                    universe: fixture.universe,
                    source: format!("{:?}", univ_state.source),
                    healthy: univ_state.active,
                    age_ms,
                });
            }
            if let Ok(mut u_guard) = shared_state.universes.try_write() {
                *u_guard = univ_list;
            }

            // Audio status snapshot
            if let Ok(mut aud_guard) = shared_state.audio.try_write() {
                if let Some(ref af) = last_audio_frame {
                    aud_guard.connected = true;
                    aud_guard.bpm = af.bpm as f32 / 100.0;
                    aud_guard.energy = af.rms_level as f32 / 255.0;
                    aud_guard.kick = af.is_kick();
                    aud_guard.snare = af.is_snare();
                    aud_guard.hihat = af.is_hihat();
                } else {
                    aud_guard.connected = false;
                    aud_guard.kick = false;
                    aud_guard.snare = false;
                    aud_guard.hihat = false;
                }
            }
        }

        // Sleep to maintain consistent 50 FPS
        let elapsed_loop = loop_start.elapsed();
        if elapsed_loop < frame_interval {
            sleep(frame_interval - elapsed_loop);
        }
    }
}
