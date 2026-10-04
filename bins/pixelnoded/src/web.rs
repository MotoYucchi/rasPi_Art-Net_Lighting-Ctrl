//! Web management interface and REST API server for `pixelnoded`.

use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Instant;
use tiny_http::{Header, Response, Server, StatusCode};
use tracing::{info, warn};

pub const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UniverseStatusInfo {
    pub universe: u16,
    pub source: String,
    pub healthy: bool,
    pub age_ms: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioTelemetryInfo {
    pub connected: bool,
    pub bpm: f32,
    pub energy: f32,
    pub kick: bool,
    pub snare: bool,
    pub hihat: bool,
}

pub enum WebCommand {
    SetTestMode(u8, [u8; 3]),
    SetLock(bool),
    ApplyConfig(Box<crate::Config>),
}

pub struct SharedWebState {
    pub node_name: RwLock<String>,
    pub show_lock: AtomicBool,
    pub fps_x10: AtomicU32,
    pub frame_count: AtomicU64,
    pub artnet_rx: AtomicU64,
    pub sacn_rx: AtomicU64,
    pub current_ma: AtomicU32,
    pub max_psu_ma: AtomicU32,
    pub failsafe_active: AtomicBool,
    pub test_mode: AtomicU8,
    pub custom_rgb: [AtomicU8; 3],
    pub channels: RwLock<Vec<u8>>,
    pub universes: RwLock<Vec<UniverseStatusInfo>>,
    pub audio: RwLock<AudioTelemetryInfo>,
    pub config: RwLock<crate::Config>,
    pub config_path: String,
    pub start_time: Instant,
    pub cmd_tx: Sender<WebCommand>,
}

#[derive(serde::Serialize)]
struct StatusResponse<'a> {
    node_name: String,
    version: &'static str,
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
    channels: &'a [u8],
    universes: Vec<UniverseStatusInfo>,
    audio: AudioTelemetryInfo,
}

#[derive(serde::Deserialize)]
struct TestRequest {
    mode: String,
    #[serde(default)]
    color: Option<[u8; 3]>,
}

#[derive(serde::Deserialize)]
struct LockRequest {
    show_lock: bool,
}

