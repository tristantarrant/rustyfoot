#[allow(dead_code)]
mod addressings;
#[allow(dead_code)]
mod bank;
mod communication;
mod development;
#[allow(dead_code)]
mod hmi;
#[allow(dead_code)]
mod host;
#[allow(dead_code)]
mod lv2_utils;
#[allow(dead_code)]
mod mod_protocol;
mod plugin_cache;
#[allow(dead_code)]
mod profile;
#[allow(dead_code)]
mod protocol;
mod recorder;
mod screenshot;
mod session;
mod store;
mod settings;
#[allow(dead_code)]
mod template;
#[allow(dead_code)]
mod tempo;
#[allow(dead_code)]
mod tuner;
#[allow(dead_code)]
mod utils;
mod web;

use actix_files::Files;
use actix_web::{App, HttpServer, middleware};
use session::SharedSession;
use settings::Settings;

/// Shared application state, accessible from all handlers via web::Data<AppState>.
pub struct AppState {
    pub settings: Settings,
    pub session: SharedSession,
    /// Broadcast channel for WebSocket messages (session → all WS clients)
    pub ws_broadcast: tokio::sync::broadcast::Sender<String>,
    /// Whether the mod-host read loop is already running
    pub read_loop_running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Cached plugin list for fast startup
    pub plugin_cache: plugin_cache::PluginCache,
    /// Patchstorage store backend
    pub store_patchstorage: store::patchstorage::PatchstorageBackend,
    /// Tone3000 store backend
    pub store_tone3000: store::tone3000::Tone3000Backend,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,actix_web=warn")),
        )
        .init();

    let settings = Settings::from_env();

    // Ensure well-known user file directories exist
    ensure_user_file_dirs(&settings.user_files_dir);

    // Copy default and factory pedalboards if not present
    ensure_default_pedalboard(&settings.mod_ui_dir, &settings.lv2_pedalboards_dir);
    install_factory_pedalboards(&settings.mod_ui_dir, &settings.lv2_pedalboards_dir);

    // Initialize the LV2 world (loads plugin metadata via lilv)
    lv2_utils::init();

    // Initialize JACK client (needed for MIDI device listing, audio ports, etc.)
    if lv2_utils::init_jack() {
        tracing::debug!("JACK client initialized");
    } else {
        tracing::warn!("Failed to initialize JACK client");
    }

    let bind_addr = if settings.desktop {
        "127.0.0.1"
    } else {
        "0.0.0.0"
    };
    let port = settings.device_webserver_port;
    let html_dir = settings.html_dir.clone();

    let shared_session = session::create_session(&settings);

    let (ws_tx, _) = tokio::sync::broadcast::channel::<String>(256);

    // Give the session a reference to the broadcast sender
    {
        let mut session = shared_session.write().await;
        session.ws_broadcast = Some(ws_tx.clone());
    }

    let pcache = plugin_cache::PluginCache::new(&settings.cache_dir);
    let tone3000 = store::tone3000::Tone3000Backend::new(&settings.data_dir);

    let app_state = actix_web::web::Data::new(AppState {
        settings,
        session: shared_session,
        ws_broadcast: ws_tx,
        read_loop_running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        plugin_cache: pcache,
        store_patchstorage: store::patchstorage::PatchstorageBackend::new(),
        store_tone3000: tone3000,
    });

    // Start background plugin scan (serves disk cache immediately if available)
    app_state.plugin_cache.spawn_refresh();

    // Connect to mod-host, enable MIDI monitoring, and load last pedalboard at startup
    // (don't wait for a browser to connect)
    {
        let state = app_state.clone().into_inner();
        actix_web::rt::spawn(async move {
            startup_connect(state).await;
        });
    }

    tracing::info!("Starting rustyfoot on {}:{}", bind_addr, port);
    tracing::debug!("Serving static files from {:?}", html_dir);

    HttpServer::new(move || {
        let html_dir_str = html_dir.to_string_lossy().to_string();

        App::new()
            .wrap(middleware::NormalizePath::trim())
            .wrap(middleware::Logger::new("%s %r").exclude("/ping"))
            .app_data(app_state.clone())
            // ---- API endpoints ----
            // Misc
            .service(web::handlers::misc::ping)
            .service(web::handlers::misc::hello)
            // System
            .service(web::handlers::system::system_info)
            .service(web::handlers::system::system_prefs)
            .service(web::handlers::system::system_exechange)
            .service(web::handlers::system::system_cleanup)
            .service(web::handlers::system::reset)
            // Effect/Plugin
            .service(web::handlers::effect::effect_list)
            .service(web::handlers::effect::effect_get)
            .service(web::handlers::effect::effect_get_non_cached)
            .service(web::handlers::effect::effect_bulk)
            .service(web::handlers::effect::effect_add)
            .service(web::handlers::effect::effect_remove)
            .service(web::handlers::effect::effect_connect)
            .service(web::handlers::effect::effect_disconnect)
            .service(web::handlers::effect::effect_parameter_address)
            .service(web::handlers::effect::effect_parameter_set)
            .service(web::handlers::effect::effect_preset_load)
            .service(web::handlers::effect::effect_preset_save_new)
            .service(web::handlers::effect::effect_preset_save_replace)
            .service(web::handlers::effect::effect_preset_delete)
            .service(web::handlers::effect::effect_image)
            .service(web::handlers::effect::effect_file)
            .service(web::handlers::effect::effect_install)
            // Pedalboard
            .service(web::handlers::pedalboard::pedalboard_list)
            .service(web::handlers::pedalboard::pedalboard_save)
            .service(web::handlers::pedalboard::pedalboard_load_bundle)
            .service(web::handlers::pedalboard::pedalboard_info)
            .service(web::handlers::pedalboard::pedalboard_remove)
            .service(web::handlers::pedalboard::pedalboard_rename)
            .service(web::handlers::pedalboard::pedalboard_image)
            .service(web::handlers::pedalboard::pedalboard_image_wait)
            .service(web::handlers::pedalboard::pedalboard_image_generate)
            .service(web::handlers::pedalboard::pedalboard_image_check)
            .service(web::handlers::pedalboard::pedalboard_transport_set_sync_mode)
            .service(web::handlers::pedalboard::pedalboard_cv_add)
            .service(web::handlers::pedalboard::pedalboard_cv_remove)
            // Snapshot
            .service(web::handlers::snapshot::snapshot_save)
            .service(web::handlers::snapshot::snapshot_saveas)
            .service(web::handlers::snapshot::snapshot_rename)
            .service(web::handlers::snapshot::snapshot_remove)
            .service(web::handlers::snapshot::snapshot_list)
            .service(web::handlers::snapshot::snapshot_name)
            .service(web::handlers::snapshot::snapshot_load)
            // Bank
            .service(web::handlers::bank::bank_load)
            .service(web::handlers::bank::bank_save)
            // Recording
            .service(web::handlers::recording::recording_start)
            .service(web::handlers::recording::recording_stop)
            .service(web::handlers::recording::recording_reset)
            .service(web::handlers::recording::recording_play)
            .service(web::handlers::recording::recording_download)
            // JACK/Audio
            .service(web::handlers::jack::jack_get_midi_devices)
            .service(web::handlers::jack::jack_set_midi_devices)
            .service(web::handlers::jack::set_buffersize)
            .service(web::handlers::jack::reset_xruns)
            .service(web::handlers::jack::truebypass)
            // Auth & Tokens
            .service(web::handlers::auth::auth_nonce)
            .service(web::handlers::auth::auth_token)
            .service(web::handlers::auth::tokens_get)
            .service(web::handlers::auth::tokens_delete)
            .service(web::handlers::auth::tokens_save)
            // Favorites & Preferences
            .service(web::handlers::favorites::favorites_add)
            .service(web::handlers::favorites::favorites_remove)
            .service(web::handlers::favorites::config_set)
            .service(web::handlers::favorites::save_user_id)
            // Files
            .service(web::handlers::files::files_list)
            // Store
            .service(web::handlers::store::store_sources)
            .service(web::handlers::store::store_search)
            .service(web::handlers::store::store_get)
            .service(web::handlers::store::store_categories)
            .service(web::handlers::store::store_install)
            .service(web::handlers::store::tone3000_auth_status)
            .service(web::handlers::store::tone3000_auth_start)
            .service(web::handlers::store::tone3000_auth_callback)
            .service(web::handlers::store::tone3000_auth_disconnect)
            // File browser
            .service(web::handlers::filebrowser::filebrowser_page)
            .service(web::handlers::filebrowser::filebrowser_list)
            .service(web::handlers::filebrowser::filebrowser_upload)
            .service(web::handlers::filebrowser::filebrowser_download)
            .service(web::handlers::filebrowser::filebrowser_mkdir)
            .service(web::handlers::filebrowser::filebrowser_delete)
            .service(web::handlers::filebrowser::filebrowser_rename)
            // Plugin resources (must be before static files)
            .service(web::handlers::effect::effect_resource)
            // WebSocket
            .service(web::handlers::websocket::websocket)
            // ---- Template pages (before static files) ----
            .service(web::handlers::templates::template_page_bare)
            .service(web::handlers::templates::template_page_alias)
            .service(web::handlers::templates::bulk_template_loader)
            .service(web::handlers::templates::load_template)
            .service(web::handlers::templates::template_page)
            // ---- Static files fallback (must be last) ----
            .service(Files::new("/", &html_dir_str).prefer_utf8(true))
    })
    .bind((bind_addr, port))?
    .shutdown_timeout(3)
    .run()
    .await
}

