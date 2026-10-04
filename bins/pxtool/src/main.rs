//! `pxtool`: Field diagnosis, standalone rig-check, and DMX transmission CLI.

use clap::{Parser, Subcommand};
use engine::{ColorOrder, RenderEngine, RgbColor, TestPatternMode};
use output::PixelSink;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "pxtool", author, version, about = "Pixel LED Node Field Diagnostic & Rig Check Utility")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run standalone rig-check self-test patterns without lighting console
    Test {
        /// Test mode: rgbw | walker | soak | strobe
        #[arg(short, long, default_value = "rgbw")]
        mode: String,

        /// Number of LED pixels
        #[arg(short, long, default_value_t = 300)]
        pixels: usize,

        /// Color order: rgb | grb | brg
        #[arg(short, long, default_value = "brg")]
        color_order: String,

        /// SPI device path (omit to run virtual console output)
        #[arg(long)]
        spi: Option<String>,

        /// Maximum duration in seconds (0 = run forever)
        #[arg(short, long, default_value_t = 0)]
        duration: u64,
    },

    /// Transmit test Art-Net DMX frame
    SendArtnet {
        /// Target IP address (e.g. 127.0.0.1 or broadcast 2.255.255.255)
        #[arg(long, default_value = "127.0.0.1")]
        ip: String,

        /// Universe (15-bit port address)
        #[arg(short, long, default_value_t = 1)]
        universe: u16,

        /// Raw DMX channel bytes (comma separated, e.g. "255,0,128")
        #[arg(long)]
        channels: String,
    },

    /// Transmit test sACN DMX frame
    SendSacn {
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
    },
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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Test {
            mode,
            pixels,
            color_order,
            spi,
            duration,
        } => {
            let pattern_mode = match mode.to_lowercase().as_str() {
                "rgbw" => TestPatternMode::RgbwCycle,
                "walker" => TestPatternMode::AddressWalker,
                "soak" => TestPatternMode::PowerSoak100,
                "strobe" => TestPatternMode::HighRateStrobe40Hz,
                _ => {
                    eprintln!("Unknown mode '{}'. Using 'rgbw'.", mode);
                    TestPatternMode::RgbwCycle
                }
            };

            let c_order = parse_color_order(&color_order);
            let engine = RenderEngine::new(c_order, 20_000); // 20A limit

            println!("==================================================");
            println!(" Rig Check: {:?} on {} pixels (ColorOrder: {:?})", pattern_mode, pixels, c_order);
            if let Some(ref path) = spi {
                println!(" Output Target: SPI device '{}'", path);
            } else {
                println!(" Output Target: Virtual Console");
            }
            println!(" Press Ctrl+C to terminate");
            println!("==================================================");

            #[cfg(target_os = "linux")]
            let mut spi_sink = if let Some(ref path) = spi {
                match output::spi_ws281x::SpiWs281xSink::open(path, pixels) {
                    Ok(sink) => Some(sink),
                    Err(e) => {
                        eprintln!("Failed to open SPI {}: {}", path, e);
                        return;
                    }
                }
            } else {
                None
            };

            let mut out_pixels = vec![RgbColor::default(); pixels];
            let mut byte_buffer = vec![0u8; pixels * 3];

            let start = Instant::now();
            let mut frame_count: u64 = 0;

            loop {
                let elapsed = start.elapsed();
                if duration > 0 && elapsed.as_secs() >= duration {
                    println!("\nTest completed after {}s.", duration);
                    break;
                }

                engine.render_test_pattern(pattern_mode, elapsed, &mut out_pixels);
                engine.pack_bytes(&out_pixels, &mut byte_buffer);

                #[cfg(target_os = "linux")]
                if let Some(ref mut sink) = spi_sink {
                    if let Err(e) = sink.write_pixels(&byte_buffer) {
                        eprintln!("Error writing to SPI: {}", e);
                    }
                }

                if spi.is_none() && frame_count % 50 == 0 {
                    let first = out_pixels[0];
                    let mid = out_pixels[pixels / 2];
                    println!(
                        "[{:>5.1}s] Px 0: RGB({:3},{:3},{:3}) | Px {}: RGB({:3},{:3},{:3})",
                        elapsed.as_secs_f32(),
                        first.r, first.g, first.b,
                        pixels / 2,
                        mid.r, mid.g, mid.b
                    );
                }

                frame_count += 1;
                sleep(Duration::from_millis(20)); // ~50 fps
            }
        }

        Commands::SendArtnet { ip, universe, channels } => {
            let data = parse_csv_channels(&channels);
            if data.len() < 2 {
                eprintln!("Provide at least 2 channel values (e.g. --channels 255,128)");
                return;
            }

            let mut packet_buf = [0u8; 530];
            let len = proto_artnet::write_art_dmx(&mut packet_buf, 1, 0, universe, &data)
                .expect("Failed to serialize ArtDmx");

            let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
            let target = format!("{}:{}", ip, proto_artnet::ARTNET_PORT);
            socket
                .send_to(&packet_buf[..len], &target)
                .expect("Failed to send Art-Net packet");

            println!("Sent ArtDmx: {} channels to {} (Universe {})", data.len(), target, universe);
        }

        Commands::SendSacn { ip, universe, priority, channels } => {
            let data = parse_csv_channels(&channels);
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
                priority,
                universe,
                1,
                &data,
            )
            .expect("Failed to serialize sACN packet");

            let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket");
            let target = format!("{}:{}", ip, proto_sacn::SACN_PORT);
            socket
                .send_to(&packet_buf[..len], &target)
                .expect("Failed to send sACN packet");

            println!("Sent sACN: {} channels to {} (Universe {}, Priority {})", data.len(), target, universe, priority);
        }
    }
}
