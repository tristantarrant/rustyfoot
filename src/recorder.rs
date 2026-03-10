// Audio recording/playback, ported from mod/recorder.py
// Wraps jack_capture and sndfile-jackplay subprocesses.

use std::process::{Child, Command};
use std::sync::Arc;

use tokio::sync::Notify;

use crate::settings::Settings;

pub struct Recorder {
    recording: bool,
    proc: Option<Child>,
    capture_path: std::path::PathBuf,
    is_device: bool,
}

impl Recorder {
    pub fn new(settings: &Settings) -> Self {
        Self {
            recording: false,
            proc: None,
            capture_path: settings.capture_path.clone(),
            is_device: settings.device_key.is_some(),
        }
    }

    pub fn start(&mut self) {
        if self.recording {
            return;
        }

        let mut cmd = Command::new("jack_capture");
        cmd.arg("-f").arg("ogg");
        cmd.arg("-b").arg("16");
        cmd.arg("-B").arg("65536");
        let ports = crate::lv2_utils::get_jack_hardware_ports(true, true);
        let num_channels = ports.len().max(2);
        cmd.arg("-c").arg(num_channels.to_string());

        if self.is_device {
            for i in 1..=num_channels {
                cmd.arg("-p").arg(format!("mod-monitor:out_{}", i));
            }
        } else {
            for port in &ports {
                cmd.arg("-p").arg(port);
            }
        }

        cmd.arg(&self.capture_path);

        match cmd.spawn() {
            Ok(child) => {
                self.proc = Some(child);
                self.recording = true;
                tracing::info!("[recorder] started recording");
            }
            Err(e) => {
                tracing::error!("[recorder] failed to start: {}", e);
            }
        }
    }

    pub fn stop(&mut self) -> bool {
        if !self.recording {
            return false;
        }
        self.recording = false;

        if let Some(ref mut child) = self.proc {
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(child.id() as i32, libc::SIGINT);
                }
            }
            let _ = child.wait();
        }
        self.proc = None;
        true
    }

    pub fn delete(&mut self) {
        self.stop();
        let _ = std::fs::remove_file(&self.capture_path);
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }
}

pub struct Player {
    playing: bool,
    playback_path: std::path::PathBuf,
    /// Notified when playback finishes (subprocess exits).
    finished: Arc<Notify>,
}

impl Player {
    pub fn new(settings: &Settings) -> Self {
        Self {
            playing: false,
            playback_path: settings.playback_path.clone(),
            finished: Arc::new(Notify::new()),
        }
    }

    pub fn play(&mut self) -> bool {
        if self.playing {
            return false;
        }
        if !self.playback_path.exists() {
            return false;
        }

        let path = self.playback_path.clone();
        let finished = self.finished.clone();

        // Spawn async task that runs the subprocess and notifies on completion
        actix_web::rt::spawn(async move {
            let result = tokio::process::Command::new("sndfile-jackplay")
                .arg(path.as_os_str())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;

            match result {
                Ok(status) => {
                    tracing::debug!("[player] finished with status: {}", status);
                }
                Err(e) => {
                    tracing::error!("[player] error: {}", e);
                }
            }
            finished.notify_waiters();
        });

        self.playing = true;
        true
    }

    pub fn stop(&mut self) {
        if !self.playing {
            return;
        }
        self.playing = false;
        // The async task will detect the process exit and notify
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Get a handle to await playback completion.
    pub fn wait_handle(&self) -> Arc<Notify> {
        self.finished.clone()
    }
}
