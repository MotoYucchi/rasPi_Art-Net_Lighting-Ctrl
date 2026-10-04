//! `pxtool`: Field diagnosis, standalone rig-check, remote configuration, and DMX transmission CLI.

use clap::{Args, Parser, Subcommand};
use engine::{ColorOrder, RenderEngine, RgbColor, TestPatternMode};
use output::PixelSink;
use serde::Deserialize;
use std::fs;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "pxtool",
    author = "motoyucchi",
    version,
    about = "PixelNode Field Diagnostic, Configuration & Rig Check CLI Utility",
    after_help = "EXAMPLES:
  # Check live node telemetry (FPS, DMX channels, power, audio sync)
  pxtool status

  # Monitor live Art-Net and sACN network traffic on the local interface
  pxtool monitor --protocol all --channels 8

  # Run standalone hardware rig-check directly on SPI
  pxtool test --mode rgbw --pixels 300 --spi /dev/spidev0.0 --duration 10

  # Trigger remote rig-check pattern via running daemon's REST API
  pxtool test --mode walker --remote

  # Query running node configuration
  pxtool config show

  # Validate a configuration TOML file before deployment
  pxtool config validate deploy/config.example.toml

  # Modify running node's start universe and personality dynamically
  pxtool config set --universe 2 --address 1 --personality preset_7ch

  # Lock node for live show (prevents accidental changes)
  pxtool lock --enable

  # Send test Art-Net DMX frame
  pxtool send-artnet --universe 1 --channels 255,255,0,0,0,0,0
"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Query status, FPS, live channel values, and health from running pixelnoded
    Status(StatusArgs),

    /// Real-time lighting network traffic monitor (packet sniffer for Art-Net / sACN)
    Monitor(MonitorArgs),

    /// Run standalone rig-check self-test patterns (local SPI or via remote daemon)
    Test(TestArgs),

    /// View, validate, or modify node configuration
    Config(ConfigCommand),

    /// Toggle live show lock (protect node against accidental edits during show)
    Lock(LockArgs),

    /// Transmit test Art-Net DMX frame
    SendArtnet(SendArtnetArgs),

    /// Transmit test sACN DMX frame
    SendSacn(SendSacnArgs),
}

#[derive(Args)]
struct StatusArgs {
    /// PixelNode REST API endpoint URL
    #[arg(short, long, default_value = "http://127.0.0.1:8080")]
    node: String,

    /// Output raw JSON instead of human-readable view
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct MonitorArgs {
    /// Local interface IP to bind
    #[arg(short, long, default_value = "0.0.0.0")]
    interface: String,

    /// Protocol to monitor: artnet | sacn | all
    #[arg(short, long, default_value = "all")]
    protocol: String,

    /// Filter by universe ID (omit to show all universes)
    #[arg(short, long)]
    universe: Option<u16>,

    /// Number of DMX channel values to display per packet
    #[arg(short, long, default_value_t = 8)]
    channels: usize,

    /// Maximum duration in seconds (0 = run forever until Ctrl+C)
    #[arg(short, long, default_value_t = 0)]
    duration: u64,
}

#[derive(Args)]
struct TestArgs {
    /// Test mode: rgbw | walker | soak | strobe | color | off
    #[arg(short, long, default_value = "rgbw")]
    mode: String,

    /// Number of LED pixels (for local test)
    #[arg(short, long, default_value_t = 300)]
    pixels: usize,

    /// Color order for local test: brg | rgb | grb
    #[arg(long, default_value = "brg")]
    color_order: String,

    /// Custom RGB color for 'color' mode (e.g. '255,0,128')
    #[arg(long)]
    color: Option<String>,

    /// SPI device path for local test (omit for virtual console preview)
    #[arg(long)]
    spi: Option<String>,

    /// Trigger test pattern on running daemon via REST API instead of local SPI
    #[arg(short, long)]
    remote: bool,

    /// Remote node URL when --remote is specified
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    node: String,

