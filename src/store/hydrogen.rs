// Hydrogen drumkit store backend.
// Fetches the drumkit list from hydrogen-music.org and downloads .h2drumkit files.

use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{StoreCategory, StoreFile, StoreItem, StoreQuery, StoreSearchResult};

const FEED_URL: &str = "http://www.hydrogen-music.org/feeds/drumkit_list.php";

/// A parsed drumkit entry from the XML feed.
#[derive(Clone)]
struct Drumkit {
    name: String,
    url: String,
    info: String,
    author: String,
    license: String,
}

pub struct HydrogenBackend {
    client: Client,
    /// Cached drumkit list (fetched on first search).
    cache: Arc<RwLock<Option<Vec<Drumkit>>>>,
}

impl HydrogenBackend {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("rustyfoot/0.1")
                .build()
                .expect("failed to create HTTP client"),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Fetch and parse the drumkit list XML, caching the result.
    async fn get_drumkits(&self) -> Result<Vec<Drumkit>, String> {
        // Return cached list if available
        {
            let cache = self.cache.read().await;
            if let Some(ref kits) = *cache {
                return Ok(kits.clone());
            }
        }

        let resp = self.client.get(FEED_URL)
            .send()
            .await
            .map_err(|e| format!("failed to fetch drumkit list: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("drumkit list returned {}", resp.status()));
        }

        let xml = resp.text().await
            .map_err(|e| format!("failed to read drumkit list: {}", e))?;

        let kits = parse_drumkit_xml(&xml)?;
        tracing::info!("[store] loaded {} hydrogen drumkits from feed", kits.len());

        *self.cache.write().await = Some(kits.clone());
        Ok(kits)
    }

    pub async fn search(&self, query: &StoreQuery) -> Result<StoreSearchResult, String> {
        let all_kits = self.get_drumkits().await?;

        // Filter by search query
        let filtered: Vec<&Drumkit> = if let Some(ref q) = query.q {
            let q_lower = q.to_lowercase();
            if q_lower.is_empty() {
                all_kits.iter().collect()
            } else {
                all_kits.iter().filter(|k| {
                    k.name.to_lowercase().contains(&q_lower)
                        || k.author.to_lowercase().contains(&q_lower)
                        || k.info.to_lowercase().contains(&q_lower)
                }).collect()
            }
        } else {
            all_kits.iter().collect()
        };

        let total = filtered.len() as u64;
        let per_page = query.per_page.unwrap_or(24) as usize;
        let page = query.page.unwrap_or(1).max(1) as usize;
        let total_pages = ((total as usize + per_page - 1) / per_page).max(1) as u32;

        let start = (page - 1) * per_page;
        let items: Vec<StoreItem> = filtered.iter()
            .skip(start)
            .take(per_page)
            .enumerate()
            .map(|(i, k)| k.to_store_item((start + i) as u64))
            .collect();

        Ok(StoreSearchResult {
            items,
            page: page as u32,
            total,
            total_pages,
        })
    }

    pub async fn get(&self, id: u64) -> Result<StoreItem, String> {
        let kits = self.get_drumkits().await?;
        let kit = kits.get(id as usize)
            .ok_or_else(|| format!("drumkit {} not found", id))?;

        let filename = kit.url.rsplit('/').next().unwrap_or("drumkit.h2drumkit").to_string();
        let mut item = kit.to_store_item(id);
        item.files = vec![StoreFile {
            id: 0,
            filename,
            filesize: 0,
            target: None,
            url: kit.url.clone(),
        }];
        Ok(item)
    }

    pub async fn categories(&self) -> Result<Vec<StoreCategory>, String> {
        // Hydrogen drumkits don't have categories
        Ok(vec![])
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

impl Drumkit {
    fn to_store_item(&self, id: u64) -> StoreItem {
        // Strip HTML from info field
        let description = strip_html(&self.info);

        StoreItem {
            id,
            title: self.name.clone(),
            description,
            author: self.author.clone(),
            categories: vec!["Drumkit".to_string()],
            tags: vec![],
            thumbnail_url: None,
            url: self.url.clone(),
            created_at: String::new(),
            updated_at: String::new(),
            download_count: 0,
            license: if self.license.is_empty() { None } else { Some(self.license.clone()) },
            files: vec![],
        }
    }
}

/// Parse the hydrogen drumkit list XML.
fn parse_drumkit_xml(xml: &str) -> Result<Vec<Drumkit>, String> {
    let mut kits = Vec::new();
    let mut pos = 0;

    while let Some(start) = xml[pos..].find("<drumkit>") {
        let start = pos + start;
        let end = match xml[start..].find("</drumkit>") {
            Some(e) => start + e + "</drumkit>".len(),
            None => break,
        };

        let entry = &xml[start..end];
        let name = extract_xml_tag(entry, "name");
        let url = extract_xml_tag(entry, "url");

        if !name.is_empty() && !url.is_empty() {
            kits.push(Drumkit {
                name,
                url,
                info: extract_xml_tag(entry, "info"),
                author: extract_xml_tag(entry, "author"),
                license: extract_xml_tag(entry, "license"),
            });
        }

        pos = end;
    }

    Ok(kits)
}

/// Extract text content from a simple XML tag.
fn extract_xml_tag(xml: &str, tag: &str) -> String {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = xml.find(&open) {
        let start = start + open.len();
        if let Some(end) = xml[start..].find(&close) {
            return xml[start..start + end].trim().to_string();
        }
    }
    String::new()
}

/// Strip HTML tags and decode common entities.
fn strip_html(s: &str) -> String {
    // Remove HTML tags
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // Decode common XML/HTML entities
    result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // Collapse whitespace
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
