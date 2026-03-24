// HMI (Hardware Menu Interface) - TCP communication
// Ported from mod/hmi.py (TcpHMI class only, serial support removed per user request)

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::mod_protocol::*;
use crate::protocol::{self, ParsedMessage, Protocol, RespValue};
use crate::utils;

/// Decode a percent-encoded string (e.g. "Hello%20World" → "Hello World").
fn percent_decode_str(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(0);
            let lo = chars.next().unwrap_or(0);
            let hex = [hi, lo];
            if let Ok(decoded) = u8::from_str_radix(std::str::from_utf8(&hex).unwrap_or(""), 16) {
                result.push(decoded as char);
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Commands received from the HMI that need session-level handling.
#[derive(Debug)]
pub enum HmiCommand {
    /// Pedalboard load request: (bank_id, pedalboard_index)
    PedalboardLoad(i32, String),
    /// Pedalboard save request
    PedalboardSave,
    /// Menu item change: (menu_id, value)
    MenuItemChange(i32, f64),
    /// File parameter set: (instance, param_uri, path)
    FileParamSet(String, String, String),
    /// Tuner on
    TunerOn,
    /// Tuner off
    TunerOff,
    /// Tuner input port change
    TunerInput(i32),
    /// Tuner reference frequency change
    TunerRefFreq(i32),
    /// Request snapshot list
    SnapshotList,
    /// Load snapshot by index
    SnapshotLoad(i32),
    /// Save current snapshot (overwrite at index)
    SnapshotSave,
    /// Save as new snapshot with name
    SnapshotSaveAs(String),
    /// Delete snapshot by index
    SnapshotDelete(i32),
    /// Rename snapshot: (index, new_name)
    SnapshotRename(i32, String),
    /// Request profile list
    ProfileList,
    /// Load profile by index (1-based)
    ProfileLoad(i32),
    /// Store current settings to profile index (1-based)
    ProfileStore(i32),
}

/// Trait for HMI implementations (real TCP or fake).
pub trait Hmi: Send + Sync {
    fn is_fake(&self) -> bool;
    fn send(&self, msg: &str, callback: Option<HmiCallback>, datatype: &str);
    fn ping(&self, callback: HmiCallback);
    fn clear(&self, callback: HmiCallback);
    fn ui_con(&self, callback: HmiCallback);
    fn ui_dis(&self, callback: HmiCallback);
    fn control_set(&self, hw_id: i32, value: f64, callback: HmiCallback);
    fn control_rm(&self, hw_ids: &[i32], callback: HmiCallback);
    fn set_pedalboard_name(&self, name: &str, callback: HmiCallback);
    fn set_snapshot_name(&self, index: i32, name: &str, callback: HmiCallback);
    fn set_profile_value(&self, key: i32, value: f64, callback: HmiCallback);
    fn set_pedalboard_index(&self, index: i32, callback: HmiCallback);
    fn set_bank_index(&self, index: i32, callback: HmiCallback);
    fn set_available_pages(&self, pages: &[i32], callback: HmiCallback);
    fn restore(&self, callback: HmiCallback);
    fn tuner(&self, freq: f64, note: &str, cents: i32, callback: HmiCallback);
    fn set_tuner_input(&self, port: i32, callback: HmiCallback);
    fn set_tuner_ref_freq(&self, freq: i32, callback: HmiCallback);
    fn screenshot(&self, screen: i32, callback: HmiCallback);
    fn initialized(&self) -> bool;
    fn set_initialized(&self, val: bool);
}

/// Callback type for HMI responses.
pub type HmiCallback = Box<dyn FnOnce(RespValue) + Send + 'static>;

/// A queued HMI message awaiting response.
struct QueuedMessage {
    msg: String,
    callback: Option<HmiCallback>,
    datatype: String,
}

/// TCP-based HMI connection. Ported from TcpHMI in mod/hmi.py.
pub struct TcpHmi {
    inner: Arc<Mutex<TcpHmiInner>>,
}

struct TcpHmiInner {
    host: String,
    port: u16,
    timeout_secs: u64,
    queue: VecDeque<QueuedMessage>,
    queue_idle: bool,
    is_initialized: bool,
    connected: bool,
    handling_response: bool,
    last_write_time: Option<Instant>,
    need_flush: usize,
    stream: Option<TcpStream>,
    hw_desc: serde_json::Map<String, Value>,
    hw_ids: Vec<i64>,
    bpm: Option<i64>,
    protocol: Protocol,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HmiCommand>,
}

impl TcpHmi {
    pub fn new(
        host: &str,
        port: u16,
        timeout_secs: u64,
        hw_desc_file: &std::path::Path,
    ) -> (Self, tokio::sync::mpsc::UnboundedReceiver<HmiCommand>) {
        let hw_desc = utils::get_hardware_descriptor(hw_desc_file);
        let hw_ids = hw_desc
            .get("actuators")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("id").and_then(|v| v.as_i64()))
                    .collect()
            })
            .unwrap_or_default();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let hmi = TcpHmi {
            inner: Arc::new(Mutex::new(TcpHmiInner {
                host: host.to_string(),
                port,
                timeout_secs,
                queue: VecDeque::new(),
                queue_idle: true,
                is_initialized: false,
                connected: false,
                handling_response: false,
                last_write_time: None,
                need_flush: 0,
                stream: None,
                hw_desc,
                hw_ids,
                bpm: None,
                protocol: Protocol::new(),
                cmd_tx,
            })),
        };

        (hmi, cmd_rx)
    }

    /// Start the TCP connection and begin reading messages.
    /// This spawns a background task for the read loop.
    pub fn connect(&self) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            Self::connect_loop(inner).await;
        });
    }

    async fn connect_loop(inner: Arc<Mutex<TcpHmiInner>>) {
        let mut was_connected = false;

        loop {
            let (host, port) = {
                let guard = inner.lock().await;
                (guard.host.clone(), guard.port)
            };

            if !was_connected {
                tracing::debug!("[hmi] connecting to {}:{}...", host, port);
            }

            match TcpStream::connect((host.as_str(), port)).await {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    tracing::info!("[hmi] TCP connected to {}:{}", host, port);
                    was_connected = true;

                    {
                        let mut guard = inner.lock().await;
                        guard.stream = Some(stream);
                        guard.connected = true;
                        guard.queue.clear();
                        guard.queue_idle = true;
                    }

                    // Start read loop (blocks until disconnect)
                    Self::read_loop(inner.clone()).await;

                    // Disconnected — clear state and retry
                    {
                        let mut guard = inner.lock().await;
                        guard.stream = None;
                        guard.connected = false;
                        guard.is_initialized = false;
                    }
                    tracing::warn!("[hmi] TCP connection lost, reconnecting...");
                }
                Err(e) => {
                    if was_connected {
                        tracing::warn!("[hmi] TCP connection to {}:{} failed: {}", host, port, e);
                        was_connected = false;
                    } else {
                        tracing::debug!("[hmi] TCP connection to {}:{} failed: {}", host, port, e);
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    /// Read null-terminated messages from the TCP stream.
    async fn read_loop(inner: Arc<Mutex<TcpHmiInner>>) {
        let mut buf = vec![0u8; 4096];
        let mut msg_buf = Vec::new();

        loop {
            // Take the stream out briefly for reading
            let read_result = {
                let mut guard = inner.lock().await;
                if let Some(ref mut stream) = guard.stream {
                    match stream.try_read(&mut buf) {
                        Ok(0) => Err("connection closed"),
                        Ok(n) => Ok(buf[..n].to_vec()),
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(vec![]),
                        Err(_) => Err("read error"),
                    }
                } else {
                    Err("no stream")
                }
            };

            match read_result {
                Ok(data) if data.is_empty() => {
                    // No data ready, yield briefly
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    continue;
                }
                Ok(data) => {
                    msg_buf.extend_from_slice(&data);

                    // Process complete null-terminated messages
                    while let Some(pos) = msg_buf.iter().position(|&b| b == 0) {
                        let msg_bytes = msg_buf.drain(..=pos).collect::<Vec<_>>();
                        let msg = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);
                        let msg = msg.trim().to_string();

                        if !msg.is_empty() {
                            Self::handle_message(inner.clone(), &msg).await;
                        }
                    }
                }
                Err(_) => {
                    return; // Stream closed or error, exit read loop
                }
            }
        }
    }

    /// Handle a received message (response or command from HMI).
    async fn handle_message(inner: Arc<Mutex<TcpHmiInner>>, msg: &str) {
        let mut guard = inner.lock().await;
        guard.last_write_time = None;
        guard.need_flush = 0;

        if Protocol::is_resp(msg) {
            // It's a response to a queued command
            if let Some(queued) = guard.queue.pop_front() {
                tracing::debug!("[hmi] response for '{}': {}", queued.msg, msg);
                if let Some(callback) = queued.callback {
                    let parsed = ParsedMessage {
                        msg: msg.to_string(),
                        cmd: String::new(),
                        args: Vec::new(),
                        is_response: true,
                    };
                    let resp = parsed.process_resp(&queued.datatype);
                    // Release lock before calling callback
                    drop(guard);
                    callback(resp);
                    // Re-acquire to process queue
                    let mut guard = inner.lock().await;
                    Self::process_queue_inner(&mut guard).await;
                } else {
                    Self::process_queue_inner(&mut guard).await;
                }
            } else {
                tracing::warn!("[hmi] received response but queue is empty: {}", msg);
            }
        } else {
            // It's a command from HMI — parse directly
            tracing::debug!("[hmi] received command: {}", msg);
            let parts: Vec<&str> = msg.splitn(2, ' ').collect();
            let cmd = parts[0];
            let args_str = parts.get(1).copied().unwrap_or("");

            match cmd {
                CMD_PEDALBOARD_LOAD => {
                    let args: Vec<&str> = args_str.splitn(2, ' ').collect();
                    if args.len() >= 2 {
                        if let Ok(bank_id) = args[0].parse::<i32>() {
                            let _ = guard.cmd_tx.send(HmiCommand::PedalboardLoad(
                                bank_id,
                                args[1].to_string(),
                            ));
                        }
                    }
                }
                CMD_PEDALBOARD_SAVE => {
                    let _ = guard.cmd_tx.send(HmiCommand::PedalboardSave);
                }
                CMD_MENU_ITEM_CHANGE => {
                    let args: Vec<&str> = args_str.splitn(2, ' ').collect();
                    if args.len() >= 2 {
                        if let (Ok(menu_id), Ok(value)) = (args[0].parse::<i32>(), args[1].parse::<f64>()) {
                            let _ = guard.cmd_tx.send(HmiCommand::MenuItemChange(menu_id, value));
                        }
                    }
                }
                CMD_FILE_PARAM_SET => {
                    // fps <instance> <param_uri> <path>
                    let args: Vec<&str> = args_str.splitn(3, ' ').collect();
                    if args.len() >= 3 {
                        let _ = guard.cmd_tx.send(HmiCommand::FileParamSet(
                            args[0].to_string(),
                            args[1].to_string(),
                            args[2].to_string(),
                        ));
                    }
                }
                CMD_TUNER_ON => {
                    let _ = guard.cmd_tx.send(HmiCommand::TunerOn);
                }
                CMD_TUNER_OFF => {
                    let _ = guard.cmd_tx.send(HmiCommand::TunerOff);
                }
                CMD_TUNER_INPUT => {
                    if let Ok(port) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::TunerInput(port));
                    }
                }
                CMD_TUNER_REF_FREQ => {
                    if let Ok(freq) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::TunerRefFreq(freq));
                    }
                }
                CMD_SNAPSHOTS => {
                    if args_str.is_empty() {
                        // Request for snapshot list (no args = HMI asking for list)
                        let _ = guard.cmd_tx.send(HmiCommand::SnapshotList);
                    }
                    // If it has args, it's a response from HMI (shouldn't happen in this direction)
                }
                CMD_SNAPSHOTS_LOAD => {
                    if let Ok(index) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::SnapshotLoad(index));
                    }
                }
                CMD_SNAPSHOTS_SAVE => {
                    let _ = guard.cmd_tx.send(HmiCommand::SnapshotSave);
                }
                CMD_SNAPSHOT_SAVE_AS => {
                    let name = percent_decode_str(args_str.trim());
                    if !name.is_empty() {
                        let _ = guard.cmd_tx.send(HmiCommand::SnapshotSaveAs(name));
                    }
                }
                CMD_SNAPSHOT_DELETE => {
                    if let Ok(index) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::SnapshotDelete(index));
                    }
                }
                CMD_SNAPSHOT_NAME_SET => {
                    let args: Vec<&str> = args_str.splitn(2, ' ').collect();
                    if args.len() >= 2 {
                        if let Ok(index) = args[0].parse::<i32>() {
                            let name = percent_decode_str(args[1]);
                            let _ = guard.cmd_tx.send(HmiCommand::SnapshotRename(index, name));
                        }
                    }
                }
                CMD_PROFILE_LOAD => {
                    if args_str.trim().is_empty() {
                        // No args = request profile list
                        let _ = guard.cmd_tx.send(HmiCommand::ProfileList);
                    } else if let Ok(index) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::ProfileLoad(index));
                    }
                }
                CMD_PROFILE_STORE => {
                    if let Ok(index) = args_str.trim().parse::<i32>() {
                        let _ = guard.cmd_tx.send(HmiCommand::ProfileStore(index));
                    }
                }
                _ => {
                    tracing::debug!("[hmi] unhandled command from HMI: {}", msg);
                }
            }

            guard.handling_response = true;
            let resp_msg = format!("{} 0", CMD_RESPONSE);
            if let Some(ref mut stream) = guard.stream {
                let data = format!("{}\0", resp_msg);
                let _ = stream.write_all(data.as_bytes()).await;
            }
            guard.handling_response = false;
            if guard.queue_idle {
                Self::process_queue_inner(&mut guard).await;
            }
        }
    }

    /// Send the next queued message over the wire.
    async fn process_queue_inner(inner: &mut TcpHmiInner) {
        if inner.stream.is_none() {
            return;
        }

        if let Some(front) = inner.queue.front() {
            tracing::debug!("[hmi] sending: {}", front.msg);
            let data = format!("{}\0", front.msg);
            if let Some(ref mut stream) = inner.stream {
                match stream.write_all(data.as_bytes()).await {
                    Ok(_) => {
                        inner.queue_idle = false;
                        inner.last_write_time = Some(Instant::now());
                    }
                    Err(e) => {
                        tracing::error!("[hmi] write error: {}", e);
                        inner.stream = None;
                    }
                }
            }
        } else {
            inner.queue_idle = true;
            inner.last_write_time = None;
        }
    }

    /// Queue a message for sending.
    async fn queue_send(&self, msg: &str, callback: Option<HmiCallback>, datatype: &str) {
        let mut guard = self.inner.lock().await;

        if guard.stream.is_none() {
            // Not connected, call callback with failure
            if let Some(cb) = callback {
                cb(protocol::process_resp(None, datatype));
            }
            return;
        }

        // Check for timeout
        if guard.timeout_secs > 0 {
            if guard.queue.len() > 30 {
                guard.need_flush = guard.queue.len();
            } else if let Some(lwt) = guard.last_write_time {
                if lwt.elapsed().as_secs() > guard.timeout_secs {
                    tracing::warn!(
                        "[hmi] no response for {}s, flushing",
                        guard.timeout_secs
                    );
                    Self::flush_inner(&mut guard);
                    return;
                }
            }
        }

        // Check if this is a response message (shouldn't be queued)
        if Protocol::is_resp(msg) {
            if let Some(ref mut stream) = guard.stream {
                let data = format!("{}\0", msg);
                let _ = stream.write_all(data.as_bytes()).await;
            }
            return;
        }

        let was_idle = guard.queue_idle && !guard.handling_response;
        guard.queue.push_back(QueuedMessage {
            msg: msg.to_string(),
            callback,
            datatype: datatype.to_string(),
        });

        if was_idle {
            Self::process_queue_inner(&mut guard).await;
        }
    }

    /// Flush the queue, calling callbacks with error values.
    fn flush_inner(inner: &mut TcpHmiInner) {
        tracing::warn!(
            "[hmi] flushing queue: {} messages",
            inner.queue.len()
        );

        // Drain all but the last message, calling callbacks with failures
        while inner.queue.len() > 1 {
            if let Some(queued) = inner.queue.pop_front() {
                if let Some(cb) = queued.callback {
                    if Protocol::is_resp(&queued.msg) {
                        cb(protocol::process_resp(None, &queued.datatype));
                    } else {
                        cb(RespValue::Raw("-1003".to_string()));
                    }
                }
            }
        }

        // Close the stream to force reconnection
        inner.stream = None;
        inner.connected = false;
    }
}

