// Configuration and constants, ported from mod/settings.py
// All settings are loaded from environment variables with sensible defaults.

use std::env;
use std::fs;
use std::path::PathBuf;

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .map(|v| v != 0)
        .unwrap_or(default)
}

fn env_str(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_int(key: &str, default: i32) -> i32 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Take and remove an env var (like Python's os.environ.pop)
fn env_pop(key: &str, default: &str) -> String {
    let val = env::var(key).unwrap_or_else(|_| default.to_string());
    // SAFETY: We only call this during single-threaded init before spawning any threads.
    unsafe { env::remove_var(key) };
    val
}

// Well-known instance constants
pub const PEDALBOARD_INSTANCE: &str = "/pedalboard";
pub const PEDALBOARD_INSTANCE_ID: i32 = 9995;
pub const PEDALBOARD_URI: &str = "urn:mod:pedalboard";

pub struct Settings {
    // Development flags
    pub dev_environment: bool,
    pub dev_hmi: bool,
    pub dev_host: bool,
    pub dev_api: bool,
    pub desktop: bool,
    pub log: i32,

    // Device identity
    pub api_key: Option<String>,
    pub device_key: Option<String>,
    pub device_tag: Option<String>,
    pub device_uid: Option<String>,

    // Paths
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub user_files_dir: PathBuf,
    pub keys_path: String,
    pub download_tmp_dir: PathBuf,
    pub pedalboard_tmp_dir: PathBuf,

    // JSON config files
    pub favorites_json_file: PathBuf,
    pub last_state_json_file: PathBuf,
    pub preferences_json_file: PathBuf,
    pub user_id_json_file: PathBuf,
    pub user_banks_json_file: PathBuf,
    pub factory_banks_json_file: PathBuf,

    // LV2 directories
    pub lv2_plugin_dir: PathBuf,
    pub lv2_pedalboards_dir: PathBuf,
    pub lv2_factory_pedalboards_dir: PathBuf,

    // HMI settings
    pub hmi_baud_rate: i32,
    pub hmi_serial_port: String,
    pub hmi_timeout: i32,
    pub hmi_transport: String,
    pub hmi_tcp_host: String,
    pub hmi_tcp_port: i32,

    // Model
    pub model_cpu: Option<String>,
    pub model_type: Option<String>,

    // Network
    pub device_webserver_port: u16,
    pub device_host_port: i32,

    // HTML / templates
    pub html_dir: PathBuf,
    pub mod_ui_dir: PathBuf,
    pub default_pedalboard: PathBuf,
    pub default_icon_template: PathBuf,
    pub default_settings_template: PathBuf,

    // Cloud API
    pub cloud_http_address: String,
    pub cloud_labs_http_address: String,
    pub plugins_http_address: String,
    pub pedalboards_http_address: String,
    pub pedalboards_labs_http_address: String,
    pub controlchain_http_address: String,

    // Image/firmware version
    pub image_version: Option<String>,

    // Hardware descriptor
    pub hardware_desc_file: PathBuf,

    // Built-in plugin URIs
    pub midi_beat_clock_sender_uri: String,
    pub midi_beat_clock_sender_instance_id: i32,
    pub midi_beat_clock_sender_output_port: String,

    pub tuner_uri: String,
    pub tuner_input_port: String,
    pub tuner_monitor_port: String,
    pub tuner_instance_id: i32,

    pub pedalboard_instance: String,
    pub pedalboard_instance_id: i32,
    pub pedalboard_uri: String,

    pub untitled_pedalboard_name: String,
    pub default_snapshot_name: String,

    // MIDI program change channel routing (0-based, -1 = disabled)
    pub midi_pedalboard_channel: i32,
    pub midi_snapshot_channel: i32,

    // Audio paths
    pub capture_path: PathBuf,
    pub playback_path: PathBuf,

    // Update/firmware files
    pub update_mod_os_file: PathBuf,
    pub update_mod_os_helper_file: PathBuf,
    pub update_cc_firmware_file: PathBuf,
    pub using_256_frames_file: PathBuf,
}

impl Settings {
    pub fn from_env() -> Self {
        let dev_environment = env_bool("MOD_DEV_ENVIRONMENT", false);
        let dev_hmi = env_bool("MOD_DEV_HMI", dev_environment);
        let dev_host = env_bool("MOD_DEV_HOST", dev_environment);

        let data_dir = PathBuf::from(env_str(
            "MOD_DATA_DIR",
            &dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/root"))
                .join("data")
                .to_string_lossy(),
        ));
        let cache_dir = data_dir.join(".cache");

        let mut keys_path = env_str("MOD_KEYS_PATH", &data_dir.join("keys").to_string_lossy());
        if !keys_path.ends_with('/') {
            keys_path.push('/');
        }
        // SAFETY: Called during single-threaded init before spawning any threads.
        unsafe { env::set_var("MOD_KEYS_PATH", &keys_path) };

        let lv2_plugin_dir = PathBuf::from(env_str(
            "MOD_USER_PLUGINS_DIR",
            &dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/root"))
                .join(".lv2")
                .to_string_lossy(),
        ));
        let lv2_pedalboards_dir = PathBuf::from(env_str(
            "MOD_USER_PEDALBOARDS_DIR",
            &dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/root"))
                .join(".pedalboards")
                .to_string_lossy(),
        ));

        let html_dir = PathBuf::from(env_str("MOD_HTML_DIR", "html/"));
        let default_pedalboard = lv2_pedalboards_dir.join("default.pedalboard");

        let image_version_path = env_pop("MOD_IMAGE_VERSION_PATH", "/etc/mod-release/release");
        let image_version = fs::read_to_string(&image_version_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let tuner = env_str("MOD_TUNER_PLUGIN", "gxtuner");
        let (tuner_uri, tuner_input_port, tuner_monitor_port) = if tuner == "tuna" {
            ("urn:mod:tuna".into(), "in".into(), "freq_out".into())
        } else {
            ("http://guitarix.sourceforge.net/plugins/gxtuner#tuner".into(), "in".into(), "FREQ".into())
        };

        let update_os_filename = env_str("MOD_UPDATE_MOD_OS_FILE", "modduo.tar")
            .replace('*', "cloud");

        Settings {
            dev_environment,
            dev_hmi,
            dev_host,
            dev_api: env_bool("MOD_DEV_API", false),
            desktop: env_bool("MOD_DESKTOP", false),
            log: env_int("MOD_LOG", 0),

            api_key: env::var("MOD_API_KEY").ok(),
            device_key: env::var("MOD_DEVICE_KEY").ok(),
            device_tag: env::var("MOD_DEVICE_TAG").ok(),
            device_uid: env::var("MOD_DEVICE_UID").ok(),

            cache_dir,
            user_files_dir: PathBuf::from(env_str("MOD_USER_FILES_DIR", "/data/user-files")),
            keys_path,
            favorites_json_file: PathBuf::from(env_str(
                "MOD_FAVORITES_JSON",
                &data_dir.join("favorites.json").to_string_lossy(),
            )),
            last_state_json_file: PathBuf::from(env_str(
                "MOD_LAST_STATE_JSON",
                &data_dir.join("last.json").to_string_lossy(),
            )),
            preferences_json_file: PathBuf::from(env_str(
                "MOD_PREFERENCES_JSON",
                &data_dir.join("prefs.json").to_string_lossy(),
            )),
            user_id_json_file: PathBuf::from(env_str(
                "MOD_USER_ID_JSON",
                &data_dir.join("user-id.json").to_string_lossy(),
            )),
            user_banks_json_file: PathBuf::from(env_str(
                "MOD_USER_BANKS_JSON",
                &data_dir.join("banks.json").to_string_lossy(),
            )),
            factory_banks_json_file: PathBuf::from(env_str(
                "MOD_FACTORY_BANKS_JSON",
                "/usr/share/mod/banks.json",
            )),
            download_tmp_dir: PathBuf::from(env_str("MOD_DOWNLOAD_TMP_DIR", "/tmp/mod-ui")),
            pedalboard_tmp_dir: PathBuf::from(env_str(
                "MOD_PEDALBOARD_TMP_DIR",
                &data_dir.join("pedalboard-tmp-data").to_string_lossy(),
            )),
            data_dir,

            lv2_plugin_dir,
            lv2_pedalboards_dir,
            lv2_factory_pedalboards_dir: PathBuf::from(env_str(
                "MOD_FACTORY_PEDALBOARDS_DIR",
                "/usr/share/mod/pedalboards",
            )),

            hmi_baud_rate: env_int("MOD_HMI_BAUD_RATE", 10_000_000),
            hmi_serial_port: env_str("MOD_HMI_SERIAL_PORT", "/dev/ttyUSB0"),
            hmi_timeout: env_int("MOD_HMI_TIMEOUT", 0),
            hmi_transport: env_str("MOD_HMI_TRANSPORT", "tcp"),
            hmi_tcp_host: env_str("MOD_HMI_TCP_HOST", "127.0.0.1"),
            hmi_tcp_port: env_int("MOD_HMI_TCP_PORT", 9898),

            model_cpu: env::var("MOD_MODEL_CPU").ok(),
            model_type: env::var("MOD_MODEL_TYPE").ok(),

            device_webserver_port: env_int("MOD_DEVICE_WEBSERVER_PORT", 8888) as u16,
            device_host_port: env_int("MOD_DEVICE_HOST_PORT", 5555),

            default_icon_template: html_dir.join("resources/templates/pedal-default.html"),
            default_settings_template: html_dir.join("resources/settings.html"),
            mod_ui_dir: PathBuf::from(env_str("MOD_UI_DIR", ".")),
            html_dir,
            default_pedalboard,

            cloud_http_address: env_pop("MOD_CLOUD_HTTP_ADDRESS", "https://api.mod.audio/v2"),
            cloud_labs_http_address: env_pop(
                "MOD_CLOUD_LABS_HTTP_ADDRESS",
                "https://api-labs.mod.audio/v2",
            ),
            plugins_http_address: env_pop(
                "MOD_PLUGINS_HTTP_ADDRESS",
                "https://pedalboards.mod.audio/plugins",
            ),
            pedalboards_http_address: env_pop(
                "MOD_PEDALBOARDS_HTTP_ADDRESS",
                "https://pedalboards.mod.audio",
            ),
            pedalboards_labs_http_address: env_pop(
                "MOD_PEDALBOARDS_LABS_HTTP_ADDRESS",
                "https://pedalboards-labs.mod.audio",
            ),
            controlchain_http_address: env_pop(
                "MOD_CONTROLCHAIN_HTTP_ADDRESS",
                "https://download.mod.audio/releases/cc-firmware/v3",
            ),

            image_version,
            hardware_desc_file: PathBuf::from(env_pop(
                "MOD_HARDWARE_DESC_FILE",
                "/etc/mod-hardware-descriptor.json",
            )),

            midi_beat_clock_sender_uri: "urn:mod:mclk".into(),
            midi_beat_clock_sender_instance_id: 9993,
            midi_beat_clock_sender_output_port: "mclk".into(),

            tuner_uri,
            tuner_input_port,
            tuner_monitor_port,
            tuner_instance_id: 9994,

            pedalboard_instance: "/pedalboard".into(),
            pedalboard_instance_id: 9995,
            pedalboard_uri: "urn:mod:pedalboard".into(),

            untitled_pedalboard_name: "Untitled Pedalboard".into(),
            default_snapshot_name: "Default".into(),

            midi_pedalboard_channel: env_int("MOD_MIDI_PEDALBOARD_CHANNEL", 0),
            midi_snapshot_channel: env_int("MOD_MIDI_SNAPSHOT_CHANNEL", -1),

            capture_path: PathBuf::from("/tmp/capture.ogg"),
            playback_path: PathBuf::from("/tmp/playback.ogg"),

            update_mod_os_file: PathBuf::from(format!("/data/{}", update_os_filename)),
            update_mod_os_helper_file: PathBuf::from("/data/boot-restore"),
            update_cc_firmware_file: PathBuf::from("/tmp/cc-firmware.bin"),
            using_256_frames_file: PathBuf::from("/data/using-256-frames"),
        }
    }
}
