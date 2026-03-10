// Screenshot generation for pedalboards.
// Composites plugin screenshots onto a canvas and saves screenshot.png + thumbnail.png.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{DynamicImage, Rgba, RgbaImage};
use tokio::sync::{mpsc, Notify};

use crate::lv2_utils;
use crate::settings::Settings;

const MAX_THUMB_WIDTH: u32 = 640;
const MAX_THUMB_HEIGHT: u32 = 640;

// Cable colors (R, G, B)
const AUDIO_COLOR: (u8, u8, u8) = (0x81, 0x00, 0x9A); // purple
const MIDI_COLOR: (u8, u8, u8) = (0x00, 0x54, 0x6C); // teal
const CV_COLOR: (u8, u8, u8) = (0xBB, 0x67, 0x36); // orange
const CABLE_WIDTH: f64 = 7.0;

/// Handle to the screenshot generator, safe to clone and share.
#[derive(Clone)]
pub struct ScreenshotGenerator {
    tx: mpsc::UnboundedSender<ScreenshotRequest>,
    state: Arc<tokio::sync::RwLock<GeneratorState>>,
}

struct ScreenshotRequest {
    bundlepath: PathBuf,
}

struct GeneratorState {
    /// Bundlepaths currently queued or being processed
    pending: HashMap<PathBuf, Arc<Notify>>,
}

impl ScreenshotGenerator {
    pub fn new(settings: &Settings) -> Self {
        let html_dir = settings.html_dir.clone();
        let state = Arc::new(tokio::sync::RwLock::new(GeneratorState {
            pending: HashMap::new(),
        }));

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn the background worker
        let worker_state = state.clone();
        actix_web::rt::spawn(async move {
            screenshot_worker(rx, worker_state, html_dir).await;
        });

        Self { tx, state }
    }

    /// Queue a screenshot for generation.
    pub fn schedule_screenshot(&self, bundlepath: &Path) {
        let bp = bundlepath.to_path_buf();
        let _ = self.tx.send(ScreenshotRequest { bundlepath: bp });
    }

    /// Check screenshot status: 1 = exists, 0 = queued/processing, -1 = missing.
    pub async fn check_screenshot(&self, bundlepath: &Path) -> i32 {
        let screenshot = bundlepath.join("screenshot.png");
        if screenshot.exists() {
            1
        } else {
            let state = self.state.read().await;
            if state.pending.contains_key(bundlepath) {
                0
            } else {
                -1
            }
        }
    }

    /// Wait for screenshot generation to complete for a given bundlepath.
    /// Returns true if screenshot was generated successfully (file exists).
    pub async fn wait_for_screenshot(&self, bundlepath: &Path) -> bool {
        let screenshot = bundlepath.join("screenshot.png");
        if screenshot.exists() {
            return true;
        }

        let notify = {
            let state = self.state.read().await;
            state.pending.get(bundlepath).cloned()
        };

        if let Some(notify) = notify {
            notify.notified().await;
            screenshot.exists()
        } else {
            screenshot.exists()
        }
    }
}

/// Background worker that processes screenshot requests sequentially.
async fn screenshot_worker(
    mut rx: mpsc::UnboundedReceiver<ScreenshotRequest>,
    state: Arc<tokio::sync::RwLock<GeneratorState>>,
    html_dir: PathBuf,
) {
    while let Some(req) = rx.recv().await {
        let bp = req.bundlepath;

        // Register as pending (skip if already pending)
        let notify = {
            let mut s = state.write().await;
            if s.pending.contains_key(&bp) {
                continue;
            }
            let notify = Arc::new(Notify::new());
            s.pending.insert(bp.clone(), notify.clone());
            notify
        };

        tracing::info!("[screenshot] generating for {:?}", bp);

        // Run the CPU-intensive work on a blocking thread
        let html_dir_clone = html_dir.clone();
        let bp_clone = bp.clone();
        let result = tokio::task::spawn_blocking(move || {
            generate_screenshot(&bp_clone, &html_dir_clone)
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::info!("[screenshot] generated for {:?}", bp);
            }
            Ok(Err(e)) => {
                tracing::error!("[screenshot] failed for {:?}: {}", bp, e);
            }
            Err(e) => {
                tracing::error!("[screenshot] task panicked for {:?}: {}", bp, e);
            }
        }

        // Remove from pending and notify waiters
        {
            let mut s = state.write().await;
            s.pending.remove(&bp);
        }
        notify.notify_waiters();
    }
}