impl Hmi for TcpHmi {
    fn is_fake(&self) -> bool {
        false
    }

    fn send(&self, msg: &str, callback: Option<HmiCallback>, datatype: &str) {
        let inner = self.inner.clone();
        let msg = msg.to_string();
        let datatype = datatype.to_string();
        tokio::spawn(async move {
            let hmi = TcpHmi { inner };
            hmi.queue_send(&msg, callback, &datatype).await;
        });
    }

    fn ping(&self, callback: HmiCallback) {
        self.send(CMD_PING, Some(callback), "boolean");
    }

    fn clear(&self, callback: HmiCallback) {
        self.send(CMD_PEDALBOARD_CLEAR, Some(callback), "int");
    }

    fn ui_con(&self, callback: HmiCallback) {
        self.send(CMD_GUI_CONNECTED, Some(callback), "boolean");
    }

    fn ui_dis(&self, callback: HmiCallback) {
        self.send(CMD_GUI_DISCONNECTED, Some(callback), "boolean");
    }

    fn control_set(&self, hw_id: i32, value: f64, callback: HmiCallback) {
        self.send(
            &format!("{} {} {}", CMD_CONTROL_SET, hw_id, value),
            Some(callback),
            "boolean",
        );
    }

    fn control_rm(&self, hw_ids: &[i32], callback: HmiCallback) {
        let ids: Vec<String> = hw_ids.iter().map(|id| id.to_string()).collect();
        self.send(
            &format!("{} {}", CMD_CONTROL_REMOVE, ids.join(" ")),
            Some(callback),
            "boolean",
        );
    }

