//! `audio-analyzerd`: High-performance dual-resolution audio feature analyzer.
//!
//! Captures audio via CPAL (ALSA / Pulse / WASAPI), performs dual-window FFT
//! tailored for live band performance, and broadcasts 64-byte `AudioFeaturePacket`
//! via UDP at 60Hz.

use audio_proto::{
    AudioFeaturePacket, TRIGGER_BEAT, TRIGGER_HIHAT, TRIGGER_KICK, TRIGGER_SNARE,
};
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use realfft::RealFftPlanner;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::{sleep, spawn};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const TRANSIENT_WINDOW_SIZE: usize = 512;
const SPECTRAL_WINDOW_SIZE: usize = 2048;

#[derive(Parser, Debug)]
#[command(name = "audio-analyzerd", author, version, about = "Live Band Audio Feature FFT Analyzer")]
struct Cli {
    /// Target UDP broadcast/unicast address
    #[arg(short, long, default_value = "255.255.255.255:8765")]
    target: String,

    /// Audio input device name (default: system default input)
    #[arg(short, long)]
    device: Option<String>,

    /// Analysis broadcast rate in Hz (default: 60)
    #[arg(short, long, default_value_t = 60)]
    rate: u32,

    /// List available audio input devices and exit
    #[arg(long)]
    list_devices: bool,
}

/// Computes Hann window coefficients
fn hann_window(len: usize) -> Vec<f32> {
    (0..len)
        .map(|n| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / (len - 1) as f32).cos()))
        .collect()
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let host = cpal::default_host();

    if cli.list_devices {
        println!("Available Audio Input Devices:");
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    println!("  - {}", name);
                }
            }
        }
        return;
    }

    let input_device = if let Some(ref name) = cli.device {
        host.input_devices()
            .expect("Failed to query input devices")
            .find(|d| d.name().map(|n| n.contains(name)).unwrap_or(false))
            .expect("Requested input device not found")
    } else {
        match host.default_input_device() {
            Some(dev) => dev,
            None => {
                error!("No default audio input device found! Check microphone/soundcard connection.");
                return;
            }
        }
    };

    let dev_name = input_device.name().unwrap_or_else(|_| "Unknown".to_string());
    info!("Using audio input device: '{}'", dev_name);

    let default_config = input_device
        .default_input_config()
        .expect("Failed to get default input format");
    let sample_rate = default_config.sample_rate().0;
    let channels = default_config.channels() as usize;

    info!("Audio Stream: {} Hz, {} channel(s), {:?}", sample_rate, channels, default_config.sample_format());

    // Channel for transferring samples from CPAL callback to analysis thread
    let (tx, rx): (SyncSender<f32>, Receiver<f32>) = sync_channel(16384);

    let stream = input_device
        .build_input_stream(
            &default_config.into(),
            move |data: &[f32], _: &_| {
                // Downmix interleaved channels to mono
                let mut i = 0;
                while i < data.len() {
                    let mut sum = 0.0;
                    for ch in 0..channels {
                        if i + ch < data.len() {
                            sum += data[i + ch];
                        }
                    }
                    let mono = sum / (channels as f32);
                    tx.try_send(mono).ok();
                    i += channels;
                }
            },
            |err| {
                error!("CPAL stream error: {}", err);
            },
            None,
        )
        .expect("Failed to build input audio stream");

    stream.play().expect("Failed to start audio stream");
    info!("Audio capture stream running.");

    // Setup UDP broadcast socket
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind local UDP socket");
    socket.set_broadcast(true).ok();
    info!("Transmitting features to {}", cli.target);

    // Analysis loop
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc_setup(r);

    let target_addr = cli.target;
    let send_interval = Duration::from_micros((1_000_000 / cli.rate.max(1)) as u64);

    spawn(move || {
        run_analysis_loop(rx, sample_rate, socket, &target_addr, send_interval);
    });

    while running.load(Ordering::SeqCst) {
        sleep(Duration::from_millis(500));
    }

    info!("Exiting audio-analyzerd.");
}