fn ensure_user_file_dirs(user_files_dir: &std::path::Path) {
    let dirs = [
        "Audio Loops",
        "Audio Recordings",
        "Audio Samples",
        "Audio Tracks",
        "Speaker Cabinets IRs",
        "Hydrogen Drumkits",
        "Reverb IRs",
        "MIDI Clips",
        "MIDI Songs",
        "SF2 Instruments",
        "SFZ Instruments",
        "Aida DSP Models",
        "NAM Models",
    ];
    for dir in &dirs {
        let path = user_files_dir.join(dir);
        if let Err(e) = std::fs::create_dir_all(&path) {
            tracing::warn!("Failed to create {}: {}", path.display(), e);
        }
    }
}

fn ensure_default_pedalboard(ui_dir: &std::path::Path, pedalboards_dir: &std::path::Path) {
    let dest = pedalboards_dir.join("default.pedalboard");
    if dest.exists() {
        return;
    }
    let src = ui_dir.join("default.pedalboard");
    if !src.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dest) {
        tracing::warn!("Failed to create {}: {}", dest.display(), e);
        return;
    }
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let target = dest.join(entry.file_name());
            if let Err(e) = std::fs::copy(entry.path(), &target) {
                tracing::warn!("Failed to copy {}: {}", entry.path().display(), e);
            }
        }
    }
    tracing::debug!("Installed default pedalboard to {}", dest.display());
}

