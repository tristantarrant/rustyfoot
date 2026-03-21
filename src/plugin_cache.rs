// Plugin list cache for fast startup
// Serves a cached plugin list immediately, refreshes in the background.
// Watches the user plugin directory for changes via inotify.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};

#[derive(Clone)]
pub struct PluginCache {
    plugins: Arc<RwLock<Option<Vec<Value>>>>,
    ready: Arc<Notify>,
    cache_file: PathBuf,
}

impl PluginCache {
    /// Create a new cache, loading from disk if available.
    pub fn new(cache_dir: &Path) -> Self {
        let cache_file = cache_dir.join("plugins.json");
        let cached = load_from_disk(&cache_file);
        let has_cache = cached.is_some();

        let cache = Self {
            plugins: Arc::new(RwLock::new(cached)),
            ready: Arc::new(Notify::new()),
            cache_file,
        };

        if has_cache {
            // Notify immediately since we have data to serve
            cache.ready.notify_waiters();
            tracing::debug!("Plugin cache loaded from disk");
        }

        cache
    }

    /// Spawn background refresh. Call after server starts.
    pub fn spawn_refresh(&self) {
        let cache = self.clone();
        actix_web::rt::spawn(async move {
            // Run the blocking FFI call on a thread pool
            let plugins = tokio::task::spawn_blocking(|| {
                tracing::info!("Scanning LV2 plugins...");
                let start = std::time::Instant::now();
                let plugins = crate::lv2_utils::get_all_plugins();
                tracing::info!(
                    "Plugin scan complete: {} plugins in {:.1}s",
                    plugins.len(),
                    start.elapsed().as_secs_f64()
                );
                plugins
            })
            .await;

            if let Ok(plugins) = plugins {
                // Write to disk
                save_to_disk(&cache.cache_file, &plugins);

                // Update in-memory cache
                *cache.plugins.write().await = Some(plugins);
                cache.ready.notify_waiters();
            }
        });
    }

    /// Get the cached plugin list. Waits for first scan if no disk cache.
    pub async fn get_plugins(&self) -> Vec<Value> {
        // Fast path: cache is ready
        {
            let guard = self.plugins.read().await;
            if let Some(ref plugins) = *guard {
                return plugins.clone();
            }
        }

        // Slow path: wait for background scan to finish
        self.ready.notified().await;
        self.plugins.read().await.clone().unwrap_or_default()
    }

    /// Force a refresh (e.g. after plugin install/remove).
    /// Safe to call from any context (actix handlers, std threads, etc.).
    pub fn refresh(&self) {
        let cache = self.clone();
        tokio::spawn(async move {
            let plugins = tokio::task::spawn_blocking(|| {
                tracing::info!("Scanning LV2 plugins...");
                let start = std::time::Instant::now();
                let plugins = crate::lv2_utils::get_all_plugins();
                tracing::info!(
                    "Plugin scan complete: {} plugins in {:.1}s",
                    plugins.len(),
                    start.elapsed().as_secs_f64()
                );
                plugins
            })
            .await;

            if let Ok(plugins) = plugins {
                save_to_disk(&cache.cache_file, &plugins);
                *cache.plugins.write().await = Some(plugins);
                cache.ready.notify_waiters();
            }
        });
    }

    /// Watch a directory for new/removed LV2 bundles and auto-refresh.
    pub fn spawn_watcher(&self, plugin_dir: PathBuf) {
        use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

        let cache = self.clone();
        let rt = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create plugin directory watcher: {}", e);
                    return;
                }
            };
            if let Err(e) = watcher.watch(&plugin_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch {}: {}", plugin_dir.display(), e);
                return;
            }
            tracing::info!("Watching {} for plugin changes", plugin_dir.display());

            // Debounce: collect events for 2s before triggering a refresh
            loop {
                // Block until first event
                let event = match rx.recv() {
                    Ok(e) => e,
                    Err(_) => break,
                };
                // Drain any additional events within the debounce window
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut events = vec![event];
                loop {
                    let timeout = deadline.saturating_duration_since(std::time::Instant::now());
                    if timeout.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(timeout) {
                        Ok(e) => events.push(e),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }

                // Process the batch: add/remove bundles from lilv world
                let mut changed = false;
                for event in events {
                    let event = match event {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    for path in &event.paths {
                        let is_lv2 = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.ends_with(".lv2"));
                        if !is_lv2 {
                            continue;
                        }
                        let bundle = path.to_string_lossy();
                        match event.kind {
                            EventKind::Create(_) => {
                                tracing::info!("New plugin bundle detected: {}", bundle);
                                crate::lv2_utils::add_bundle_to_lilv_world(&bundle);
                                changed = true;
                            }
                            EventKind::Remove(_) => {
                                tracing::info!("Plugin bundle removed: {}", bundle);
                                crate::lv2_utils::remove_bundle_from_lilv_world(&bundle, None);
                                changed = true;
                            }
                            _ => {}
                        }
                    }
                }
                if changed {
                    tracing::info!("Scanning LV2 plugins...");
                    let start = std::time::Instant::now();
                    let plugins = crate::lv2_utils::get_all_plugins();
                    tracing::info!(
                        "Plugin scan complete: {} plugins in {:.1}s",
                        plugins.len(),
                        start.elapsed().as_secs_f64()
                    );
                    save_to_disk(&cache.cache_file, &plugins);
                    let cache2 = cache.clone();
                    rt.spawn(async move {
                        *cache2.plugins.write().await = Some(plugins);
                        cache2.ready.notify_waiters();
                    });
                }
            }
        });
    }
}

fn load_from_disk(path: &Path) -> Option<Vec<Value>> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_to_disk(path: &Path, plugins: &[Value]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(plugins) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                tracing::warn!("Failed to write plugin cache: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize plugin cache: {}", e),
    }
}