    fn set_pedalboard_name(&self, name: &str, callback: HmiCallback) {
        let hw_name = utils::normalize_for_hw(name, 31);
        self.send(
            &format!("{} {}", CMD_PEDALBOARD_NAME_SET, hw_name),
            Some(callback),
            "int",
        );
    }

    fn set_snapshot_name(&self, index: i32, name: &str, callback: HmiCallback) {
        let hw_name = utils::normalize_for_hw(name, 31);
        self.send(
            &format!("{} {} {}", CMD_SNAPSHOT_NAME_SET, index, hw_name),
            Some(callback),
            "int",
        );
    }

    fn set_profile_value(&self, key: i32, value: f64, callback: HmiCallback) {
        // Special handling for BPM: don't send if rounded value hasn't changed
        let inner = self.inner.clone();
        let msg_key = key;
        let msg_value = value;
        tokio::spawn(async move {
            let mut guard = inner.lock().await;
            if msg_key == MENU_ID_TEMPO {
                let rounded = msg_value.round() as i64;
                if guard.bpm == Some(rounded) {
                    drop(guard);
                    callback(RespValue::Bool(true));
                    return;
                }
                guard.bpm = Some(rounded);
                let msg = format!("{} {} {}", CMD_MENU_ITEM_CHANGE, msg_key, rounded);
                drop(guard);
                let hmi = TcpHmi {
                    inner: inner.clone(),
                };
                // Re-clone inner for the new TcpHmi
                hmi.queue_send(&msg, Some(callback), "boolean").await;
            } else {
                let msg = format!(
                    "{} {} {}",
                    CMD_MENU_ITEM_CHANGE,
                    msg_key,
                    msg_value as i64
                );
                drop(guard);
                let hmi = TcpHmi {
                    inner: inner.clone(),
                };
                hmi.queue_send(&msg, Some(callback), "boolean").await;
            }
        });
    }

