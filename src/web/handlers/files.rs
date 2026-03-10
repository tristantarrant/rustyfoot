// File listing endpoints, ported from webserver.py FilesList
// /files/list

use actix_web::{get, web, HttpResponse};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use crate::AppState;

// High-quality audio formats (lossless)
const HQ_AUDIO_EXTS: &[&str] = &["aif", "aifc", "aiff", "flac", "w64", "wav"];

// All supported audio formats (lossless + lossy)
const ALL_AUDIO_EXTS: &[&str] = &[
    "aif", "aifc", "aiff", "au", "bwf", "flac", "htk", "iff", "mat4", "mat5",
    "oga", "ogg", "opus", "paf", "pvf", "pvf5", "sd2", "sf", "snd", "svx",
    "vcc", "w64", "wav", "xi",
    "3g2", "3gp", "aac", "ac3", "amr", "ape", "mp2", "mp3", "mpc", "wma",
];

/// Map a filetype string to (directory_name, allowed_extensions).
fn get_dir_and_extensions(filetype: &str) -> Option<(&'static str, &'static [&'static str])> {
    match filetype {
        "audioloop"     => Some(("Audio Loops", ALL_AUDIO_EXTS)),
        "audiorecording"=> Some(("Audio Recordings", ALL_AUDIO_EXTS)),
        "audiosample"   => Some(("Audio Samples", ALL_AUDIO_EXTS)),
        "audiotrack"    => Some(("Audio Tracks", ALL_AUDIO_EXTS)),
        "cabsim"        => Some(("Speaker Cabinets IRs", HQ_AUDIO_EXTS)),
        "h2drumkit"     => Some(("Hydrogen Drumkits", &["h2drumkit"])),
        "ir"            => Some(("Reverb IRs", HQ_AUDIO_EXTS)),
        "midiclip"      => Some(("MIDI Clips", &["mid", "midi"])),
        "midisong"      => Some(("MIDI Songs", &["mid", "midi"])),
        "sf2"           => Some(("SF2 Instruments", &["sf2", "sf3"])),
        "sfz"           => Some(("SFZ Instruments", &["sfz"])),
        "aidadspmodel"  => Some(("Aida DSP Models", &["aidax", "json"])),
        "nammodel"      => Some(("NAM Models", &["nam"])),
        _ => None,
    }
}

/// Recursively walk a directory, collecting files with matching extensions.
fn walk_files(dir: &Path, extensions: &[&str], files: &mut Vec<(String, String)>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, extensions, files);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.iter().any(|&allowed| allowed.eq_ignore_ascii_case(ext)) {
                    let fullname = path.to_string_lossy().to_string();
                    let basename = path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    files.push((fullname, basename));
                }
            }
        }
    }
}

#[derive(Deserialize)]
pub struct FilesQuery {
    types: Option<String>,
}

/// GET /files/list?types=nammodel,ir,cabsim - list user files by semantic type
#[get("/files/list")]
pub async fn files_list(
    query: web::Query<FilesQuery>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let types_str = match &query.types {
        Some(t) => t.as_str(),
        None => {
            return HttpResponse::BadRequest().json(json!({"ok": false, "error": "Missing types"}));
        }
    };

    let user_files_dir = &state.settings.user_files_dir;
    let mut all_files = Vec::new();

    for filetype in types_str.split(',') {
        let filetype = filetype.trim();
        let (datadir, extensions) = match get_dir_and_extensions(filetype) {
            Some(v) => v,
            None => continue,
        };

        let dir = user_files_dir.join(datadir);
        let mut matched = Vec::new();
        walk_files(&dir, extensions, &mut matched);

        for (fullname, basename) in matched {
            all_files.push(json!({
                "fullname": fullname,
                "basename": basename,
                "filetype": filetype,
            }));
        }
    }

    all_files.sort_by(|a, b| {
        let fa = a["fullname"].as_str().unwrap_or("");
        let fb = b["fullname"].as_str().unwrap_or("");
        fa.cmp(fb)
    });

    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(json!({"ok": true, "files": all_files}))
}