/// Load a PNG image, returning a transparent 1x1 image on failure.
fn load_png(path: &Path) -> DynamicImage {
    image::open(path).unwrap_or_else(|_| {
        tracing::warn!("[screenshot] failed to load {:?}", path);
        DynamicImage::ImageRgba8(RgbaImage::new(1, 1))
    })
}

/// Overlay `top` onto `canvas` at position (x, y) with correct Porter-Duff alpha compositing.
fn paste(canvas: &mut RgbaImage, top: &DynamicImage, x: i64, y: i64) {
    let top_rgba = top.to_rgba8();
    let (tw, th) = top_rgba.dimensions();
    let (cw, ch) = canvas.dimensions();

    for ty in 0..th {
        let cy = y + ty as i64;
        if cy < 0 || cy >= ch as i64 {
            continue;
        }
        for tx in 0..tw {
            let cx = x + tx as i64;
            if cx < 0 || cx >= cw as i64 {
                continue;
            }
            let src = top_rgba.get_pixel(tx, ty);
            let dst = canvas.get_pixel(cx as u32, cy as u32);
            let src_a = src[3] as f32 / 255.0;
            let dst_a = dst[3] as f32 / 255.0;
            let out_a = src_a + dst_a * (1.0 - src_a);
            let blended = if out_a == 0.0 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([
                    ((src[0] as f32 * src_a + dst[0] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
                    ((src[1] as f32 * src_a + dst[1] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
                    ((src[2] as f32 * src_a + dst[2] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
                    (out_a * 255.0) as u8,
                ])
            };
            canvas.put_pixel(cx as u32, cy as u32, blended);
        }
    }
}

/// Draw an anti-aliased pixel with the given color and opacity.
fn draw_pixel_aa(canvas: &mut RgbaImage, x: i32, y: i32, color: (u8, u8, u8), alpha: f32) {
    let (cw, ch) = canvas.dimensions();
    if x < 0 || y < 0 || x >= cw as i32 || y >= ch as i32 {
        return;
    }
    let dst = canvas.get_pixel(x as u32, y as u32);
    let src_a = alpha;
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a == 0.0 {
        return;
    }
    let blended = Rgba([
        ((color.0 as f32 * src_a + dst[0] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
        ((color.1 as f32 * src_a + dst[1] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
        ((color.2 as f32 * src_a + dst[2] as f32 * dst_a * (1.0 - src_a)) / out_a) as u8,
        (out_a * 255.0) as u8,
    ]);
    canvas.put_pixel(x as u32, y as u32, blended);
}

/// Draw a thick anti-aliased dot at (cx, cy) with the given radius.
fn draw_thick_dot(canvas: &mut RgbaImage, cx: f64, cy: f64, radius: f64, color: (u8, u8, u8)) {
    let r_ceil = radius.ceil() as i32 + 1;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let px = cx + dx as f64;
            let py = cy + dy as f64;
            let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            if dist <= radius - 0.5 {
                draw_pixel_aa(canvas, px as i32, py as i32, color, 1.0);
            } else if dist <= radius + 0.5 {
                let alpha = (radius + 0.5 - dist).clamp(0.0, 1.0) as f32;
                draw_pixel_aa(canvas, px as i32, py as i32, color, alpha);
            }
        }
    }
}

/// Evaluate a cubic bezier curve at parameter t.
fn cubic_bezier(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;
    (
        uuu * p0.0 + 3.0 * uu * t * p1.0 + 3.0 * u * tt * p2.0 + ttt * p3.0,
        uuu * p0.1 + 3.0 * uu * t * p1.1 + 3.0 * u * tt * p2.1 + ttt * p3.1,
    )
}

/// Draw a thick anti-aliased cubic bezier cable on the canvas.
fn draw_cable(
    canvas: &mut RgbaImage,
    src_x: f64, src_y: f64,
    tgt_x: f64, tgt_y: f64,
    color: (u8, u8, u8),
) {
    // Control point calculation matching Python's SVG path logic
    let mut delta_x = tgt_x - src_x - 50.0;
    if delta_x < 0.0 {
        delta_x = 8.5 * (delta_x / 6.0);
    } else {
        delta_x /= 1.5;
    }

    let p0 = (src_x, src_y);
    let p1 = (tgt_x - delta_x, src_y);
    let p2 = (src_x + delta_x, tgt_y);
    let p3 = (tgt_x, tgt_y);

    // Estimate curve length for sampling density
    let chord = ((p3.0 - p0.0).powi(2) + (p3.1 - p0.1).powi(2)).sqrt();
    let steps = (chord * 2.0).max(200.0) as usize;
    let radius = CABLE_WIDTH / 2.0;

    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let (x, y) = cubic_bezier(p0, p1, p2, p3, t);
        draw_thick_dot(canvas, x, y, radius, color);
    }
}

/// Detect port connector positions on a plugin screenshot by scanning for
/// transparent/opaque transitions along the first/last column.
/// Returns pairs of (x, y) transition points. Each port occupies 2 transition points.
fn detect_port_columns(img: &DynamicImage, num_ports: usize, from_right: bool) -> Vec<(i64, i64)> {
    if num_ports == 0 {
        return Vec::new();
    }

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    let scan_range: Box<dyn Iterator<Item = u32>> = if from_right {
        Box::new((0..w).rev())
    } else {
        Box::new(0..w)
    };

    for i in scan_range {
        let mut was_transparent = true;
        let mut transitions = Vec::new();

        for j in 0..h {
            let pixel = rgba.get_pixel(i, j);
            let is_transparent = pixel[3] < 255;
            if was_transparent != is_transparent {
                transitions.push((i as i64, j as i64));
                was_transparent = is_transparent;
            }
        }

        if !transitions.is_empty() {
            // Pad with extra entries if we don't have enough for all ports
            while transitions.len() < num_ports * 2 {
                transitions.insert(0, (transitions[0].0, 100));
            }
            return transitions;
        }
    }

    // Fallback: if detection fails and single port, use a default position
    if num_ports == 1 {
        let x = if from_right { w as i64 - 4 } else { 0 };
        return vec![(x, 100)];
    }
    Vec::new()
}

/// Hardcoded port positions for the default "tuna can" pedal image.
fn default_port_columns(from_right: bool) -> Vec<(i64, i64)> {
    if from_right {
        vec![
            (259, 121), (259, 146), (259, 190), (259, 215), (259, 259), (259, 284),
            (259, 328), (259, 353), (259, 397), (259, 422), (259, 466), (259, 491),
            (259, 535), (259, 560), (259, 604), (259, 629), (259, 673), (259, 698),
            (259, 742), (259, 767), (259, 811), (259, 836), (259, 880), (259, 949),
            (259, 974), (259, 1018), (259, 1043), (259, 1087), (259, 1112),
            (259, 1156), (259, 1181), (259, 1225),
        ]
    } else {
        vec![
            (-9, 121), (-9, 146), (-9, 190), (-9, 215), (-9, 259), (-9, 284),
            (-9, 328), (-9, 353), (-9, 397), (-9, 422), (-9, 466), (-9, 491),
            (-9, 535), (-9, 560), (-9, 604), (-9, 629),
        ]
    }
}

/// Port type for determining cable color and connector offsets.
#[derive(Clone, Copy, PartialEq)]
enum PortType {
    Audio,
    Midi,
    Cv,
}

/// Info about a resolved port connector position on the canvas.
struct PortConnector {
    /// Canvas position where the cable attaches
    cable_x: f64,
    cable_y: f64,
    /// Canvas position to paste the connected connector image
    img_x: i64,
    img_y: i64,
    port_type: PortType,
    connected_img: DynamicImage,
}

/// Info about a device connector (capture/playback) with its resolved position.
struct DeviceConnector {
    symbol: String,
    x: i64,
    y: i64, // center y
    img: DynamicImage,
    connected_img: DynamicImage,
    port_type: PortType,
}

/// Plugin with its screenshot, position, and detected port connector positions.
struct PluginEntry {
    instance: String,
    x: i64,
    y: i64,
    img: DynamicImage,
    /// Input port connectors: (symbol, port_type, position relative to plugin image)
    in_ports: Vec<(String, PortType, (i64, i64))>,
    /// Output port connectors
    out_ports: Vec<(String, PortType, (i64, i64))>,
}

fn generate_screenshot(bundle_path: &Path, html_dir: &Path) -> Result<(), String> {
    let bundle_str = bundle_path.to_string_lossy();
    let pb = lv2_utils::get_pedalboard_info(&bundle_str)
        .ok_or_else(|| "failed to get pedalboard info".to_string())?;

    let img_dir = html_dir.join("img");

    // Preload connector images
    let audio_output_img = load_png(&img_dir.join("audio-output.png"));
    let audio_output_connected = load_png(&img_dir.join("audio-output-connected.png"));
    let audio_input_img = load_png(&img_dir.join("audio-input.png"));
    let audio_input_connected = load_png(&img_dir.join("audio-input-connected.png"));
    let midi_output_img = load_png(&img_dir.join("midi-output.png"));
    let midi_output_connected = load_png(&img_dir.join("midi-output-connected.png"));
    let midi_input_img = load_png(&img_dir.join("midi-input.png"));
    let midi_input_connected = load_png(&img_dir.join("midi-input-connected.png"));
    let default_screenshot = load_png(&html_dir.join("resources").join("pedals").join("default.png"));

    let right_padding = audio_input_connected.width() * 2;
    let bottom_padding = right_padding;

    // Parse pedalboard data
    let plugins = pb["plugins"].as_array().cloned().unwrap_or_default();
    let connections = pb["connections"].as_array().cloned().unwrap_or_default();
    let hw = &pb["hardware"];
    let audio_ins = hw["audio_ins"].as_u64().unwrap_or(2) as usize;
    let audio_outs = hw["audio_outs"].as_u64().unwrap_or(2) as usize;
    let midi_separated = pb.get("midi_separated_mode").and_then(|v| v.as_bool()).unwrap_or(false);

    // Build device capture connectors (left side)
    let mut device_capture: Vec<DeviceConnector> = Vec::new();
    for ix in 0..audio_ins {
        device_capture.push(DeviceConnector {
            symbol: format!("capture_{}", ix + 1),
            x: 0, y: 0,
            img: audio_output_img.clone(),
            connected_img: audio_output_connected.clone(),
            port_type: PortType::Audio,
        });
    }
    if !midi_separated {
        device_capture.push(DeviceConnector {
            symbol: "midi_merger_out".to_string(),
            x: 0, y: 0,
            img: midi_output_img.clone(),
            connected_img: midi_output_connected.clone(),
            port_type: PortType::Midi,
        });
    }

    // Build device playback connectors (right side)
    let mut device_playback: Vec<DeviceConnector> = Vec::new();
    for ix in 0..audio_outs {
        device_playback.push(DeviceConnector {
            symbol: format!("playback_{}", ix + 1),
            x: 0, y: 0,
            img: audio_input_img.clone(),
            connected_img: audio_input_connected.clone(),
            port_type: PortType::Audio,
        });
    }
    if !midi_separated {
        device_playback.push(DeviceConnector {
            symbol: "midi_broadcaster_in".to_string(),
            x: 0, y: 0,
            img: midi_input_img.clone(),
            connected_img: midi_input_connected.clone(),
            port_type: PortType::Midi,
        });
    }

    // Collect used connection symbols
    let used_symbols: Vec<String> = connections
        .iter()
        .flat_map(|c| {
            let src = c["source"].as_str().unwrap_or("");
            let tgt = c["target"].as_str().unwrap_or("");
            vec![src.to_string(), tgt.to_string()]
        })
        .collect();

    // Ensure device connectors exist for all ports referenced in connections
    // (the pedalboard info may not report enough audio_ins/audio_outs)
    for sym in &used_symbols {
        if let Some(num_str) = sym.strip_prefix("capture_") {
            if !device_capture.iter().any(|d| d.symbol == *sym) {
                if num_str.parse::<u32>().is_ok() {
                    device_capture.push(DeviceConnector {
                        symbol: sym.clone(), x: 0, y: 0,
                        img: audio_output_img.clone(),
                        connected_img: audio_output_connected.clone(),
                        port_type: PortType::Audio,
                    });
                }
            }
        } else if let Some(num_str) = sym.strip_prefix("playback_") {
            if !device_playback.iter().any(|d| d.symbol == *sym) {
                if num_str.parse::<u32>().is_ok() {
                    device_playback.push(DeviceConnector {
                        symbol: sym.clone(), x: 0, y: 0,
                        img: audio_input_img.clone(),
                        connected_img: audio_input_connected.clone(),
                        port_type: PortType::Audio,
                    });
                }
            }
        } else if sym.starts_with("midi_capture_") {
            if !device_capture.iter().any(|d| d.symbol == *sym) {
                device_capture.push(DeviceConnector {
                    symbol: sym.clone(), x: 0, y: 0,
                    img: midi_output_img.clone(),
                    connected_img: midi_output_connected.clone(),
                    port_type: PortType::Midi,
                });
            }
        } else if sym.starts_with("midi_playback_") {
            if !device_playback.iter().any(|d| d.symbol == *sym) {
                device_playback.push(DeviceConnector {
                    symbol: sym.clone(), x: 0, y: 0,
                    img: midi_input_img.clone(),
                    connected_img: midi_input_connected.clone(),
                    port_type: PortType::Midi,
                });
            }
        }
    }

    // Sort device connectors by symbol for consistent ordering
    device_capture.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    device_playback.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    // Filter device connectors: always show audio, only show MIDI if connected
    let always_show = ["midi_merger_out", "midi_broadcaster_in"];
    device_capture.retain(|d| {
        d.port_type == PortType::Audio || always_show.contains(&d.symbol.as_str()) || used_symbols.contains(&d.symbol)
    });
    device_playback.retain(|d| {
        d.port_type == PortType::Audio || always_show.contains(&d.symbol.as_str()) || used_symbols.contains(&d.symbol)
    });

    // Load plugin screenshots, detect ports, and build plugin map
    let mut plugin_entries: Vec<PluginEntry> = Vec::new();

    for p in &plugins {
        let uri = p["uri"].as_str().unwrap_or("");
        if uri == "http://drobilla.net/ns/ingen#GraphPrototype" {
            continue;
        }
        let instance = p["instance"].as_str().unwrap_or("").to_string();
        let x = p["x"].as_f64().unwrap_or(0.0) as i64;
        let y = p["y"].as_f64().unwrap_or(0.0) as i64;

        // Try to load plugin screenshot
        let gui = lv2_utils::get_plugin_gui(uri);
        let screenshot_path = gui
            .as_ref()
            .and_then(|g| g["screenshot"].as_str())
            .filter(|s| !s.is_empty() && Path::new(s).is_file())
            .map(|s| s.to_string());

        let pimg = screenshot_path.as_ref()
            .and_then(|s| image::open(s).ok())
            .unwrap_or_else(|| default_screenshot.clone());
        let has_real_screenshot = screenshot_path.is_some();

        // Get plugin info for port counts
        let data = lv2_utils::get_plugin_info(uri);

        let mut in_ports: Vec<(String, PortType, (i64, i64))> = Vec::new();
        let mut out_ports: Vec<(String, PortType, (i64, i64))> = Vec::new();

        if let Some(ref data) = data {
            // Collect input ports in order: audio, cv, midi (matching Python)
            let audio_in = data["ports"]["audio"]["input"].as_array();
            let cv_in = data["ports"]["cv"]["input"].as_array();
            let midi_in = data["ports"]["midi"]["input"].as_array();

            let mut in_port_list: Vec<(String, PortType)> = Vec::new();
            if let Some(ports) = audio_in {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    in_port_list.push((sym, PortType::Audio));
                }
            }
            if let Some(ports) = cv_in {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    in_port_list.push((sym, PortType::Cv));
                }
            }
            if let Some(ports) = midi_in {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    in_port_list.push((sym, PortType::Midi));
                }
            }

            // Collect output ports in order: audio, cv, midi
            let audio_out = data["ports"]["audio"]["output"].as_array();
            let cv_out = data["ports"]["cv"]["output"].as_array();
            let midi_out = data["ports"]["midi"]["output"].as_array();

            let mut out_port_list: Vec<(String, PortType)> = Vec::new();
            if let Some(ports) = audio_out {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    out_port_list.push((sym, PortType::Audio));
                }
            }
            if let Some(ports) = cv_out {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    out_port_list.push((sym, PortType::Cv));
                }
            }
            if let Some(ports) = midi_out {
                for port in ports {
                    let sym = port["symbol"].as_str().unwrap_or("").to_string();
                    out_port_list.push((sym, PortType::Midi));
                }
            }

            // Detect port positions on the plugin image
            let in_columns = if has_real_screenshot {
                detect_port_columns(&pimg, in_port_list.len(), false)
            } else {
                default_port_columns(false)
            };
            let out_columns = if has_real_screenshot {
                detect_port_columns(&pimg, out_port_list.len(), true)
            } else {
                default_port_columns(true)
            };

            // Map ports to their connector positions (using pairs of transitions)
            for (ix, (sym, ptype)) in in_port_list.into_iter().enumerate() {
                let pair_idx = ix * 2;
                if pair_idx < in_columns.len() {
                    let pos = in_columns[pair_idx];
                    in_ports.push((sym, ptype, pos));
                }
            }
            for (ix, (sym, ptype)) in out_port_list.into_iter().enumerate() {
                let pair_idx = ix * 2;
                if pair_idx < out_columns.len() {
                    let pos = out_columns[pair_idx];
                    out_ports.push((sym, ptype, pos));
                }
            }
        }

        plugin_entries.push(PluginEntry {
            instance, x, y, img: pimg, in_ports, out_ports,
        });
    }

    // Calculate canvas size
    let mut width: u32 = pb["width"].as_f64().unwrap_or(0.0).max(0.0) as u32;
    let mut height: u32 = pb["height"].as_f64().unwrap_or(0.0).max(0.0) as u32;

    for p in &plugin_entries {
        let pw = p.x as u32 + p.img.width() + right_padding;
        let ph = p.y as u32 + p.img.height() + bottom_padding;
        if pw > width {
            width = pw;
        }
        if ph > height {
            height = ph;
        }
    }
    if height == 0 {
        height = 1112;
    }
    if width == 0 {
        width = 3840;
    }

    // Calculate device connector positions
    if !device_capture.is_empty() {
        let step = height as i64 / (device_capture.len() as i64 + 1);
        for (i, d) in device_capture.iter_mut().enumerate() {
            d.x = 0;
            d.y = step * (i as i64 + 1);
        }
    }
    if !device_playback.is_empty() {
        let step = height as i64 / (device_playback.len() as i64 + 1);
        for (i, d) in device_playback.iter_mut().enumerate() {
            d.x = width as i64;
            d.y = step * (i as i64 + 1);
        }
    }

    // Build plugin map by instance name for connection lookup
    let plugin_map: HashMap<&str, &PluginEntry> = plugin_entries
        .iter()
        .map(|p| (p.instance.as_str(), p))
        .collect();

    // Connector image offsets (where the cable attaches relative to the connector image)
    // These match the Python code's offset values
    let audio_input_offset: (i64, i64) = (79, 15);
    let audio_output_offset: (i64, i64) = (8, 15);
    let midi_input_offset: (i64, i64) = (67, 9);
    let midi_output_offset: (i64, i64) = (8, 9);
    let cv_input_offset: (i64, i64) = (67, 15);
    let cv_output_offset: (i64, i64) = (11, 22);

    // Process connections: resolve positions, collect cables and port connectors to draw
    struct CableInfo {
        src_x: f64,
        src_y: f64,
        tgt_x: f64,
        tgt_y: f64,
        color: (u8, u8, u8),
    }
    struct ConnectorToDraw {
        img: DynamicImage,
        x: i64,
        y: i64,
    }

    let mut cables: Vec<CableInfo> = Vec::new();
    let mut connectors_to_draw: Vec<ConnectorToDraw> = Vec::new();

    for c in &connections {
        let source_sym = c["source"].as_str().unwrap_or("");
        let target_sym = c["target"].as_str().unwrap_or("");

        // Resolve source position
        let source_info = resolve_connection_endpoint(
            source_sym, true, &device_capture, &plugin_map,
            &audio_output_connected, &midi_output_connected,
            audio_output_offset, midi_output_offset, cv_output_offset,
        );
        let target_info = resolve_connection_endpoint(
            target_sym, false, &device_playback, &plugin_map,
            &audio_input_connected, &midi_input_connected,
            audio_input_offset, midi_input_offset, cv_input_offset,
        );

        if let (Some(src), Some(tgt)) = (source_info, target_info) {
            let color = if src.port_type == PortType::Midi || tgt.port_type == PortType::Midi {
                MIDI_COLOR
            } else if src.port_type == PortType::Cv || tgt.port_type == PortType::Cv {
                CV_COLOR
            } else {
                AUDIO_COLOR
            };

            cables.push(CableInfo {
                src_x: src.cable_x,
                src_y: src.cable_y,
                tgt_x: tgt.cable_x,
                tgt_y: tgt.cable_y,
                color,
            });

            connectors_to_draw.push(ConnectorToDraw {
                img: src.connected_img,
                x: src.img_x,
                y: src.img_y,
            });
            connectors_to_draw.push(ConnectorToDraw {
                img: tgt.connected_img,
                x: tgt.img_x,
                y: tgt.img_y,
            });
        }
    }

    // Create canvas
    let mut canvas = RgbaImage::new(width, height);

    // Draw plugins
    for p in &plugin_entries {
        paste(&mut canvas, &p.img, p.x, p.y);
    }

    // Draw device connectors (unconnected ones)
    for d in &device_capture {
        if !used_symbols.contains(&d.symbol) {
            let cy = d.y - d.img.height() as i64 / 2;
            paste(&mut canvas, &d.img, 0, cy);
        }
    }
    for d in &device_playback {
        if !used_symbols.contains(&d.symbol) {
            let cx = width as i64 - d.img.width() as i64;
            let cy = d.y - d.img.height() as i64 / 2;
            paste(&mut canvas, &d.img, cx, cy);
        }
    }

    // Draw cables
    for cable in &cables {
        draw_cable(&mut canvas, cable.src_x, cable.src_y, cable.tgt_x, cable.tgt_y, cable.color);
    }

    // Draw connected connector images (on top of cables)
    for conn in &connectors_to_draw {
        paste(&mut canvas, &conn.img, conn.x, conn.y);
    }

    // Save screenshot
    let screenshot_path = bundle_path.join("screenshot.png");
    let dyn_img = DynamicImage::ImageRgba8(canvas);
    dyn_img
        .save(&screenshot_path)
        .map_err(|e| format!("failed to save screenshot: {}", e))?;

    // Generate and save thumbnail
    let thumb = dyn_img.resize(
        MAX_THUMB_WIDTH,
        MAX_THUMB_HEIGHT,
        image::imageops::FilterType::Lanczos3,
    );
    let thumbnail_path = bundle_path.join("thumbnail.png");
    thumb
        .save(&thumbnail_path)
        .map_err(|e| format!("failed to save thumbnail: {}", e))?;

    Ok(())
}

/// Resolve a connection endpoint (source or target) to canvas coordinates.
/// For device connectors (capture/playback), uses the device position.
/// For plugin ports, uses the plugin position + detected port offset.
fn resolve_connection_endpoint(
    symbol: &str,
    is_source: bool,
    device_connectors: &[DeviceConnector],
    plugin_map: &HashMap<&str, &PluginEntry>,
    audio_connected_img: &DynamicImage,
    midi_connected_img: &DynamicImage,
    audio_offset: (i64, i64),
    midi_offset: (i64, i64),
    cv_offset: (i64, i64),
) -> Option<PortConnector> {
    // Check device connectors first
    if let Some(dev) = device_connectors.iter().find(|d| d.symbol == symbol) {
        let conn_img = &dev.connected_img;
        let (iw, ih) = (conn_img.width() as i64, conn_img.height() as i64);

        if is_source {
            // Left side: anchor left-center
            let img_y = dev.y - ih / 2;
            return Some(PortConnector {
                cable_x: (dev.x + iw) as f64,
                cable_y: dev.y as f64,
                img_x: dev.x,
                img_y,
                port_type: dev.port_type,
                connected_img: conn_img.clone(),
            });
        } else {
            // Right side: anchor right-center
            let img_x = dev.x - iw;
            let img_y = dev.y - ih / 2;
            return Some(PortConnector {
                cable_x: (dev.x - iw) as f64,
                cable_y: dev.y as f64,
                img_x,
                img_y,
                port_type: dev.port_type,
                connected_img: conn_img.clone(),
            });
        }
    }

    // Plugin port: split "instance/port_symbol"
    let (instance, port_sym) = symbol.split_once('/')?;
    let plugin = plugin_map.get(instance)?;

    let (ports, offset_fn): (&Vec<(String, PortType, (i64, i64))>, Box<dyn Fn(PortType) -> (i64, i64)>) = if is_source {
        (&plugin.out_ports, Box::new(move |pt| match pt {
            PortType::Audio => audio_offset,
            PortType::Midi => midi_offset,
            PortType::Cv => cv_offset,
        }))
    } else {
        (&plugin.in_ports, Box::new(move |pt| match pt {
            PortType::Audio => audio_offset,
            PortType::Midi => midi_offset,
            PortType::Cv => cv_offset,
        }))
    };

    let (_, port_type, conn_pos) = ports.iter().find(|(sym, _, _)| sym == port_sym)?;

    let offset = offset_fn(*port_type);
    let img_x = plugin.x + conn_pos.0 - offset.0;
    let img_y = plugin.y + conn_pos.1 - offset.1;

    let connected_img = match port_type {
        PortType::Audio => audio_connected_img.clone(),
        PortType::Midi => midi_connected_img.clone(),
        PortType::Cv => audio_connected_img.clone(), // TODO: cv connected img
    };

    let (ciw, cih) = (connected_img.width() as i64, connected_img.height() as i64);

    if is_source {
        Some(PortConnector {
            cable_x: (img_x + ciw) as f64,
            cable_y: (img_y + cih / 2) as f64,
            img_x,
            img_y,
            port_type: *port_type,
            connected_img,
        })
    } else {
        Some(PortConnector {
            cable_x: img_x as f64,
            cable_y: (img_y + cih / 2) as f64,
            img_x,
            img_y,
            port_type: *port_type,
            connected_img,
        })
    }
}