    /// Maximum duration in seconds (0 = run forever)
    #[arg(short, long, default_value_t = 0)]
    duration: u64,
}

#[derive(Args)]
struct ConfigCommand {
    #[command(subcommand)]
    action: ConfigSubcommands,
}

#[derive(Subcommand)]
enum ConfigSubcommands {
    /// Display active configuration from running node or a local file
    Show {
        /// Remote node URL to query
        #[arg(short, long, default_value = "http://127.0.0.1:8080")]
        node: String,

        /// Path to local TOML file (overrides remote query)
        #[arg(short, long)]
        path: Option<String>,
    },

    /// Validate syntax, channel ranges, and PSU limits in a TOML config file
    Validate {
        /// Path to config TOML file
        path: String,
    },

    /// Modify running node settings via REST API
    Set {
        /// Remote node URL
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        node: String,

        /// Node display name
        #[arg(long)]
        name: Option<String>,

        /// Personality: preset_7ch | standard | pixel_direct
        #[arg(long)]
        personality: Option<String>,

        /// Start Universe ID (1..63999)
        #[arg(long)]
        universe: Option<u16>,

        /// Start Address (1..512)
        #[arg(long)]
        address: Option<u16>,

        /// Total pixels
        #[arg(long)]
        pixels: Option<usize>,

        /// Color order: brg | rgb | grb
        #[arg(long)]
        color_order: Option<String>,

        /// Max PSU current in mA
        #[arg(long)]
        max_current_ma: Option<u32>,

        /// Failsafe policy: hold | hold_then_fade | blackout
        #[arg(long)]
        failsafe: Option<String>,
    },
}

#[derive(Args)]
struct LockArgs {
    /// Remote node URL
    #[arg(short, long, default_value = "http://127.0.0.1:8080")]
    node: String,

    /// Enable show lock
    #[arg(long)]
    enable: bool,

    /// Disable show lock
    #[arg(long)]
    disable: bool,
}

#[derive(Args)]
struct SendArtnetArgs {
    /// Target IP address (e.g. 127.0.0.1 or broadcast 2.255.255.255)
    #[arg(long, default_value = "127.0.0.1")]
    ip: String,

    /// Universe (15-bit port address)
    #[arg(short, long, default_value_t = 1)]
    universe: u16,

    /// Raw DMX channel bytes (comma separated, e.g. "255,0,128")
    #[arg(long)]
    channels: String,
}

#[derive(Args)]
struct SendSacnArgs {
    /// Target IP address (e.g. 127.0.0.1 or multicast)
    #[arg(long, default_value = "127.0.0.1")]
    ip: String,

    /// Universe ID (1..63999)
    #[arg(short, long, default_value_t = 1)]
    universe: u16,

    /// Priority (1..200)
    #[arg(short, long, default_value_t = 100)]
    priority: u8,