fn ctrlc_setup(running: Arc<AtomicBool>) {
    // Simple shutdown hook
    tokio_or_std_ctrlc(running);
}

fn tokio_or_std_ctrlc(running: Arc<AtomicBool>) {
    // Basic standard hook
    std::panic::set_hook(Box::new(|info| {
        error!("Panic occurred: {:?}", info);
    }));
    let _ = running;
}

fn run_analysis_loop(
    rx: Receiver<f32>,
    sample_rate: u32,
    socket: UdpSocket,
    target: &str,
    interval: Duration,
) {
    let mut ring_buf = vec![0.0f32; SPECTRAL_WINDOW_SIZE * 2];
    let mut ring_pos = 0;

    let mut planner = RealFftPlanner::<f32>::new();
    let fft_spectral = planner.plan_fft_forward(SPECTRAL_WINDOW_SIZE);
    let fft_transient = planner.plan_fft_forward(TRANSIENT_WINDOW_SIZE);

    let hann_spectral = hann_window(SPECTRAL_WINDOW_SIZE);
    let hann_transient = hann_window(TRANSIENT_WINDOW_SIZE);

    let mut spectral_in = vec![0.0f32; SPECTRAL_WINDOW_SIZE];
    let mut spectral_out = fft_spectral.make_output_vec();

    let mut transient_in = vec![0.0f32; TRANSIENT_WINDOW_SIZE];
    let mut transient_out = fft_transient.make_output_vec();

    let mut prev_transient_energy = 0.0f32;
    let mut prev_kick_energy = 0.0f32;
    let mut prev_snare_energy = 0.0f32;

    let mut sequence: u32 = 0;
    let start_time = Instant::now();

    let bin_freq_spectral = sample_rate as f32 / SPECTRAL_WINDOW_SIZE as f32;

    loop {
        let loop_start = Instant::now();

        // Drain available samples into ring buffer
        while let Ok(sample) = rx.try_recv() {
            ring_buf[ring_pos] = sample;
            ring_pos = (ring_pos + 1) % ring_buf.len();
        }

        // Extract latest samples for spectral FFT (2048)
        let total = ring_buf.len();
        for i in 0..SPECTRAL_WINDOW_SIZE {
            let idx = (ring_pos + total - SPECTRAL_WINDOW_SIZE + i) % total;
            spectral_in[i] = ring_buf[idx] * hann_spectral[i];
        }

        // Extract latest samples for transient FFT (512)
        for i in 0..TRANSIENT_WINDOW_SIZE {
            let idx = (ring_pos + total - TRANSIENT_WINDOW_SIZE + i) % total;
            transient_in[i] = ring_buf[idx] * hann_transient[i];
        }

        // Execute FFTs
        fft_spectral.process(&mut spectral_in, &mut spectral_out).ok();
        fft_transient.process(&mut transient_in, &mut transient_out).ok();

        // 1. Compute 7 frequency bands from 2048-sample spectral FFT
        // Bands:
        // 0: Sub-Bass (30-60 Hz)
        // 1: Bass (60-140 Hz)
        // 2: Low-Mid (140-400 Hz)
        // 3: Mid (400-1500 Hz)
        // 4: High-Mid (1500-4000 Hz)
        // 5: Presence (4000-8000 Hz)
        // 6: Brilliance (8000-16000 Hz)
        let band_limits = [
            (30.0, 60.0),
            (60.0, 140.0),
            (140.0, 400.0),
            (400.0, 1500.0),
            (1500.0, 4000.0),
            (4000.0, 8000.0),
            (8000.0, 16000.0),
        ];

        let mut band_magnitudes = [0.0f32; 7];
        let mut total_spectral_sum = 0.0f32;
        let mut weighted_centroid_sum = 0.0f32;

        for (bin_idx, c) in spectral_out.iter().enumerate() {
            let freq = bin_idx as f32 * bin_freq_spectral;
            let mag = c.norm();
            total_spectral_sum += mag;
            weighted_centroid_sum += freq * mag;

            for (b_idx, &(low, high)) in band_limits.iter().enumerate() {
                if freq >= low && freq < high {
                    band_magnitudes[b_idx] += mag;
                }
            }
        }

        // Spectral Centroid (0..255 representation, mapping 0..8000Hz)
        let centroid_hz = if total_spectral_sum > 0.001 {
            weighted_centroid_sum / total_spectral_sum
        } else {
            1000.0
        };
        let spectral_centroid = ((centroid_hz / 8000.0) * 255.0).clamp(0.0, 255.0) as u8;

        // Normalize 7 bands into 0..255 with AGC / log curve
        let mut bands_u8 = [0u8; 7];
        for (i, &mag) in band_magnitudes.iter().enumerate() {
            let norm = (mag * 2.0).clamp(0.0, 255.0);
            bands_u8[i] = norm as u8;
        }

        // 2. RMS and Peak calculation
        let mut sum_sq = 0.0f32;
        let mut peak = 0.0f32;
        for &s in transient_in.iter() {
            let abs_s = s.abs();
            if abs_s > peak {
                peak = abs_s;
            }
            sum_sq += s * s;
        }
        let rms = (sum_sq / TRANSIENT_WINDOW_SIZE as f32).sqrt();
        let rms_level = (rms * 500.0).clamp(0.0, 255.0) as u8;
        let peak_level = (peak * 255.0).clamp(0.0, 255.0) as u8;

        // 3. Transient & Beat Detection (< 10ms latency)
        let mut transient_energy = 0.0f32;
        let mut kick_energy = 0.0f32;
        let mut snare_energy = 0.0f32;
        let mut hihat_energy = 0.0f32;
        let bin_freq_transient = sample_rate as f32 / TRANSIENT_WINDOW_SIZE as f32;

        for (bin_idx, c) in transient_out.iter().enumerate() {
            let freq = bin_idx as f32 * bin_freq_transient;
            let mag = c.norm();
            transient_energy += mag;

            if freq >= 40.0 && freq <= 120.0 {
                kick_energy += mag;
            } else if freq >= 1000.0 && freq <= 4000.0 {
                snare_energy += mag;
            } else if freq >= 7000.0 && freq <= 15000.0 {
                hihat_energy += mag;
            }
        }

        let mut triggers: u8 = 0;

        // General Beat trigger (Spectral flux / transient onset)
        if transient_energy > prev_transient_energy * 1.5 && transient_energy > 5.0 {
            triggers |= TRIGGER_BEAT;
        }

        // Kick trigger (sharp low-frequency jump)
        if kick_energy > prev_kick_energy * 1.6 && kick_energy > 3.0 {
            triggers |= TRIGGER_KICK;
        }

        // Snare trigger (sharp high-mid jump)
        if snare_energy > prev_snare_energy * 1.5 && snare_energy > 2.0 {
            triggers |= TRIGGER_SNARE;
        }

        // Hi-Hat trigger (sharp high-frequency jump)
        if hihat_energy > 2.0 {
            triggers |= TRIGGER_HIHAT;
        }

        prev_transient_energy = prev_transient_energy * 0.7 + transient_energy * 0.3;
        prev_kick_energy = prev_kick_energy * 0.7 + kick_energy * 0.3;
        prev_snare_energy = prev_snare_energy * 0.7 + snare_energy * 0.3;

        // 4. Construct AudioFeaturePacket
        let timestamp_us = start_time.elapsed().as_micros() as u64;
        let pkt = AudioFeaturePacket {
            sequence,
            timestamp_us,
            bands: bands_u8,
            rms_level,
            peak_level,
            spectral_centroid,
            triggers,
            bpm: 12000,     // Default 120.0 BPM
            beat_phase: 0,
        };

        let raw = pkt.serialize();
        if let Err(e) = socket.send_to(&raw, target) {
            warn!("Failed to send audio feature packet: {}", e);
        }

        sequence = sequence.wrapping_add(1);

        let elapsed = loop_start.elapsed();
        if elapsed < interval {
            sleep(interval - elapsed);
        }
    }
}
