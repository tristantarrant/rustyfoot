// Plugin list cache for fast startup
// Serves a cached plugin list immediately, refreshes in the background.

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
            tracing::info!("Plugin cache loaded from disk");
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

    /// Force a refresh (e.g. after plugin install/remove)
    pub fn refresh(&self) {
        self.spawn_refresh();
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
