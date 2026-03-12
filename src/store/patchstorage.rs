// Patchstorage.com backend for LV2 plugin browsing and installation.

use reqwest::Client;
use serde::Deserialize;

use super::{StoreCategory, StoreFile, StoreItem, StoreQuery, StoreSearchResult};

const BASE_URL: &str = "https://patchstorage.com/api/beta";
const LV2_PLATFORM_ID: u32 = 8046;

// Target IDs for the LV2 platform
const TARGET_RPI_AARCH64: u32 = 8280;
const TARGET_PATCHBOX_ARM32: u32 = 8278;
const TARGET_LINUX_AMD64: u32 = 8279;

// Category name to Patchstorage ID mapping
fn category_name_to_id(name: &str) -> Option<u32> {
    match name {
        "Composition" => Some(378),
        "Effect" => Some(77),
        "Game" => Some(3317),
        "Other" => Some(1),
        "Sampler" => Some(75),
        "Sequencer" => Some(76),
        "Sound" => Some(372),
        "Synthesizer" => Some(74),
        "Utility" => Some(117),
        "Video" => Some(91),
        _ => name.parse::<u32>().ok(), // allow passing numeric IDs directly
    }
}

pub struct PatchstorageBackend {
    client: Client,
    target_id: u32,
    target_slug: &'static str,
}

impl PatchstorageBackend {
    pub fn new() -> Self {
        let target_slug = super::detect_target();
        let target_id = match target_slug {
            "rpi-aarch64" => TARGET_RPI_AARCH64,
            "patchbox-os-arm32" => TARGET_PATCHBOX_ARM32,
            "linux-amd64" => TARGET_LINUX_AMD64,
            _ => TARGET_RPI_AARCH64,
        };

        Self {
            client: Client::builder()
                .user_agent("rustyfoot/0.1")
                .build()
                .expect("failed to create HTTP client"),
            target_id,
            target_slug,
        }
    }

    pub async fn search(&self, query: &StoreQuery) -> Result<StoreSearchResult, String> {
        let page = query.page.unwrap_or(1);
        let per_page = query.per_page.unwrap_or(24).min(100);

        let mut url = format!(
            "{}/patches?platforms[]={}&targets[]={}&page={}&per_page={}",
            BASE_URL, LV2_PLATFORM_ID, self.target_id, page, per_page,
        );

        if let Some(ref q) = query.q {
            if !q.is_empty() {
                url.push_str(&format!("&search={}", urlencoding::encode(q)));
            }
        }

        if let Some(ref cat) = query.category {
            if !cat.is_empty() {
                if let Some(cat_id) = category_name_to_id(cat) {
                    url.push_str(&format!("&categories[]={}", cat_id));
                }
            }
        }

        url.push_str("&orderby=download_count&order=desc");

        let resp = self.client.get(&url).send().await
            .map_err(|e| format!("patchstorage request failed: {}", e))?;

        let total: u64 = resp.headers()
            .get("X-WP-Total")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let total_pages: u32 = resp.headers()
            .get("X-WP-TotalPages")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        let patches: Vec<PsListPatch> = resp.json().await
            .map_err(|e| format!("patchstorage parse failed: {}", e))?;

        let items = patches.into_iter().map(|p| p.into_store_item()).collect();

        Ok(StoreSearchResult { items, page, total, total_pages })
    }

