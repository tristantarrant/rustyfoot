// Plugin/content store abstraction supporting multiple backends.

pub mod hydrogen;
pub mod musical_artifacts;
pub mod patchstorage;
pub mod tone3000;

use serde::{Deserialize, Serialize};

/// Description of an available store backend.
#[derive(Serialize, Clone)]
pub struct StoreSource {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// Unified search query across all backends.
#[derive(Deserialize)]
pub struct StoreQuery {
    pub q: Option<String>,
    pub category: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

/// A search result page.
#[derive(Serialize)]
pub struct StoreSearchResult {
    pub items: Vec<StoreItem>,
    pub page: u32,
    pub total: u64,
    pub total_pages: u32,
}

/// A single item in a store listing or detail view.
#[derive(Serialize, Clone)]
pub struct StoreItem {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub author: String,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub thumbnail_url: Option<String>,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
    pub download_count: u64,
    pub license: Option<String>,
    /// Only populated in detail view, not in search results.
    pub files: Vec<StoreFile>,
}

/// A downloadable file within a store item.
#[derive(Serialize, Clone)]
pub struct StoreFile {
    pub id: u64,
    pub filename: String,
    pub filesize: u64,
    pub target: Option<String>,
    pub url: String,
}

/// A category/filter option.
#[derive(Serialize)]
pub struct StoreCategory {
    pub id: String,
    pub name: String,
    pub slug: String,
}

/// Detect the system target slug for Patchstorage LV2 platform.
pub fn detect_target() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    { "rpi-aarch64" }
    #[cfg(target_arch = "arm")]
    { "patchbox-os-arm32" }
    #[cfg(target_arch = "x86_64")]
    { "linux-amd64" }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "arm", target_arch = "x86_64")))]
    { "unknown" }
}

pub const SOURCES: &[StoreSource] = &[
    StoreSource {
        id: "patchstorage",
        name: "Patchstorage",
        description: "LV2 audio plugins from patchstorage.com",
    },
    StoreSource {
        id: "tone3000",
        name: "Tone3000",
        description: "NAM models and impulse responses from tone3000.com",
    },
    StoreSource {
        id: "hydrogen",
        name: "Hydrogen Drumkits",
        description: "Drumkits for Hydrogen-compatible drum plugins",
    },
    StoreSource {
        id: "musical_artifacts",
        name: "Musical Artifacts",
        description: "SF2 soundfonts, SFZ instruments, and MIDI files from musical-artifacts.com",
    },
];