pub fn start_web_server(
    bind_ip: &str,
    port: u16,
    state: Arc<SharedWebState>,
) -> Result<thread::JoinHandle<()>, std::io::Error> {
    let addr = format!("{}:{}", bind_ip, port);
    let server = Server::http(&addr).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("tiny_http bind error: {}", e))
    })?;

    info!("PixelNode Web Management UI & REST API listening on http://{}", addr);

    let handle = thread::Builder::new()
        .name("pixelnode-web".to_string())
        .spawn(move || {
            let json_header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).ok();
            let html_header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).ok();
            let cors_header = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).ok();

            for mut req in server.incoming_requests() {
                let url = req.url().to_string();
                let method = req.method().as_str().to_uppercase();

                if method == "OPTIONS" {
                    let mut resp = Response::empty(StatusCode(204));
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    req.respond(resp).ok();
                    continue;
                }

                if (method == "GET" || method == "HEAD") && (url == "/" || url == "/index.html") {
                    let mut resp = Response::from_string(DASHBOARD_HTML);
                    if let Some(ref h) = html_header {
                        resp.add_header(h.clone());
                    }
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    req.respond(resp).ok();
                    continue;
                }

                if method == "GET" && url == "/api/status" {
                    let node_name = state.node_name.read().map(|n| n.clone()).unwrap_or_default();
                    let chs_guard = state.channels.read().ok();
                    let chs_slice = chs_guard.as_deref().map(|v| v.as_slice()).unwrap_or(&[]);
                    let univs = state.universes.read().map(|u| u.clone()).unwrap_or_default();
                    let audio = state.audio.read().map(|a| a.clone()).unwrap_or_default();

                    let status_payload = StatusResponse {
                        node_name,
                        version: env!("CARGO_PKG_VERSION"),
                        uptime_sec: state.start_time.elapsed().as_secs(),
                        show_lock: state.show_lock.load(Ordering::Relaxed),
                        fps_x10: state.fps_x10.load(Ordering::Relaxed),
                        frame_count: state.frame_count.load(Ordering::Relaxed),
                        artnet_rx: state.artnet_rx.load(Ordering::Relaxed),
                        sacn_rx: state.sacn_rx.load(Ordering::Relaxed),
                        current_ma: state.current_ma.load(Ordering::Relaxed),
                        max_psu_ma: state.max_psu_ma.load(Ordering::Relaxed),
                        failsafe_active: state.failsafe_active.load(Ordering::Relaxed),
                        test_mode: state.test_mode.load(Ordering::Relaxed),
                        channels: chs_slice,
                        universes: univs,
                        audio,
                    };

                    let body = serde_json::to_string(&status_payload).unwrap_or_default();
                    let mut resp = Response::from_string(body);
                    if let Some(ref h) = json_header {
                        resp.add_header(h.clone());
                    }
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    req.respond(resp).ok();
                    continue;
                }

                if method == "GET" && url == "/api/config" {
                    let cfg_json = match state.config.read() {
                        Ok(c) => serde_json::to_string(&*c).unwrap_or_default(),
                        Err(_) => "{}".to_string(),
                    };
                    let mut resp = Response::from_string(cfg_json);
                    if let Some(ref h) = json_header {
                        resp.add_header(h.clone());
                    }
                    if let Some(ref h) = cors_header {
                        resp.add_header(h.clone());
                    }
                    req.respond(resp).ok();
                    continue;
                }

                if method == "POST" && url == "/api/lock" {
                    let mut body_str = String::new();
                    req.as_reader().read_to_string(&mut body_str).ok();
                    match serde_json::from_str::<LockRequest>(&body_str) {
                        Ok(lr) => {
                            state.show_lock.store(lr.show_lock, Ordering::SeqCst);
                            state.cmd_tx.send(WebCommand::SetLock(lr.show_lock)).ok();
                            let mut resp = Response::from_string(r#"{"status":"ok"}"#);
                            if let Some(ref h) = json_header {
                                resp.add_header(h.clone());
                            }
                            req.respond(resp).ok();
                        }
                        Err(e) => {
                            req.respond(Response::from_string(format!("Invalid JSON: {}", e)).with_status_code(StatusCode(400))).ok();
                        }
                    }
                    continue;
                }

                if method == "POST" && url == "/api/test" {
                    if state.show_lock.load(Ordering::Relaxed) {
                        req.respond(Response::from_string("Node is show-locked").with_status_code(StatusCode(403))).ok();
                        continue;
                    }

                    let mut body_str = String::new();
                    req.as_reader().read_to_string(&mut body_str).ok();
                    match serde_json::from_str::<TestRequest>(&body_str) {
                        Ok(tr) => {
                            let (m_val, rgb) = match tr.mode.to_lowercase().as_str() {
                                "rgbw" => (1, [0, 0, 0]),
                                "walker" => (2, [0, 0, 0]),
                                "soak" => (3, [0, 0, 0]),
                                "strobe" => (4, [0, 0, 0]),
                                "color" => (5, tr.color.unwrap_or([255, 0, 0])),
                                _ => (0, [0, 0, 0]),
                            };
                            state.test_mode.store(m_val, Ordering::SeqCst);
                            state.custom_rgb[0].store(rgb[0], Ordering::SeqCst);
                            state.custom_rgb[1].store(rgb[1], Ordering::SeqCst);
                            state.custom_rgb[2].store(rgb[2], Ordering::SeqCst);
                            state.cmd_tx.send(WebCommand::SetTestMode(m_val, rgb)).ok();

                            let mut resp = Response::from_string(r#"{"status":"ok"}"#);
                            if let Some(ref h) = json_header {
                                resp.add_header(h.clone());
                            }
                            req.respond(resp).ok();
                        }
                        Err(e) => {
                            req.respond(Response::from_string(format!("Invalid JSON: {}", e)).with_status_code(StatusCode(400))).ok();
                        }
                    }
                    continue;
                }

                if method == "POST" && url == "/api/config" {
                    if state.show_lock.load(Ordering::Relaxed) {
                        req.respond(Response::from_string("Node is show-locked: cannot change configuration").with_status_code(StatusCode(403))).ok();
                        continue;
                    }

                    let mut body_str = String::new();
                    req.as_reader().read_to_string(&mut body_str).ok();
                    match serde_json::from_str::<crate::Config>(&body_str) {
                        Ok(new_cfg) => {
                            // Serialize to TOML and write back to disk
                            match toml::to_string_pretty(&new_cfg) {
                                Ok(toml_str) => {
                                    if let Err(e) = fs::write(&state.config_path, toml_str) {
                                        warn!("Failed to write config file {}: {}", state.config_path, e);
                                    } else {
                                        info!("Atomically saved configuration to '{}'", state.config_path);
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to serialize TOML: {}", e);
                                }
                            }

                            // Update in-memory state
                            if let Ok(mut node_name) = state.node_name.write() {
                                *node_name = new_cfg.node.name.clone();
                            }
                            if let Ok(mut cfg_guard) = state.config.write() {
                                *cfg_guard = new_cfg.clone();
                            }

                            // Send apply command to render engine
                            state.cmd_tx.send(WebCommand::ApplyConfig(Box::new(new_cfg))).ok();

                            let mut resp = Response::from_string(r#"{"status":"ok","message":"Config saved and applied"}"#);
                            if let Some(ref h) = json_header {
                                resp.add_header(h.clone());
                            }
                            req.respond(resp).ok();
                        }
                        Err(e) => {
                            req.respond(Response::from_string(format!("Invalid config JSON: {}", e)).with_status_code(StatusCode(400))).ok();
                        }
                    }
                    continue;
                }

                // 404 for anything else
                req.respond(Response::from_string("Not Found").with_status_code(StatusCode(404))).ok();
            }
        })?;

    Ok(handle)
}