fn install_factory_pedalboards(ui_dir: &std::path::Path, pedalboards_dir: &std::path::Path) {
    let src_dir = ui_dir.join("factory-pedalboards");
    if !src_dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(&src_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".pedalboard") {
            continue;
        }
        let dest = pedalboards_dir.join(&name);
        if dest.exists() {
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&dest) {
            tracing::warn!("Failed to create {}: {}", dest.display(), e);
            continue;
        }
        copy_dir_recursive(&entry.path(), &dest);
        tracing::debug!("Installed factory pedalboard {}", name.to_string_lossy());
    }
}

/// Connect to mod-host at startup, enable MIDI monitoring, and load the last pedalboard.
/// This runs as a background task so the HTTP server can start immediately.
async fn startup_connect(state: std::sync::Arc<AppState>) {
    use std::sync::atomic::Ordering;

    // Connect to mod-host
    let read_stream = {
        let mut session = state.session.write().await;
        session.host.start_session().await
    };

    // Spawn the notification read loop
    if let Some(read_stream) = read_stream {
        if !state.read_loop_running.swap(true, Ordering::SeqCst) {
            let state_for_reader = state.clone();
            let flag = state.read_loop_running.clone();
            actix_web::rt::spawn(async move {
                web::handlers::websocket::mod_host_read_loop(read_stream, state_for_reader).await;
                flag.store(false, Ordering::SeqCst);
            });
        }
    }

    // Load the last pedalboard
    let (raw_bank, last_pb) = bank::get_last_bank_and_pedalboard(&state.settings.last_state_json_file);
    if let Some(bundlepath) = last_pb {
        if std::path::Path::new(&bundlepath).is_dir() {
            let mut session = state.session.write().await;
            // Convert raw bank index (-1=All, 0=first user, ...) to internal bank_id
            session.host.bank_id = raw_bank + session.host.userbanks_offset;
            session.web_load_pedalboard(&bundlepath, false, &state.settings).await;
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) {
    let entries = match std::fs::read_dir(src) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let target = dest.join(entry.file_name());
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            let _ = std::fs::create_dir_all(&target);
            copy_dir_recursive(&entry.path(), &target);
        } else if ft.is_file() {
            if let Err(e) = std::fs::copy(entry.path(), &target) {
                tracing::warn!("Failed to copy {}: {}", entry.path().display(), e);
            }
        }
    }
}
