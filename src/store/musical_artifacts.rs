// Musical Artifacts store backend.
// Fetches SF2, SFZ, and MIDI files from musical-artifacts.com.

use reqwest::Client;
use serde::Deserialize;

use super::{StoreCategory, StoreFile, StoreItem, StoreQuery, StoreSearchResult};

const API_BASE: &str = "https://musical-artifacts.com/artifacts.json";

/// Supported format filters and their display names.
const FORMAT_CATEGORIES: &[(&str, &str)] = &[
    ("sf2", "SF2 Soundfonts"),
    ("sfz", "SFZ Instruments"),
    ("mid", "MIDI Files"),
];

/// A parsed artifact from the JSON API.
#[derive(Deserialize)]
struct Artifact {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    file: String,
    #[serde(default)]
    download_count: u64,
}

pub struct MusicalArtifactsBackend {
    client: Client,
}

impl MusicalArtifactsBackend {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("rustyfoot/0.1")
                .build()
                .expect("failed to create HTTP client"),
        }
    }

    pub async fn search(&self, query: &StoreQuery) -> Result<StoreSearchResult, String> {
        let per_page = query.per_page.unwrap_or(24);
        let page = query.page.unwrap_or(1).max(1);

        // Build format filter from category or default to all supported formats
        let formats = if let Some(ref cat) = query.category {
            cat.clone()
        } else {
            FORMAT_CATEGORIES.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(",")
        };

        let mut url = format!("{}?formats={}&per_page={}&page={}", API_BASE, formats, per_page, page);
        if let Some(ref q) = query.q {
            if !q.is_empty() {
                url.push_str(&format!("&q={}", urlencoding::encode(q)));
            }
        }

        let resp = self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("failed to fetch artifacts: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("musical-artifacts returned {}", resp.status()));
        }

        let artifacts: Vec<Artifact> = resp.json().await
            .map_err(|e| format!("failed to parse artifacts: {}", e))?;

        let count = artifacts.len() as u64;

        // The API doesn't return total count, so we estimate:
        // if we got a full page, there are probably more
        let has_more = count >= per_page as u64;
        let total = if has_more {
            // Estimate: at least current position + 1 more page
            (page as u64) * (per_page as u64) + per_page as u64
        } else {
            ((page - 1) as u64) * (per_page as u64) + count
        };
        let total_pages = if has_more {
            page + 1
        } else {
            page
        };

        let items: Vec<StoreItem> = artifacts.into_iter()
            .map(|a| a.to_store_item())
            .collect();

        Ok(StoreSearchResult {
            items,
            page,
            total,
            total_pages,
        })
    }

    pub async fn get(&self, id: u64) -> Result<StoreItem, String> {
        let url = format!("https://musical-artifacts.com/artifacts/{}.json", id);

        let resp = self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("failed to fetch artifact: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("musical-artifacts returned {}", resp.status()));
        }

        let artifact: Artifact = resp.json().await
            .map_err(|e| format!("failed to parse artifact: {}", e))?;

        let mut item = artifact.to_store_item();

        // Add the download file
        if !item.url.is_empty() {
            let filename = item.url.rsplit('/').next()
                .unwrap_or("download")
                .to_string();
            item.files = vec![StoreFile {
                id: 0,
                filename,
                filesize: 0,
                target: None,
                url: item.url.clone(),
            }];
        }

        Ok(item)
    }

    pub async fn categories(&self) -> Result<Vec<StoreCategory>, String> {
        Ok(FORMAT_CATEGORIES.iter().map(|(id, name)| StoreCategory {
            id: id.to_string(),
            name: name.to_string(),
            slug: id.to_string(),
        }).collect())
    }

    pub async fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self.client.get(url)
            .send()
            .await
            .map_err(|e| format!("download failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("download returned {}", resp.status()));
        }

        resp.bytes().await
            .map(|b| b.to_vec())
            .map_err(|e| format!("download read failed: {}", e))
    }
}

impl Artifact {
    fn to_store_item(self) -> StoreItem {
        let categories: Vec<String> = self.formats.iter()
            .filter_map(|f| {
                FORMAT_CATEGORIES.iter()
                    .find(|(id, _)| id == f)
                    .map(|(_, name)| name.to_string())
            })
            .collect();

        StoreItem {
            id: self.id,
            title: self.name,
            description: self.description,
            author: self.author,
            categories,
            tags: self.tags,
            thumbnail_url: None,
            url: self.file,
            created_at: String::new(),
            updated_at: String::new(),
            download_count: self.download_count,
            license: if self.license.is_empty() { None } else { Some(self.license) },
            files: vec![],
        }
    }
}

/// Determine the user-files subdirectory for a given filename.
pub fn dest_subdir_for_file(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".sf2") {
        "SF2 Instruments"
    } else if lower.ends_with(".sfz") {
        "SFZ Instruments"
    } else if lower.ends_with(".mid") || lower.ends_with(".midi") {
        "MIDI Clips"
    } else {
        "Audio Samples"
    }
}
