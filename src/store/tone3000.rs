// Tone3000.com backend for NAM models and impulse responses.

use std::path::PathBuf;
use std::sync::Arc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{StoreCategory, StoreFile, StoreItem, StoreQuery, StoreSearchResult};

const BASE_URL: &str = "https://www.tone3000.com/api/v1";

/// Gear type filters available on Tone3000.
const GEAR_TYPES: &[(&str, &str)] = &[
    ("amp", "Amp Heads"),
    ("full-rig", "Full Rig / Combo"),
    ("pedal", "Pedals"),
    ("outboard", "Outboard"),
    ("ir", "Impulse Responses"),
];

/// Persisted token data.
#[derive(Serialize, Deserialize, Clone, Default)]
struct TokenData {
    access_token: String,
    refresh_token: String,
    /// Unix timestamp when the access token expires.
    expires_at: u64,
}

pub struct Tone3000Backend {
    client: Client,
    tokens: Arc<RwLock<Option<TokenData>>>,
    token_file: PathBuf,
}

impl Tone3000Backend {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let token_file = data_dir.join("tone3000-tokens.json");
        let tokens = Self::load_tokens(&token_file);

        Self {
            client: Client::builder()
                .user_agent("rustyfoot/0.1")
                .build()
                .expect("failed to create HTTP client"),
            tokens: Arc::new(RwLock::new(tokens)),
            token_file,
        }
    }

    fn load_tokens(path: &std::path::Path) -> Option<TokenData> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn save_tokens(path: &std::path::Path, tokens: &TokenData) {
        if let Ok(json) = serde_json::to_string_pretty(tokens) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Check if we have a valid (non-expired) access token.
    pub async fn is_authenticated(&self) -> bool {
        let tokens = self.tokens.read().await;
        match tokens.as_ref() {
            Some(t) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                !t.access_token.is_empty() && now < t.expires_at
            }
            None => false,
        }
    }

    /// Get the current access token, refreshing if expired.
    async fn get_access_token(&self) -> Result<String, String> {
        // Check if current token is still valid
        {
            let tokens = self.tokens.read().await;
            if let Some(t) = tokens.as_ref() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if now < t.expires_at {
                    return Ok(t.access_token.clone());
                }
                // Token expired, try to refresh
                if !t.refresh_token.is_empty() {
                    let refresh = t.refresh_token.clone();
                    let access = t.access_token.clone();
                    drop(tokens);
                    return self.refresh_session(&access, &refresh).await;
                }
            }
        }
        Err("Not authenticated with Tone3000. Please connect your account.".to_string())
    }

    /// Exchange an API key (from OAuth callback) for session tokens.
    pub async fn create_session(&self, api_key: &str) -> Result<(), String> {
        let resp = self.client.post(&format!("{}/auth/session", BASE_URL))
            .json(&serde_json::json!({ "api_key": api_key }))
            .send()
            .await
            .map_err(|e| format!("tone3000 auth request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("tone3000 auth failed ({}): {}", status, body));
        }

        let session: SessionResponse = resp.json().await
            .map_err(|e| format!("tone3000 auth parse failed: {}", e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let token_data = TokenData {
            access_token: session.access_token,
            refresh_token: session.refresh_token,
            expires_at: now + session.expires_in.saturating_sub(60), // refresh 60s early
        };

        Self::save_tokens(&self.token_file, &token_data);
        *self.tokens.write().await = Some(token_data);

        Ok(())
    }

    /// Refresh an expired session.
    async fn refresh_session(&self, access_token: &str, refresh_token: &str) -> Result<String, String> {
        let resp = self.client.post(&format!("{}/auth/session/refresh", BASE_URL))
            .json(&serde_json::json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
            }))
            .send()
            .await
            .map_err(|e| format!("tone3000 refresh failed: {}", e))?;

        if !resp.status().is_success() {
            // Refresh failed, clear tokens
            *self.tokens.write().await = None;
            let _ = std::fs::remove_file(&self.token_file);
            return Err("Tone3000 session expired. Please reconnect your account.".to_string());
        }

        let session: SessionResponse = resp.json().await
            .map_err(|e| format!("tone3000 refresh parse failed: {}", e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let new_token = session.access_token.clone();
        let token_data = TokenData {
            access_token: session.access_token,
            refresh_token: session.refresh_token,
            expires_at: now + session.expires_in.saturating_sub(60),
        };

        Self::save_tokens(&self.token_file, &token_data);
        *self.tokens.write().await = Some(token_data);

        Ok(new_token)
    }

    /// Disconnect / clear stored tokens.
    pub async fn disconnect(&self) {
        *self.tokens.write().await = None;
        let _ = std::fs::remove_file(&self.token_file);
    }

    pub async fn search(&self, query: &StoreQuery) -> Result<StoreSearchResult, String> {
        let token = self.get_access_token().await?;
        let page = query.page.unwrap_or(1);
        let per_page = query.per_page.unwrap_or(24).min(25); // Tone3000 max is 25

        let mut request = self.client.get(&format!("{}/tones/search", BASE_URL))
            .bearer_auth(&token)
            .query(&[
                ("page", page.to_string()),
                ("page_size", per_page.to_string()),
                ("sort", "downloads-all-time".to_string()),
            ]);

        if let Some(ref q) = query.q {
            if !q.is_empty() {
                request = request.query(&[("query", q.as_str())]);
            }
        }

        if let Some(ref cat) = query.category {
            if !cat.is_empty() {
                // Category is the gear type slug (amp, pedal, ir, etc.)
                request = request.query(&[("gear", cat.as_str())]);
            }
        }

        let resp = request.send().await
            .map_err(|e| format!("tone3000 search failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("tone3000 search returned {}: {}", status, body));
        }

        let result: PaginatedResponse<Tone> = resp.json().await
            .map_err(|e| format!("tone3000 search parse failed: {}", e))?;

        let items = result.data.into_iter().map(|t| t.into_store_item()).collect();

        Ok(StoreSearchResult {
            items,
            page: result.page,
            total: result.total,
            total_pages: result.total_pages,
        })
    }

    pub async fn get(&self, id: u64) -> Result<StoreItem, String> {
        let token = self.get_access_token().await?;

        // Fetch the models for this tone directly (no "get by ID" endpoint)
        let models_resp = self.client.get(&format!("{}/models", BASE_URL))
            .bearer_auth(&token)
            .query(&[
                ("tone_id", &id.to_string()),
                ("page_size", &"100".to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("tone3000 models request failed: {}", e))?;

        if !models_resp.status().is_success() {
            return Err(format!("tone3000 models returned {}", models_resp.status()));
        }

        let models: PaginatedResponse<Model> = models_resp.json().await
            .map_err(|e| format!("tone3000 models parse failed: {}", e))?;

        if models.data.is_empty() {
            return Err(format!("No downloadable models found for tone {}", id));
        }

        let files = models.data.into_iter().map(|m| StoreFile {
            id: m.id,
            filename: m.name.clone(),
            filesize: 0,
            target: m.size,
            url: m.model_url,
        }).collect();

        Ok(StoreItem {
            id,
            title: String::new(),
            description: String::new(),
            author: String::new(),
            categories: vec![],
            tags: vec![],
            thumbnail_url: None,
            url: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            download_count: 0,
            license: None,
            files,
        })
    }

    pub async fn categories(&self) -> Result<Vec<StoreCategory>, String> {
        Ok(GEAR_TYPES.iter().map(|(slug, name)| StoreCategory {
            id: slug.to_string(),
            name: name.to_string(),
            slug: slug.to_string(),
        }).collect())
    }

    /// Download a model file by its URL, with retry on rate limiting.
    pub async fn download_model(&self, model_url: &str) -> Result<Vec<u8>, String> {
        let token = self.get_access_token().await?;

        for attempt in 0..4 {
            let resp = self.client.get(model_url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| format!("tone3000 download failed: {}", e))?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait = match attempt {
                    0 => 2,
                    1 => 5,
                    2 => 10,
                    _ => 20,
                };
                tracing::warn!("[store] tone3000 rate limited, waiting {}s (attempt {})", wait, attempt + 1);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                continue;
            }

            if !resp.status().is_success() {
                return Err(format!("tone3000 download returned {}", resp.status()));
            }

            return resp.bytes().await
                .map(|b| b.to_vec())
                .map_err(|e| format!("tone3000 download read failed: {}", e));
        }

        Err("tone3000 download failed: rate limited after multiple retries".to_string())
    }
}

// Tone3000 API response types

#[derive(Deserialize)]
struct SessionResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct PaginatedResponse<T> {
    data: Vec<T>,
    page: u32,
    #[serde(default)]
    total: u64,
    #[serde(default = "default_total_pages")]
    total_pages: u32,
}

fn default_total_pages() -> u32 { 1 }

#[derive(Deserialize)]
#[allow(dead_code)]
struct Tone {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    gear: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    downloads_count: u64,
    #[serde(default)]
    favorites_count: u64,
    #[serde(default)]
    models_count: u64,
    #[serde(default)]
    user: Option<ToneUser>,
    #[serde(default)]
    makes: Vec<ToneMake>,
    #[serde(default)]
    tags: Vec<ToneTag>,
    #[serde(default)]
    sizes: Vec<String>,
}

#[derive(Deserialize)]
struct ToneUser {
    #[serde(default)]
    username: String,
}

#[derive(Deserialize)]
struct ToneMake {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct ToneTag {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct Model {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    model_url: String,
}

impl Tone {
    fn into_store_item(self) -> StoreItem {
        let author = self.user.as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_default();

        let mut categories = vec![];
        if !self.gear.is_empty() {
            // Map gear slug to display name
            let gear_name = GEAR_TYPES.iter()
                .find(|(slug, _)| *slug == self.gear)
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| self.gear.clone());
            categories.push(gear_name);
        }
        if !self.platform.is_empty() {
            categories.push(self.platform.clone());
        }

        let makes: Vec<String> = self.makes.iter().map(|m| m.name.clone()).collect();
        let mut tags: Vec<String> = self.tags.iter().map(|t| t.name.clone()).collect();
        tags.extend(makes);

        StoreItem {
            id: self.id,
            title: self.title,
            description: self.description,
            author,
            categories,
            tags,
            thumbnail_url: None,
            url: self.url,
            created_at: self.created_at,
            updated_at: String::new(),
            download_count: self.downloads_count,
            license: if self.license.is_empty() { None } else { Some(self.license) },
            files: vec![], // populated in get() when fetching models
        }
    }
}