    fn set_pedalboard_index(&self, index: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {}", CMD_PEDALBOARD_CHANGE, index),
            Some(callback),
            "int",
        );
    }

    fn set_bank_index(&self, index: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {}", CMD_BANK_CHANGE, index),
            Some(callback),
            "int",
        );
    }

    fn set_available_pages(&self, pages: &[i32], callback: HmiCallback) {
        let page_str: Vec<String> = pages.iter().map(|p| p.to_string()).collect();
        self.send(
            &format!("{} {}", CMD_DUOX_PAGES_AVAILABLE, page_str.join(" ")),
            Some(callback),
            "boolean",
        );
    }

    fn restore(&self, callback: HmiCallback) {
        self.send(CMD_RESTORE, Some(callback), "int");
    }

    fn tuner(&self, freq: f64, note: &str, cents: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {} {} {}", CMD_TUNER, freq, note, cents),
            Some(callback),
            "int",
        );
    }

    fn set_tuner_input(&self, port: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {}", CMD_TUNER_INPUT, port),
            Some(callback),
            "int",
        );
    }

    fn set_tuner_ref_freq(&self, freq: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {}", CMD_TUNER_REF_FREQ, freq),
            Some(callback),
            "int",
        );
    }

    fn screenshot(&self, screen: i32, callback: HmiCallback) {
        self.send(
            &format!("{} {} ignored", CMD_SCREENSHOT, screen),
            Some(callback),
            "int",
        );
    }

    fn initialized(&self) -> bool {
        // Can't await in a non-async fn, so use try_lock
        self.inner
            .try_lock()
            .map(|g| g.is_initialized)
            .unwrap_or(false)
    }

    fn set_initialized(&self, val: bool) {
        if let Ok(mut guard) = self.inner.try_lock() {
            guard.is_initialized = val;
        }
    }
}