    pub async fn get(&self, id: u64) -> Result<StoreItem, String> {
        let url = format!("{}/patches/{}", BASE_URL, id);
        let resp = self.client.get(&url).send().await
            .map_err(|e| format!("patchstorage request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("patchstorage returned {}", resp.status()));
        }

        let patch: PsDetailPatch = resp.json().await
            .map_err(|e| format!("patchstorage parse failed: {}", e))?;

        Ok(patch.into_store_item(self.target_slug))
    }

    pub async fn categories(&self) -> Result<Vec<StoreCategory>, String> {
        let url = format!("{}/categories?per_page=100", BASE_URL);
        let resp = self.client.get(&url).send().await
            .map_err(|e| format!("patchstorage request failed: {}", e))?;

        let cats: Vec<PsCategory> = resp.json().await
            .map_err(|e| format!("patchstorage parse failed: {}", e))?;

        Ok(cats.into_iter().map(|c| StoreCategory {
            id: c.id.to_string(),
            name: c.name,
            slug: c.slug,
        }).collect())
    }

    pub async fn download(&self, id: u64, file_id: u64) -> Result<Vec<u8>, String> {
        let url = format!("{}/patches/{}/files/{}/download/", BASE_URL, id, file_id);
        let resp = self.client.get(&url).send().await
            .map_err(|e| format!("patchstorage download failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("patchstorage download returned {}", resp.status()));
        }

        resp.bytes().await
            .map(|b| b.to_vec())
            .map_err(|e| format!("patchstorage download read failed: {}", e))
    }
}

// Patchstorage API response types (list endpoint — no files)

#[derive(Deserialize)]
struct PsListPatch {
    id: u64,
    title: String,
    #[serde(default)]
    excerpt: String,
    url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    artwork: Option<PsArtwork>,
    #[serde(default)]
    author: Option<PsAuthor>,
    #[serde(default)]
    categories: Vec<PsTaxonomy>,
    #[serde(default)]
    tags: Vec<PsTaxonomy>,
    #[serde(default)]
    license: Option<PsTaxonomy>,
}

impl PsListPatch {
    fn into_store_item(self) -> StoreItem {
        StoreItem {
            id: self.id,
            title: self.title,
            description: self.excerpt,
            author: self.author.map(|a| a.name).unwrap_or_default(),
            categories: self.categories.into_iter().map(|c| c.name).collect(),
            tags: self.tags.into_iter().map(|t| t.name).collect(),
            thumbnail_url: self.artwork.map(|a| a.thumbnail_url),
            url: self.url,
            created_at: self.created_at,
            updated_at: self.updated_at,
            download_count: self.download_count,
            license: self.license.map(|l| l.name),
            files: Vec::new(),
        }
    }
}

// Patchstorage API response types (detail endpoint — includes files)

#[derive(Deserialize)]
struct PsDetailPatch {
    id: u64,
    title: String,
    #[serde(default)]
    content: String,
    url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    artwork: Option<PsArtwork>,
    #[serde(default)]
    author: Option<PsAuthor>,
    #[serde(default)]
    categories: Vec<PsTaxonomy>,
    #[serde(default)]
    tags: Vec<PsTaxonomy>,
    #[serde(default)]
    license: Option<PsLicense>,
    #[serde(default)]
    files: Vec<PsFile>,
}

impl PsDetailPatch {
    fn into_store_item(self, target_slug: &str) -> StoreItem {
        let files = self.files.into_iter()
            .filter(|f| {
                f.target.as_ref()
                    .map(|t| t.slug == target_slug)
                    .unwrap_or(true) // include files without a target
            })
            .map(|f| StoreFile {
                id: f.id,
                filename: f.filename,
                filesize: f.filesize,
                target: f.target.map(|t| t.slug),
                url: f.url,
            })
            .collect();

        StoreItem {
            id: self.id,
            title: self.title,
            description: self.content,
            author: self.author.map(|a| a.name).unwrap_or_default(),
            categories: self.categories.into_iter().map(|c| c.name).collect(),
            tags: self.tags.into_iter().map(|t| t.name).collect(),
            thumbnail_url: self.artwork.map(|a| a.thumbnail_url),
            url: self.url,
            created_at: self.created_at,
            updated_at: self.updated_at,
            download_count: self.download_count,
            license: self.license.map(|l| l.name),
            files,
        }
    }
}

#[derive(Deserialize)]
struct PsArtwork {
    #[allow(dead_code)]
    url: String,
    thumbnail_url: String,
}

#[derive(Deserialize)]
struct PsAuthor {
    name: String,
}

#[derive(Deserialize)]
struct PsTaxonomy {
    #[allow(dead_code)]
    id: u64,
    name: String,
    #[allow(dead_code)]
    slug: String,
}

#[derive(Deserialize)]
struct PsLicense {
    name: String,
}

#[derive(Deserialize)]
struct PsFile {
    id: u64,
    url: String,
    filesize: u64,
    filename: String,
    target: Option<PsTarget>,
}

#[derive(Deserialize)]
struct PsTarget {
    slug: String,
}

#[derive(Deserialize)]
struct PsCategory {
    id: u64,
    name: String,
    slug: String,
}
