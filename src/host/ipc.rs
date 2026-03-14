// IPC with mod-host audio engine over TCP sockets.
// Ported from the socket/queue parts of mod/host.py
//
// Two connections:
//   - write socket (port N): send commands, receive responses
//   - read socket (port N+1): receive async notifications from mod-host

use std::collections::VecDeque;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::protocol::{self, RespValue};

pub type HostCallback = Box<dyn FnOnce(RespValue) + Send + Sync + 'static>;

struct QueuedMessage {
    msg: String,
    callback: Option<HostCallback>,
    datatype: String,
}

/// Manages the two TCP connections to mod-host and the command queue.
pub struct HostIpc {
    host: String,
    port: u16,
    write_stream: Option<TcpStream>,
    queue: VecDeque<QueuedMessage>,
    idle: bool,
    pub connected: bool,
    pub crashed: bool,
}

impl HostIpc {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            write_stream: None,
            queue: VecDeque::new(),
            idle: true,
            connected: false,
            crashed: false,
        }
    }

    /// Open both connections to mod-host.
    /// Returns the read stream (for spawning the read loop separately).
    pub async fn connect(&mut self) -> Result<TcpStream, String> {
        let write_addr = format!("{}:{}", self.host, self.port);
        let read_addr = format!("{}:{}", self.host, self.port + 1);

        let write_stream = TcpStream::connect(&write_addr)
            .await
            .map_err(|e| format!("write socket connect failed: {}", e))?;
        write_stream.set_nodelay(true).ok();

        let read_stream = TcpStream::connect(&read_addr)
            .await
            .map_err(|e| format!("read socket connect failed: {}", e))?;
        read_stream.set_nodelay(true).ok();

        self.write_stream = Some(write_stream);
        self.connected = true;
        self.crashed = false;
        self.idle = true;
        self.queue.clear();

        tracing::debug!("[host-ipc] connected to mod-host at {}", write_addr);
        Ok(read_stream)
    }

    /// Disconnect from mod-host.
    pub fn disconnect(&mut self) {
        self.write_stream = None;
        self.connected = false;

        // Drain queue, calling callbacks with failure
        while let Some(queued) = self.queue.pop_front() {
            if let Some(cb) = queued.callback {
                cb(protocol::process_resp(None, &queued.datatype));
            }
        }
        self.idle = true;
    }

    /// Send a command and wait for a boolean response.
    pub async fn send_and_wait_bool(&mut self, msg: &str) -> bool {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let callback: HostCallback = Box::new(move |resp| {
            let val = matches!(resp, RespValue::Bool(true));
            let _ = tx.send(val);
        });
        self.send_notmodified(msg, Some(callback), "boolean").await;
        rx.await.unwrap_or(false)
    }

    /// Send a command and wait for a string response.
    pub async fn send_and_wait_str(&mut self, msg: &str) -> Option<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let callback: HostCallback = Box::new(move |resp| {
            let val = match resp {
                RespValue::Str(s) if !s.is_empty() => Some(s),
                _ => None,
            };
            let _ = tx.send(val);
        });
        self.send_notmodified(msg, Some(callback), "string").await;
        rx.await.unwrap_or(None)
    }

    /// Queue a command and send it if idle. Marks pedalboard as modified.
    pub async fn send_modified(
        &mut self,
        msg: &str,
        callback: Option<HostCallback>,
        datatype: &str,
    ) -> bool {
        if self.crashed {
            if let Some(cb) = callback {
                cb(protocol::process_resp(None, datatype));
            }
            return false;
        }

        self.queue.push_back(QueuedMessage {
            msg: msg.to_string(),
            callback,
            datatype: datatype.to_string(),
        });

        if self.idle {
            self.process_write_queue().await;
        }
        true
    }

    /// Queue a command and send it if idle. Does NOT mark pedalboard as modified.
    pub async fn send_notmodified(
        &mut self,
        msg: &str,
        callback: Option<HostCallback>,
        datatype: &str,
    ) {
        if self.crashed {
            if let Some(cb) = callback {
                cb(protocol::process_resp(None, datatype));
            }
            return;
        }

        self.queue.push_back(QueuedMessage {
            msg: msg.to_string(),
            callback,
            datatype: datatype.to_string(),
        });

        if self.idle {
            self.process_write_queue().await;
        }
    }

    /// Process the next message in the write queue.
    async fn process_write_queue(&mut self) {
        let queued = match self.queue.pop_front() {
            Some(q) => q,
            None => {
                self.idle = true;
                return;
            }
        };

        let stream = match self.write_stream.as_mut() {
            Some(s) => s,
            None => {
                // No connection, skip
                if let Some(cb) = queued.callback {
                    cb(protocol::process_resp(None, &queued.datatype));
                }
                self.process_write_queue_boxed().await;
                return;
            }
        };

        self.idle = false;
        tracing::debug!("[host-ipc] sending -> {}", queued.msg);

        let send_data = format!("{}\0", queued.msg);
        if let Err(e) = stream.write_all(send_data.as_bytes()).await {
            tracing::error!("[host-ipc] write error: {}", e);
            if let Some(cb) = queued.callback {
                cb(protocol::process_resp(None, &queued.datatype));
            }
            self.handle_write_error();
            return;
        }

        // Read response (null-terminated)
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte).await {
                Ok(0) => {
                    tracing::error!("[host-ipc] connection closed while reading response");
                    if let Some(cb) = queued.callback {
                        cb(protocol::process_resp(None, &queued.datatype));
                    }
                    self.handle_write_error();
                    return;
                }
                Ok(_) => {
                    if byte[0] == 0 {
                        break;
                    }
                    buf.push(byte[0]);
                }
                Err(e) => {
                    tracing::error!("[host-ipc] read error: {}", e);
                    if let Some(cb) = queued.callback {
                        cb(protocol::process_resp(None, &queued.datatype));
                    }
                    self.handle_write_error();
                    return;
                }
            }
        }

        let resp = String::from_utf8_lossy(&buf).to_string();
        tracing::debug!("[host-ipc] received <- {}", resp);

        if let Some(cb) = queued.callback {
            if queued.datatype == "string" {
                cb(RespValue::Str(resp));
            } else if !resp.starts_with("resp") {
                tracing::error!("[host-ipc] protocol error: {} (for msg: '{}')", resp, queued.msg);
                cb(protocol::process_resp(None, &queued.datatype));
            } else {
                let r = resp
                    .strip_prefix("resp ")
                    .unwrap_or(&resp)
                    .trim()
                    .to_string();
                cb(protocol::process_resp(Some(&r), &queued.datatype));
            }
        }

        // Process next in queue
        self.process_write_queue_boxed().await;
    }

    // Needed because async recursion requires boxing
    fn process_write_queue_boxed(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
        Box::pin(self.process_write_queue())
    }

    fn handle_write_error(&mut self) {
        self.write_stream = None;
        self.crashed = true;
        self.connected = false;

        // Drain remaining queue
        while let Some(queued) = self.queue.pop_front() {
            if let Some(cb) = queued.callback {
                cb(protocol::process_resp(None, &queued.datatype));
            }
        }
        self.idle = true;
    }

    pub fn is_connected(&self) -> bool {
        self.connected && !self.crashed
    }
}
