// JSFX store backend.
// Fetches ReaPack index.xml files from configured repositories,
// parses them, and provides one-click JSFX-to-LV2 installation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{StoreCategory, StoreFile, StoreItem, StoreQuery, StoreSearchResult};

const DEFAULT_REPO_URL: &str =
    "https://raw.githubusercontent.com/ReaTeam/JSFX/refs/heads/master/index.xml";
const JSFX_SCANNER: &str = "/usr/bin/rustyfoot-jsfx-scan";
const JSFX_WRAPPER_SO: &str = "/usr/lib/rustyfoot/jsfx-wrapper.so";

#[derive(Serialize, Deserialize, Clone)]
pub struct JsfxRepoConfig {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct JsfxReposFile {
    repos: Vec<JsfxRepoConfig>,
}

#[derive(Clone)]
struct JsfxSource {
    url: String,
    file: Option<String>,
}

#[derive(Clone)]
struct JsfxEntry {
    repo_index: usize,
    entry_index: usize,
    category: String,
    reapack_name: String,
    desc: String,
    author: String,
    version_name: String,
    time: String,
    changelog: String,
    screenshot_url: Option<String>,
    sources: Vec<JsfxSource>,
}

pub struct JsfxBackend {
    client: Client,
    data_dir: PathBuf,
    pub jsfx_dir: PathBuf,
    pub lv2_plugin_dir: PathBuf,
    repos: Arc<RwLock<Vec<JsfxRepoConfig>>>,
    cache: Arc<RwLock<Option<Vec<JsfxEntry>>>>,
}

impl JsfxBackend {
    pub fn new(data_dir: &Path, lv2_plugin_dir: &Path) -> Self {
        let repos_file = data_dir.join("jsfx_repos.json");
        let repos = if repos_file.exists() {
            match std::fs::read_to_string(&repos_file) {
                Ok(json) => serde_json::from_str::<JsfxReposFile>(&json)
                    .map(|f| f.repos)
                    .unwrap_or_else(|_| default_repos()),
                Err(_) => default_repos(),
            }
        } else {
            let repos = default_repos();
            save_repos(&repos_file, &repos);
            repos
        };

        let jsfx_dir = PathBuf::from("/var/lib/rustyfoot/jsfx");
        let _ = std::fs::create_dir_all(&jsfx_dir);

        Self {
            client: Client::builder()
                .user_agent("rustyfoot/0.1")
                .build()
                .expect("failed to create HTTP client"),
            data_dir: data_dir.to_path_buf(),
            jsfx_dir,
            lv2_plugin_dir: lv2_plugin_dir.to_path_buf(),
            repos: Arc::new(RwLock::new(repos)),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    // --- Repo management ---

    pub async fn list_repos(&self) -> Vec<JsfxRepoConfig> {
        self.repos.read().await.clone()
    }

    pub async fn add_repo(&self, name: &str, url: &str) -> Result<(), String> {
        let mut repos = self.repos.write().await;
        if repos.iter().any(|r| r.url == url) {
            return Err("Repository URL already exists".into());
        }
        repos.push(JsfxRepoConfig {
            name: name.to_string(),
            url: url.to_string(),
            enabled: true,
        });
        save_repos(&self.data_dir.join("jsfx_repos.json"), &repos);
        drop(repos);
        *self.cache.write().await = None;
        Ok(())
    }

    pub async fn toggle_repo(&self, index: usize) -> Result<(), String> {
        let mut repos = self.repos.write().await;
        let repo = repos.get_mut(index).ok_or("Invalid repo index")?;
        repo.enabled = !repo.enabled;
        save_repos(&self.data_dir.join("jsfx_repos.json"), &repos);
        drop(repos);
        *self.cache.write().await = None;
        Ok(())
    }

    pub async fn remove_repo(&self, index: usize) -> Result<(), String> {
        let mut repos = self.repos.write().await;
        if index >= repos.len() {
            return Err("Invalid repo index".into());
        }
        repos.remove(index);
        save_repos(&self.data_dir.join("jsfx_repos.json"), &repos);
        drop(repos);
        *self.cache.write().await = None;
        Ok(())
    }

    pub async fn refresh_cache(&self) -> Result<(), String> {
        *self.cache.write().await = None;
        self.ensure_cache().await?;
        Ok(())
    }

    // --- Index fetching ---

    async fn ensure_cache(&self) -> Result<(), String> {
        {
            let cache = self.cache.read().await;
            if cache.is_some() {
                return Ok(());
            }
        }

        let repos = self.repos.read().await.clone();
        let mut all_entries = Vec::new();

        for (repo_idx, repo) in repos.iter().enumerate() {
            if !repo.enabled {
                continue;
            }
            match self.fetch_index(&repo.url, repo_idx).await {
                Ok(entries) => {
                    tracing::info!("[store] loaded {} JSFX from {}", entries.len(), repo.name);
                    all_entries.extend(entries);
                }
                Err(e) => {
                    tracing::warn!("[store] failed to fetch JSFX index {}: {}", repo.name, e);
                }
            }
        }

        *self.cache.write().await = Some(all_entries);
        Ok(())
    }

    async fn fetch_index(&self, url: &str, repo_index: usize) -> Result<Vec<JsfxEntry>, String> {
        let resp = self.client.get(url)
            .send()
            .await
            .map_err(|e| format!("failed to fetch index: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("index returned {}", resp.status()));
        }

        let xml = resp.text().await
            .map_err(|e| format!("failed to read index: {}", e))?;

        parse_reapack_index(&xml, repo_index)
    }

    // --- Store interface ---

    pub async fn search(&self, query: &StoreQuery) -> Result<StoreSearchResult, String> {
        self.ensure_cache().await?;
        let cache = self.cache.read().await;
        let entries = cache.as_ref().unwrap();

        let filtered: Vec<&JsfxEntry> = entries.iter().filter(|e| {
            if let Some(ref q) = query.q {
                let q = q.to_lowercase();
                if !q.is_empty()
                    && !e.desc.to_lowercase().contains(&q)
                    && !e.author.to_lowercase().contains(&q)
                    && !e.reapack_name.to_lowercase().contains(&q)
                    && !e.category.to_lowercase().contains(&q)
                {
                    return false;
                }
            }
            if let Some(ref cat) = query.category {
                if !cat.is_empty() && !cat.eq_ignore_ascii_case(&e.category) {
                    return false;
                }
            }
            true
        }).collect();

        let total = filtered.len() as u64;
        let per_page = query.per_page.unwrap_or(24) as usize;
        let page = query.page.unwrap_or(1).max(1) as usize;
        let total_pages = ((total as usize + per_page - 1) / per_page).max(1) as u32;

        let start = (page - 1) * per_page;
        let items: Vec<StoreItem> = filtered.iter()
            .skip(start)
            .take(per_page)
            .map(|e| e.to_store_item())
            .collect();

        Ok(StoreSearchResult { items, page: page as u32, total, total_pages })
    }

    pub async fn get(&self, id: u64) -> Result<StoreItem, String> {
        self.ensure_cache().await?;
        let cache = self.cache.read().await;
        let entries = cache.as_ref().unwrap();

        let entry = entries.iter()
            .find(|e| e.stable_id() == id)
            .ok_or_else(|| format!("JSFX {} not found", id))?;

        let mut item = entry.to_store_item();
        item.files = entry.sources.iter().enumerate().map(|(i, s)| {
            let filename = s.file.as_deref()
                .unwrap_or(&entry.reapack_name)
                .to_string();
            StoreFile {
                id: i as u64,
                filename,
                filesize: 0,
                target: s.file.clone(),
                url: s.url.clone(),
            }
        }).collect();

        Ok(item)
    }

    pub async fn categories(&self) -> Result<Vec<StoreCategory>, String> {
        self.ensure_cache().await?;
        let cache = self.cache.read().await;
        let entries = cache.as_ref().unwrap();

        let mut seen = std::collections::BTreeSet::new();
        for e in entries {
            if !e.category.is_empty() {
                seen.insert(e.category.clone());
            }
        }

        Ok(seen.into_iter().map(|name| {
            let slug = name.to_lowercase().replace(' ', "-");
            StoreCategory { id: slug.clone(), name, slug }
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

impl JsfxEntry {
    fn stable_id(&self) -> u64 {
        (self.repo_index as u64) * 100_000 + self.entry_index as u64
    }

    fn to_store_item(&self) -> StoreItem {
        StoreItem {
            id: self.stable_id(),
            title: self.desc.clone(),
            description: self.changelog.clone(),
            author: self.author.clone(),
            categories: vec![self.category.clone()],
            tags: vec![self.reapack_name.clone()],
            thumbnail_url: self.screenshot_url.clone(),
            url: self.sources.iter()
                .find(|s| s.file.is_none())
                .map(|s| s.url.clone())
                .unwrap_or_default(),
            created_at: self.time.clone(),
            updated_at: self.time.clone(),
            download_count: 0,
            license: None,
            files: vec![],
        }
    }
}

// --- ReaPack XML parsing ---

fn parse_reapack_index(xml: &str, repo_index: usize) -> Result<Vec<JsfxEntry>, String> {
    let mut entries = Vec::new();
    let mut entry_index: usize = 0;
    let mut cat_pos = 0;

    while let Some(cat_start) = xml[cat_pos..].find("<category ") {
        let cat_start = cat_pos + cat_start;
        let cat_end = match xml[cat_start..].find("</category>") {
            Some(e) => cat_start + e + "</category>".len(),
            None => break,
        };

        let cat_tag_end = xml[cat_start..].find('>').map(|i| cat_start + i).unwrap_or(cat_end);
        let category = extract_attr(&xml[cat_start..cat_tag_end], "name");
        let cat_body = &xml[cat_tag_end + 1..cat_end];

        let mut rp_pos = 0;
        while let Some(rp_start) = cat_body[rp_pos..].find("<reapack ") {
            let rp_start = rp_pos + rp_start;
            let rp_end = match cat_body[rp_start..].find("</reapack>") {
                Some(e) => rp_start + e + "</reapack>".len(),
                None => break,
            };

            let rp_tag_end = cat_body[rp_start..].find('>').map(|i| rp_start + i).unwrap_or(rp_end);
            let rp_attrs = &cat_body[rp_start..rp_tag_end];
            let rp_type = extract_attr(rp_attrs, "type");

            if rp_type == "effect" {
                let reapack_name = extract_attr(rp_attrs, "name");
                let desc = extract_attr(rp_attrs, "desc");
                let rp_body = &cat_body[rp_tag_end + 1..rp_end];

                // Use the last <version> (most recent)
                if let Some(ver) = parse_last_version(rp_body) {
                    let screenshot_url = extract_link_href(rp_body, "screenshot");

                    entries.push(JsfxEntry {
                        repo_index,
                        entry_index,
                        category: category.clone(),
                        reapack_name,
                        desc,
                        author: ver.author,
                        version_name: ver.name,
                        time: ver.time,
                        changelog: ver.changelog,
                        screenshot_url,
                        sources: ver.sources,
                    });
                    entry_index += 1;
                }
            }

            rp_pos = rp_end;
        }

        cat_pos = cat_end;
    }

    Ok(entries)
}

struct ParsedVersion {
    name: String,
    author: String,
    time: String,
    changelog: String,
    sources: Vec<JsfxSource>,
}

fn parse_last_version(body: &str) -> Option<ParsedVersion> {
    let mut last: Option<ParsedVersion> = None;
    let mut pos = 0;

    while let Some(ver_start) = body[pos..].find("<version ") {
        let ver_start = pos + ver_start;
        let ver_end = match body[ver_start..].find("</version>") {
            Some(e) => ver_start + e + "</version>".len(),
            None => break,
        };

        let ver_tag_end = body[ver_start..].find('>').map(|i| ver_start + i).unwrap_or(ver_end);
        let ver_attrs = &body[ver_start..ver_tag_end];
        let ver_body = &body[ver_tag_end + 1..ver_end];

        let mut sources = Vec::new();
        let mut src_pos = 0;
        while let Some(src_start) = ver_body[src_pos..].find("<source") {
            let src_start = src_pos + src_start;
            let src_end = match ver_body[src_start..].find("</source>") {
                Some(e) => src_start + e + "</source>".len(),
                None => break,
            };

            let src_tag_end = ver_body[src_start..].find('>').map(|i| src_start + i).unwrap_or(src_end);
            let src_attrs = &ver_body[src_start..src_tag_end];
            let file_attr = extract_attr(src_attrs, "file");
            let url = ver_body[src_tag_end + 1..src_end - "</source>".len()].trim().to_string();

            if !url.is_empty() {
                sources.push(JsfxSource {
                    url,
                    file: if file_attr.is_empty() { None } else { Some(file_attr) },
                });
            }

            src_pos = src_end;
        }

        let changelog = extract_cdata_tag(ver_body, "changelog");

        last = Some(ParsedVersion {
            name: extract_attr(ver_attrs, "name"),
            author: extract_attr(ver_attrs, "author"),
            time: extract_attr(ver_attrs, "time"),
            changelog,
            sources,
        });

        pos = ver_end;
    }

    last
}

fn extract_attr(tag: &str, attr: &str) -> String {
    let search = format!("{}=\"", attr);
    if let Some(start) = tag.find(&search) {
        let start = start + search.len();
        if let Some(end) = tag[start..].find('"') {
            return decode_xml_entities(&tag[start..start + end]);
        }
    }
    String::new()
}

fn extract_link_href(body: &str, rel: &str) -> Option<String> {
    let search = format!("rel=\"{}\"", rel);
    let mut pos = 0;
    while let Some(link_start) = body[pos..].find("<link ") {
        let link_start = pos + link_start;
        let link_end = match body[link_start..].find('>') {
            Some(e) => link_start + e,
            None => break,
        };
        let tag = &body[link_start..link_end];
        if tag.contains(&search) {
            let href = extract_attr(tag, "href");
            if !href.is_empty() {
                return Some(href);
            }
        }
        pos = link_end;
    }
    None
}

fn extract_cdata_tag(body: &str, tag: &str) -> String {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = body.find(&open) {
        let start = start + open.len();
        if let Some(end) = body[start..].find(&close) {
            let content = &body[start..start + end];
            let content = content.strip_prefix("<![CDATA[").unwrap_or(content);
            let content = content.strip_suffix("]]>").unwrap_or(content);
            return content.trim().to_string();
        }
    }
    String::new()
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// --- TTL generation ---

#[derive(Deserialize)]
pub struct WrapperJson {
    pub jsfx_file: String,
    pub uri: String,
    pub name: String,
    pub maker: Option<String>,
    pub category: String,
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    pub parameters: Vec<WrapperParam>,
}

#[derive(Deserialize)]
pub struct WrapperParam {
    pub lv2_index: u32,
    pub lv2_symbol: String,
    pub lv2_name: String,
    pub slider_index: u32,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    #[serde(default)]
    pub toggle: bool,
    #[serde(default)]
    pub integer: bool,
    #[serde(default)]
    pub enum_labels: Option<Vec<String>>,
}

pub fn generate_manifest_ttl(uri: &str) -> String {
    format!(
        r#"@prefix lv2:  <http://lv2plug.in/ns/lv2core#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<{uri}>
    a lv2:Plugin ;
    lv2:binary <jsfx-wrapper.so> ;
    rdfs:seeAlso <plugin.ttl> .
"#
    )
}

pub fn generate_plugin_ttl(w: &WrapperJson) -> String {
    let mut ttl = String::new();

    ttl.push_str(
        "@prefix doap:  <http://usefulinc.com/ns/doap#> .\n\
         @prefix foaf:  <http://xmlns.com/foaf/0.1/> .\n\
         @prefix lv2:   <http://lv2plug.in/ns/lv2core#> .\n\
         @prefix pprop: <http://lv2plug.in/ns/ext/port-props#> .\n\
         @prefix rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
         @prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> .\n\
         @prefix state: <http://lv2plug.in/ns/ext/state#> .\n\
         @prefix urid:  <http://lv2plug.in/ns/ext/urid#> .\n\n",
    );

    let lv2_class = lv2_class_for_category(&w.category);

    ttl.push_str(&format!("<{}>\n", w.uri));
    ttl.push_str(&format!("    a lv2:Plugin, {} ;\n", lv2_class));
    ttl.push_str(&format!("    doap:name \"{}\" ;\n", ttl_escape(&w.name)));
    if let Some(ref maker) = w.maker {
        if !maker.is_empty() {
            ttl.push_str(&format!("    doap:maintainer [ foaf:name \"{}\" ] ;\n", ttl_escape(maker)));
        }
    }
    ttl.push_str("    lv2:requiredFeature urid:map ;\n");
    ttl.push_str("    lv2:extensionData state:interface ;\n\n");

    let num_params = w.parameters.len() as u32;
    let total_ports = num_params + w.audio_inputs + w.audio_outputs;
    let mut port_idx = 0u32;

    for (i, p) in w.parameters.iter().enumerate() {
        let sep = if port_idx == 0 { "    lv2:port [\n" } else { "    ] , [\n" };
        ttl.push_str(sep);
        ttl.push_str("        a lv2:InputPort, lv2:ControlPort ;\n");
        ttl.push_str(&format!("        lv2:index {} ;\n", port_idx));
        ttl.push_str(&format!("        lv2:symbol \"{}\" ;\n", p.lv2_symbol));
        ttl.push_str(&format!("        lv2:name \"{}\" ;\n", ttl_escape(&p.lv2_name)));
        ttl.push_str(&format!("        lv2:default {} ;\n", format_float(p.default)));
        ttl.push_str(&format!("        lv2:minimum {} ;\n", format_float(p.min)));
        ttl.push_str(&format!("        lv2:maximum {}", format_float(p.max)));

        if p.toggle {
            ttl.push_str(" ;\n        lv2:portProperty lv2:toggled");
        }
        if p.integer {
            ttl.push_str(" ;\n        lv2:portProperty lv2:integer");
        }

        if let Some(ref labels) = p.enum_labels {
            for (ei, label) in labels.iter().enumerate() {
                ttl.push_str(&format!(
                    " ;\n        lv2:scalePoint [ rdfs:label \"{}\" ; rdf:value {} ]",
                    ttl_escape(label),
                    ei
                ));
            }
        }

        ttl.push('\n');
        port_idx += 1;

        if i == w.parameters.len() - 1 && w.audio_inputs == 0 && w.audio_outputs == 0 {
            ttl.push_str("    ] .\n");
        }
    }

    for i in 0..w.audio_inputs {
        let sep = if port_idx == 0 { "    lv2:port [\n" } else { "    ] , [\n" };
        ttl.push_str(sep);
        ttl.push_str("        a lv2:InputPort, lv2:AudioPort ;\n");
        ttl.push_str(&format!("        lv2:index {} ;\n", port_idx));
        ttl.push_str(&format!("        lv2:symbol \"audio_in_{}\" ;\n", i + 1));
        ttl.push_str(&format!("        lv2:name \"Audio Input {}\" ;\n", i + 1));
        port_idx += 1;
        if i == w.audio_inputs - 1 && w.audio_outputs == 0 {
            ttl.push_str("    ] .\n");
        } else {
            ttl.push('\n');
        }
    }

    for i in 0..w.audio_outputs {
        let sep = if port_idx == 0 { "    lv2:port [\n" } else { "    ] , [\n" };
        ttl.push_str(sep);
        ttl.push_str("        a lv2:OutputPort, lv2:AudioPort ;\n");
        ttl.push_str(&format!("        lv2:index {} ;\n", port_idx));
        ttl.push_str(&format!("        lv2:symbol \"audio_out_{}\" ;\n", i + 1));
        ttl.push_str(&format!("        lv2:name \"Audio Output {}\" ;\n", i + 1));
        port_idx += 1;
        if i == w.audio_outputs - 1 {
            ttl.push_str("    ] .\n");
        } else {
            ttl.push('\n');
        }
    }

    if total_ports == 0 {
        ttl.push_str("    .\n");
    }

    ttl
}

fn lv2_class_for_category(category: &str) -> &'static str {
    match category {
        "lv2:DelayPlugin" => "lv2:DelayPlugin",
        "lv2:ReverbPlugin" => "lv2:ReverbPlugin",
        "lv2:FilterPlugin" => "lv2:FilterPlugin",
        "lv2:EQPlugin" => "lv2:EQPlugin",
        "lv2:DistortionPlugin" => "lv2:DistortionPlugin",
        "lv2:DynamicsPlugin" => "lv2:DynamicsPlugin",
        "lv2:ModulatorPlugin" => "lv2:ModulatorPlugin",
        "lv2:InstrumentPlugin" => "lv2:InstrumentPlugin",
        "lv2:UtilityPlugin" => "lv2:UtilityPlugin",
        "lv2:SimulatorPlugin" => "lv2:SimulatorPlugin",
        "lv2:SpatialPlugin" => "lv2:SpatialPlugin",
        _ => "lv2:Plugin",
    }
}

fn ttl_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_float(v: f64) -> String {
    if v == v.floor() && v.abs() < 1e15 {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

// --- Installation helpers ---

pub fn scanner_path() -> &'static str {
    JSFX_SCANNER
}

pub fn wrapper_so_path() -> &'static str {
    JSFX_WRAPPER_SO
}

pub fn bundle_name_for_uri(uri: &str) -> String {
    let slug = uri.strip_prefix("urn:rustyfoot:jsfx:").unwrap_or(uri);
    let safe: String = slug.chars().map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }
    }).collect();
    format!("jsfx-{}.lv2", safe)
}

// --- Helpers ---

fn default_repos() -> Vec<JsfxRepoConfig> {
    vec![JsfxRepoConfig {
        name: "ReaTeam JSFX".into(),
        url: DEFAULT_REPO_URL.into(),
        enabled: true,
    }]
}

fn save_repos(path: &Path, repos: &[JsfxRepoConfig]) {
    let file = JsfxReposFile { repos: repos.to_vec() };
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, json);
    }
}