    /// Raw DMX channel bytes (comma separated)
    #[arg(long)]
    channels: String,
}

// Telemetry deserialization structs
#[derive(Debug, Deserialize)]
struct StatusJson {
    node_name: String,
    version: String,
    uptime_sec: u64,
    show_lock: bool,
    fps_x10: u32,
    frame_count: u64,
    artnet_rx: u64,
    sacn_rx: u64,
    current_ma: u32,
    max_psu_ma: u32,
    failsafe_active: bool,
    test_mode: u8,
    #[serde(default)]
    personality: Option<String>,
    channels: Vec<u8>,
    universes: Vec<UniverseJson>,
    audio: AudioJson,
}

#[derive(Debug, Deserialize)]
struct UniverseJson {
    universe: u16,
    source: String,
    healthy: bool,
    age_ms: u64,
}

#[derive(Debug, Deserialize)]
struct AudioJson {
    connected: bool,
    bpm: f32,
    energy: f32,
    kick: bool,
    snare: bool,
    hihat: bool,
}

fn parse_color_order(s: &str) -> ColorOrder {
    match s.to_lowercase().as_str() {
        "rgb" => ColorOrder::Rgb,
        "grb" => ColorOrder::Grb,
        "brg" => ColorOrder::Brg,
        _ => ColorOrder::Brg,
    }
}

fn parse_csv_channels(s: &str) -> Vec<u8> {
    s.split(',')
        .filter_map(|part| part.trim().parse::<u8>().ok())
        .collect()
}

fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let parts: Vec<u8> = s.split(',')
        .filter_map(|part| part.trim().parse::<u8>().ok())
        .collect();
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

fn format_bar(val: u8, width: usize) -> String {
    let fill = (val as usize * width) / 255;
    let empty = width.saturating_sub(fill);
    format!("[{}{}]", "█".repeat(fill), "░".repeat(empty))
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status(args) => {
            let url = format!("{}/api/status", args.node.trim_end_matches('/'));
            match ureq::get(&url).timeout(Duration::from_secs(3)).call() {
                Ok(resp) => {
                    let body_str = resp.into_string().unwrap_or_default();
                    if args.json {
                        println!("{}", body_str);
                        return;
                    }
                    match serde_json::from_str::<StatusJson>(&body_str) {
                        Ok(st) => {
                            let fps = st.fps_x10 as f32 / 10.0;
                            let cur_a = st.current_ma as f32 / 1000.0;
                            let max_a = st.max_psu_ma as f32 / 1000.0;
                            let psu_pct = if st.max_psu_ma > 0 {
                                (st.current_ma as f32 / st.max_psu_ma as f32) * 100.0
                            } else {
                                0.0
                            };
                            let hours = st.uptime_sec / 3600;
                            let mins = (st.uptime_sec % 3600) / 60;
                            let secs = st.uptime_sec % 60;

                            println!("========================================================================");
                            println!(" PIXELNODE STATUS: {} (v{})", st.node_name, st.version);
                            println!("========================================================================");
                            println!(
                                " Uptime: {:02}h {:02}m {:02}s | Show Lock: {} | Failsafe: {}",
                                hours,
                                mins,
                                secs,
                                if st.show_lock { "LOCKED 🔒" } else { "UNLOCKED 🔓" },
                                if st.failsafe_active { "ACTIVE (HOLD) ⚠️" } else { "INACTIVE (OK)" }
                            );
                            println!("------------------------------------------------------------------------");
                            println!("[RENDERING]");
                            println!(" Target: 50.0 FPS | Actual: {:.1} FPS | Total Frames: {}", fps, st.frame_count);
                            println!(" Power Draw (ABL): {:.2} A / {:.1} A ({:.0}%) | Status: OK", cur_a, max_a, psu_pct);
                            let test_str = match st.test_mode {
                                1 => "RGBW Cycle",
                                2 => "Address Walker",
                                3 => "Power Soak 100%",
                                4 => "High-rate Strobe",
                                5 => "Solid Custom Color",
                                _ => "None (DMX Console Controlled)",
                            };
                            println!(" Active Test Override: {}", test_str);

                            println!("\n[NETWORK & PROTOCOL]");
                            println!(" Art-Net Packets Rx: {} | sACN Packets Rx: {}", st.artnet_rx, st.sacn_rx);
                            if st.universes.is_empty() {
                                println!(" Subscribed Universes: None active yet");
                            } else {
                                for u in &st.universes {
                                    println!(
                                        "   Universe {:>3} | Source: {:<8} | State: {} (Age: {}ms)",
                                        u.universe,
                                        u.source,
                                        if u.healthy { "ACTIVE" } else { "TIMEOUT" },
                                        u.age_ms
                                    );
                                }
                            }

                            println!("\n[LIVE CHANNEL VALUES]");
                            let personality = st.personality.as_deref().unwrap_or("preset_7ch");
                            let (channel_names, display_len) = if personality == "standard_12ch" || st.channels.len() > 7 {
                                (
                                    &[
                                        "Ch 1 [Dimmer    ]",
                                        "Ch 2 [Strobe    ]",
                                        "Ch 3 [Red       ]",
                                        "Ch 4 [Green     ]",
                                        "Ch 5 [Blue      ]",
                                        "Ch 6 [White     ]",
                                        "Ch 7 [Pattern   ]",
                                        "Ch 8 [Speed     ]",
                                        "Ch 9 [Size      ]",
                                        "Ch10 [Audio Mode]",
                                        "Ch11 [Audio Gain]",
                                        "Ch12 [Control   ]",
                                    ][..],
                                    12,
                                )
                            } else {
                                (
                                    &[
                                        "Ch 1 [Dim/Strobe]",
                                        "Ch 2 [Red       ]",
                                        "Ch 3 [Green     ]",
                                        "Ch 4 [Blue      ]",
                                        "Ch 5 [White     ]",
                                        "Ch 6 [Pattern   ]",
                                        "Ch 7 [Audio Sync]",
                                    ][..],
                                    7,
                                )
                            };
                            for (i, val) in st.channels.iter().take(display_len).enumerate() {
                                let name = channel_names.get(i).copied().unwrap_or("Ch --");
                                let pct = (*val as f32 / 255.0) * 100.0;
                                println!("   {}: {:3} {} {:3.0}%", name, val, format_bar(*val, 16), pct);
                            }

                            println!("\n[AUDIO REACTIVE SYNC]");
                            if st.audio.connected {
                                println!(
                                    " Status: CONNECTED | BPM: {:.1} | RMS Energy: {:.0}%",
                                    st.audio.bpm,
                                    st.audio.energy * 100.0
                                );
                                println!(
                                    " Beat Triggers: [KICK: {}] [SNARE: {}] [HIHAT: {}]",
                                    if st.audio.kick { "ON " } else { "OFF" },
                                    if st.audio.snare { "ON " } else { "OFF" },
                                    if st.audio.hihat { "ON " } else { "OFF" }
                                );
                            } else {
                                println!(" Status: DISCONNECTED (Audio sync offline)");
                            }
                            println!("========================================================================");
                        }
                        Err(e) => eprintln!("Error parsing node status JSON: {}", e),
                    }
                }
                Err(e) => eprintln!("Could not connect to PixelNode at '{}': {}", url, e),
            }
        }

        Commands::Monitor(args) => {
            println!("========================================================================");
            println!(" Lighting Network Monitor on interface '{}' (Filter: {})", args.interface, args.protocol);
            if let Some(u) = args.universe {
                println!(" Filtering Universe: {}", u);
            }
            println!(" Press Ctrl+C to stop");
            println!("========================================================================");

            let artnet_port = proto_artnet::ARTNET_PORT;
            let sacn_port = proto_sacn::SACN_PORT;

            let artnet_sock = if args.protocol == "artnet" || args.protocol == "all" {
                let addr = format!("{}:{}", args.interface, artnet_port);
                match UdpSocket::bind(&addr) {
                    Ok(s) => {
                        s.set_nonblocking(true).ok();
                        Some(s)
                    }
                    Err(e) => {
                        eprintln!("Warning: could not bind Art-Net socket {}: {}", addr, e);
                        None
                    }
                }
            } else {
                None
            };

            let sacn_sock = if args.protocol == "sacn" || args.protocol == "all" {
                let addr = format!("{}:{}", args.interface, sacn_port);
                match UdpSocket::bind(&addr) {
                    Ok(s) => {
                        s.set_nonblocking(true).ok();
                        Some(s)
                    }
                    Err(e) => {
                        eprintln!("Warning: could not bind sACN socket {}: {}", addr, e);
                        None
                    }
                }
            } else {
                None
            };

            let mut rx_buf = [0u8; 1500];
            let start = Instant::now();
            let mut packet_count: u64 = 0;

            loop {
                if args.duration > 0 && start.elapsed().as_secs() >= args.duration {
                    println!("\nMonitoring finished after {}s. Total packets: {}", args.duration, packet_count);
                    break;
                }

                let mut had_packet = false;

                if let Some(ref sock) = artnet_sock {
                    while let Ok((amt, src)) = sock.recv_from(&mut rx_buf) {
                        had_packet = true;
                        if let Ok(proto_artnet::ArtNetPacket::Dmx(dmx)) = proto_artnet::parse_artnet(&rx_buf[..amt]) {
                            let univ = dmx.port_address;
                            if args.universe.map_or(true, |u| u == univ) {
                                packet_count += 1;
                                let preview: Vec<String> = dmx.data.iter().take(args.channels).map(|c| format!("{:3}", c)).collect();
                                println!(
                                    "[{:>6.2}s] ART-NET | Src: {:<15} | Univ: {:<3} | Seq: {:<3} | Chs({}): [{}]",
                                    start.elapsed().as_secs_f32(),
                                    src.ip().to_string(),
                                    univ,
                                    dmx.sequence,
                                    dmx.data.len(),
                                    preview.join(",")
                                );
                            }
                        }
                    }
                }

                if let Some(ref sock) = sacn_sock {
                    while let Ok((amt, src)) = sock.recv_from(&mut rx_buf) {
                        had_packet = true;
                        if let Ok(proto_sacn::SacnPacket::Data(data)) = proto_sacn::parse_sacn(&rx_buf[..amt]) {
                            let univ = data.universe;
                            if args.universe.map_or(true, |u| u == univ) {
                                packet_count += 1;
                                let preview: Vec<String> = data.data.iter().take(args.channels).map(|c| format!("{:3}", c)).collect();
                                println!(
                                    "[{:>6.2}s] sACN    | Src: {:<15} | Univ: {:<3} | Pri: {:<3} | Chs({}): [{}]",
                                    start.elapsed().as_secs_f32(),
                                    src.ip().to_string(),
                                    univ,
                                    data.priority,
                                    data.data.len(),
                                    preview.join(",")
                                );
                            }
                        }
                    }
                }

                if !had_packet {
                    sleep(Duration::from_millis(5));
                }
            }
        }

        Commands::Test(args) => {
            if args.remote {
                let url = format!("{}/api/test", args.node.trim_end_matches('/'));
                let rgb = args.color.as_deref().and_then(parse_rgb);
                let payload = serde_json::json!({
                    "mode": args.mode,
                    "color": rgb
                });

                match ureq::post(&url).timeout(Duration::from_secs(3)).send_json(payload) {
                    Ok(_) => println!("Successfully triggered remote test pattern '{}' on node '{}'", args.mode, args.node),
                    Err(e) => eprintln!("Failed to trigger remote test pattern: {}", e),
                }
                return;
            }

            // Local SPI / virtual test execution
            let pattern_mode = match args.mode.to_lowercase().as_str() {
                "rgbw" => TestPatternMode::RgbwCycle,
                "walker" => TestPatternMode::AddressWalker,
                "soak" => TestPatternMode::PowerSoak100,
                "strobe" => TestPatternMode::HighRateStrobe40Hz,
                _ => TestPatternMode::RgbwCycle,
            };

            let c_order = parse_color_order(&args.color_order);
            let engine = RenderEngine::new(c_order, 20_000);

            println!("==================================================");
            println!(" Rig Check: {:?} on {} pixels (ColorOrder: {:?})", pattern_mode, args.pixels, c_order);
            if let Some(ref path) = args.spi {
                println!(" Output Target: SPI device '{}'", path);
            } else {
                println!(" Output Target: Virtual Console");
            }
            println!(" Press Ctrl+C to terminate");
            println!("==================================================");

            #[cfg(target_os = "linux")]
            let mut spi_sink = if let Some(ref path) = args.spi {
                match output::spi_ws281x::SpiWs281xSink::open(path, args.pixels) {
                    Ok(sink) => Some(sink),
                    Err(e) => {
                        eprintln!("Failed to open SPI {}: {}", path, e);
                        return;
                    }
                }
            } else {
                None
            };

            let mut out_pixels = vec![RgbColor::default(); args.pixels];
            let mut byte_buffer = vec![0u8; args.pixels * 3];

            let start = Instant::now();
            let mut frame_count: u64 = 0;

            loop {
                let elapsed = start.elapsed();
                if args.duration > 0 && elapsed.as_secs() >= args.duration {
                    println!("\nTest completed after {}s.", args.duration);
                    break;
                }

                if args.mode == "color" {
                    let rgb = args.color.as_deref().and_then(parse_rgb).unwrap_or([255, 0, 0]);
                    out_pixels.fill(RgbColor { r: rgb[0], g: rgb[1], b: rgb[2] });
                } else {
                    engine.render_test_pattern(pattern_mode, elapsed, &mut out_pixels);
                }
                engine.pack_bytes(&out_pixels, &mut byte_buffer);

                #[cfg(target_os = "linux")]
                if let Some(ref mut sink) = spi_sink {
                    if let Err(e) = sink.write_pixels(&byte_buffer) {
                        eprintln!("Error writing to SPI: {}", e);
                    }
                }

                if args.spi.is_none() && frame_count % 50 == 0 {
                    let first = out_pixels[0];
                    let mid = out_pixels[args.pixels / 2];
                    println!(
                        "[{:>5.1}s] Px 0: RGB({:3},{:3},{:3}) | Px {}: RGB({:3},{:3},{:3})",
                        elapsed.as_secs_f32(),
                        first.r, first.g, first.b,
                        args.pixels / 2,
                        mid.r, mid.g, mid.b
                    );
                }

                frame_count += 1;
                sleep(Duration::from_millis(20));
            }
        }

        Commands::Config(cfg_cmd) => match cfg_cmd.action {
            ConfigSubcommands::Show { node, path } => {
                if let Some(file_path) = path {
                    match fs::read_to_string(&file_path) {
                        Ok(content) => println!("{}", content),
                        Err(e) => eprintln!("Failed to read file '{}': {}", file_path, e),
                    }
                } else {
                    let url = format!("{}/api/config", node.trim_end_matches('/'));
                    match ureq::get(&url).timeout(Duration::from_secs(3)).call() {
                        Ok(resp) => {
                            let json_str = resp.into_string().unwrap_or_default();
                            match serde_json::from_str::<serde_json::Value>(&json_str) {
                                Ok(val) => match toml::to_string_pretty(&val) {
                                    Ok(toml_str) => println!("{}", toml_str),
                                    Err(_) => println!("{}", json_str),
                                },
                                Err(_) => println!("{}", json_str),
                            }
                        }
                        Err(e) => eprintln!("Failed to query config from node: {}", e),
                    }
                }
            }

            ConfigSubcommands::Validate { path } => {
                println!("Validating configuration file: '{}'...", path);
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("FAIL: could not read file: {}", e);
                        return;
                    }
                };

                let val: toml::Value = match toml::from_str(&content) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("FAIL: TOML syntax error: {}", e);
                        return;
                    }
                };

                // Validate required sections
                let mut errors = 0;
                if val.get("node").is_none() {
                    eprintln!("- Missing [node] section");
                    errors += 1;
                }
                if val.get("output").is_none() {
                    eprintln!("- Missing [[output]] section");
                    errors += 1;
                }
                if val.get("fixture").is_none() {
                    eprintln!("- Missing [[fixture]] section");
                    errors += 1;
                }

                if errors == 0 {
                    println!("PASS: Configuration TOML syntax and structure are valid.");
                } else {
                    eprintln!("FAIL: {} error(s) found in configuration.", errors);
                }
            }

            ConfigSubcommands::Set {
                node,
                name,
                personality,
                universe,
                address,
                pixels,
                color_order,
                max_current_ma,
                failsafe,
            } => {
                let get_url = format!("{}/api/config", node.trim_end_matches('/'));
                let mut current_cfg: serde_json::Value = match ureq::get(&get_url).timeout(Duration::from_secs(3)).call() {
                    Ok(resp) => resp.into_json().unwrap_or(serde_json::json!({})),
                    Err(e) => {
                        eprintln!("Failed to fetch current config: {}", e);
                        return;
                    }
                };

                if let Some(n) = name {
                    current_cfg["node"]["name"] = serde_json::json!(n);
                }
                if let Some(p) = personality {
                    if let Some(f) = current_cfg["fixture"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        f["personality"] = serde_json::json!(p);
                    }
                }
                if let Some(u) = universe {
                    if let Some(f) = current_cfg["fixture"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        f["universe"] = serde_json::json!(u);
                    }
                }
                if let Some(a) = address {
                    if let Some(f) = current_cfg["fixture"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        f["address"] = serde_json::json!(a);
                    }
                }
                if let Some(px) = pixels {
                    if let Some(o) = current_cfg["output"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        o["pixels"] = serde_json::json!(px);
                    }
                    if let Some(f) = current_cfg["fixture"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        f["pixel_range"] = serde_json::json!([0, px - 1]);
                    }
                }
                if let Some(co) = color_order {
                    if let Some(o) = current_cfg["output"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        o["color_order"] = serde_json::json!(co);
                    }
                }
                if let Some(ma) = max_current_ma {
                    if let Some(o) = current_cfg["output"].as_array_mut().and_then(|a| a.get_mut(0)) {
                        o["psu_max_current_ma"] = serde_json::json!(ma);
                    }
                }
                if let Some(fs_policy) = failsafe {
                    current_cfg["failsafe"]["on_data_loss"] = serde_json::json!(fs_policy);
                }

                let post_url = format!("{}/api/config", node.trim_end_matches('/'));
                match ureq::post(&post_url).timeout(Duration::from_secs(3)).send_json(current_cfg) {
                    Ok(_) => println!("Configuration updated successfully on '{}'", node),
                    Err(e) => eprintln!("Failed to update config: {}", e),
                }
            }
        },

        Commands::Lock(args) => {
            let target_state = if args.enable {
                true
            } else if args.disable {
                false
            } else {
                eprintln!("Specify either --enable or --disable");
                return;
            };

            let url = format!("{}/api/lock", args.node.trim_end_matches('/'));
            match ureq::post(&url).timeout(Duration::from_secs(3)).send_json(serde_json::json!({
                "show_lock": target_state
            })) {
                Ok(_) => {
                    if target_state {
                        println!("Node locked successfully (Show Lock: ACTIVE 🔒)");
                    } else {
                        println!("Node unlocked successfully (Show Lock: INACTIVE 🔓)");
                    }
                }
                Err(e) => eprintln!("Failed to toggle lock: {}", e),
            }
        }

        Commands::SendArtnet(args) => {
            let data = parse_csv_channels(&args.channels);
            if data.len() < 2 {
                eprintln!("Provide at least 2 channel values (e.g. --channels 255,128)");
                return;
            }

            let mut packet_buf = [0u8; 530];
            let len = proto_artnet::write_art_dmx(&mut packet_buf, 1, 0, args.universe, &data)
                .expect("Failed to serialize ArtDmx");

            let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
            let target = format!("{}:{}", args.ip, proto_artnet::ARTNET_PORT);
            socket
                .send_to(&packet_buf[..len], &target)
                .expect("Failed to send Art-Net packet");

            println!("Sent ArtDmx: {} channels to {} (Universe {})", data.len(), target, args.universe);
        }

        Commands::SendSacn(args) => {
            let data = parse_csv_channels(&args.channels);
            if data.is_empty() {
                eprintln!("Provide at least 1 channel value (e.g. --channels 255,128)");
                return;
            }

            let mut packet_buf = [0u8; 640];
            let cid = [0x55; 16];
            let len = proto_sacn::write_sacn_data(
                &mut packet_buf,
                &cid,
                "pxtool",
                args.priority,
                args.universe,
                1,
                &data,
            )
            .expect("Failed to serialize sACN packet");

            let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
            let target = format!("{}:{}", args.ip, proto_sacn::SACN_PORT);
            socket
                .send_to(&packet_buf[..len], &target)
                .expect("Failed to send sACN packet");

            println!("Sent sACN: {} channels to {} (Universe {}, Priority {})", data.len(), target, args.universe, args.priority);
        }
    }
}
