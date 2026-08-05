//! RomM API client.
//!
//! Findings that shape this file are in PLAN.md §3. The two that bite:
//! the list param is `platform_ids` (an array — unknown params are silently
//! ignored, so a typo pages the whole library), and connection reuse is worth
//! ~300x, which `reqwest`'s pool gives us for free as long as one Client is
//! shared.

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::Deserialize;

// Mirrors the server schema rather than only the fields read today.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
}

// Mirrors the server schema rather than only the fields read today.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub fs_slug: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rom_count: i64,
}

// Mirrors the server schema rather than only the fields read today.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Rom {
    pub id: i64,
    /// Display title, e.g. `"'88 Games"`. Distinct from `fs_name`.
    #[serde(default)]
    pub name: Option<String>,
    pub fs_name: String,
    #[serde(default)]
    pub fs_size_bytes: Option<i64>,
    #[serde(default)]
    pub platform_fs_slug: Option<String>,
    #[serde(default)]
    pub md5_hash: Option<String>,
    #[serde(default)]
    pub sha1_hash: Option<String>,
    #[serde(default)]
    pub crc_hash: Option<String>,
    /// Drives incremental sync via the `updated_after` query param.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Server-side artwork, e.g. `/assets/romm/resources/roms/2/42/cover/big.png?ts=...`.
    /// Note the timestamp query contains a raw space and must be encoded.
    #[serde(default)]
    pub path_cover_large: Option<String>,
    /// Thumbnail variant — averages ~71 KB against ~278 KB for the large one,
    /// which matters a lot when a grid shows hundreds at once.
    #[serde(default)]
    pub path_cover_small: Option<String>,
    #[serde(default)]
    pub merged_screenshots: Vec<String>,

    // Descriptive metadata. `metadatum` is RomM's merged view across whatever
    // sources were enabled; on this server that is the ES-DE gamelist import.
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub metadatum: Option<serde_json::Value>,
    #[serde(default)]
    pub alternative_names: Vec<String>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub path_manual: Option<String>,
    #[serde(default)]
    pub youtube_video_id: Option<String>,
    /// True for folder ROMs (multi-disc games). The content endpoint serves
    /// these as a zip of the folder, not the ROM itself.
    #[serde(default)]
    pub has_multiple_files: bool,
    /// Populated only when the request asks for `with_files=true`.
    #[serde(default)]
    pub files: Vec<RomFile>,
}

/// One file inside a multi-file (folder) ROM.
///
/// Mirrors the server schema rather than only the fields read today.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct RomFile {
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size_bytes: Option<u64>,
    #[serde(default)]
    pub md5_hash: Option<String>,
    #[serde(default)]
    pub sha1_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RomPage {
    #[serde(default)]
    pub items: Vec<Rom>,
    #[serde(default)]
    pub total: i64,
}

/// Server-side settings that change how we must treat its data.
///
/// Fetched rather than assumed: the exclusion lists feed our archive-hash
/// reproduction, and they are configurable per deployment and per RomM
/// version. Hardcoding them means silently computing the wrong hash the day
/// someone edits `config.yml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub default_excluded_files: Vec<String>,
    #[serde(default)]
    pub default_excluded_extensions: Vec<String>,
    /// When true the server stores no hashes at all, so downloads can only be
    /// size-checked. Worth surfacing rather than discovering per-file.
    #[serde(default)]
    pub skip_hash_calculation: bool,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(rename = "DEFAULT_EXCLUDED_FILES", default)]
    files: Vec<String>,
    #[serde(rename = "DEFAULT_EXCLUDED_EXTENSIONS", default)]
    exts: Vec<String>,
    #[serde(rename = "SKIP_HASH_CALCULATION", default)]
    skip_hash: bool,
}

#[derive(Debug, Deserialize)]
pub struct Heartbeat {
    #[serde(rename = "SYSTEM")]
    pub system: HeartbeatSystem,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatSystem {
    #[serde(rename = "VERSION", default)]
    pub version: String,
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    auth: String,
}

impl Client {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self> {
        if base_url.is_empty() {
            bail!("server.url is empty — copy config.example.toml to config.toml");
        }
        let http = crate::util::http_client(None).context("building HTTP client")?;
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("{username}:{password}"));
        Ok(Self {
            http,
            base: base_url.trim_end_matches('/').to_owned(),
            auth,
        })
    }

    /// Shared HTTP client — reuse it so connections stay pooled.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Pre-encoded HTTP Basic credential, for callers issuing their own
    /// requests (streaming downloads).
    pub fn auth(&self) -> &str {
        &self.auth
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let hint = if status == reqwest::StatusCode::FORBIDDEN {
                "\n  403 usually means the token lacks a scope. This client needs \
                 roms.read and platforms.read."
            } else {
                ""
            };
            bail!("GET {url} -> {status}{hint}\n  {}", body.chars().take(300).collect::<String>());
        }
        resp.json::<T>()
            .await
            .with_context(|| format!("decoding response from {url}"))
    }

    pub async fn me(&self) -> Result<User> {
        self.get_json("/api/users/me").await
    }

    pub async fn platforms(&self) -> Result<Vec<Platform>> {
        self.get_json("/api/platforms").await
    }

    /// One page of ROMs. `platform_id` is sent as `platform_ids` — the API
    /// ignores unknown params silently, and `platform_id` is not a real one.
    ///
    /// `updated_after` (ISO-8601) makes this incremental: after a first full
    /// pull, later syncs return only what changed.
    pub async fn roms(
        &self,
        platform_id: Option<i64>,
        limit: u32,
        offset: u32,
        updated_after: Option<&str>,
    ) -> Result<RomPage> {
        let mut path = format!("/api/roms?limit={limit}&offset={offset}");
        if let Some(id) = platform_id {
            path.push_str(&format!("&platform_ids={id}"));
        }
        if let Some(ts) = updated_after {
            path.push_str(&format!("&updated_after={}", urlencode(ts)));
        }
        self.get_json(&path).await
    }

    /// One ROM with its member files.
    ///
    /// Folder ROMs need this: the rom-level hash is a composite computed in
    /// filesystem order on the server and cannot be reproduced elsewhere, but
    /// each member carries its own md5, which verifies precisely.
    pub async fn rom_with_files(&self, id: i64) -> Result<Rom> {
        self.get_json(&format!("/api/roms/{id}?with_files=true")).await
    }

    /// `(file_name, md5)` for each member of a folder ROM.
    ///
    /// Tolerant by design: this only ever *strengthens* verification, so a
    /// server that cannot answer costs us the per-file check rather than the
    /// download. (`/api/roms/{id}/files` returns 500 on 5.0.0; the rom
    /// endpoint with `with_files=true` is the working route.)
    pub async fn member_hashes(&self, id: i64) -> Vec<(String, String)> {
        match self.rom_with_files(id).await {
            Ok(rom) => rom
                .files
                .into_iter()
                .filter_map(|f| Some((f.file_name, f.md5_hash?)))
                .filter(|(n, m)| !n.is_empty() && !m.is_empty())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Every ROM id the server currently has.
    ///
    /// Cheap (one array of ints for ~10k roms) and the only way to notice
    /// deletions: `updated_after` reports changes, never removals.
    pub async fn rom_identifiers(&self) -> Result<Vec<i64>> {
        self.get_json("/api/roms/identifiers").await
    }

    /// Server settings that affect how we interpret its data.
    pub async fn config(&self) -> Result<ServerConfig> {
        let raw: RawConfig = self.get_json("/api/config").await?;
        Ok(ServerConfig {
            default_excluded_files: raw.files,
            default_excluded_extensions: raw.exts,
            skip_hash_calculation: raw.skip_hash,
        })
    }

    /// Unauthenticated; also the cheapest reachability check.
    pub async fn heartbeat(&self) -> Result<Heartbeat> {
        self.get_json("/api/heartbeat").await
    }

    /// Total ROM count, without pulling any rows we don't need.
    pub async fn rom_count(&self) -> Result<i64> {
        Ok(self.roms(None, 1, 0, None).await?.total)
    }
}

/// Minimal percent-encoding for query values (timestamps contain `:` and `+`).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A RomM collection: hand-made, smart (a saved filter), or virtual
/// (auto-grouped by genre, franchise, company…).
///
/// One struct covers all three because the server returns the same shape for
/// each; only `is_virtual`/`is_smart` and the presence of `type` differ.
#[derive(Debug, Clone, Deserialize)]
pub struct Collection {
    /// Hand-made collections have a numeric id, virtual ones a base64 string.
    /// Kept as text so the two can share a table.
    #[serde(deserialize_with = "id_as_string")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rom_ids: Vec<i64>,
    #[serde(default)]
    pub rom_count: i64,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub is_virtual: bool,
    #[serde(default)]
    pub is_smart: bool,
    /// Virtual collections only: `genre`, `franchise`, `company`, …
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// Covers of the first few members, for a mosaic thumbnail.
    #[serde(default)]
    pub path_covers_small: Vec<String>,
}

impl Collection {
    /// Which of the three kinds this is, as a stable string for the cache.
    pub fn group(&self) -> &str {
        match () {
            _ if self.is_virtual => self.kind.as_deref().unwrap_or("virtual"),
            _ if self.is_smart => "smart",
            _ => "user",
        }
    }
}

/// Accepts either a JSON number or string, since the two collection families
/// disagree on the type of `id`.
fn id_as_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    })
}

impl Client {
    /// Hand-made collections.
    pub async fn collections(&self) -> Result<Vec<Collection>> {
        self.get_json("/api/collections").await
    }

    /// Saved-filter collections.
    pub async fn smart_collections(&self) -> Result<Vec<Collection>> {
        self.get_json("/api/collections/smart").await
    }

    /// Auto-grouped collections of one kind.
    pub async fn virtual_collections(&self, kind: &str) -> Result<Vec<Collection>> {
        self.get_json(&format!("/api/collections/virtual?type={kind}")).await
    }

    /// Which virtual kinds this server actually has.
    ///
    /// Read from the identifiers list rather than hard-coded, so a RomM that
    /// grows a new kind appears without a client change. Each identifier is
    /// base64 of `{"name": ..., "type": ...}`.
    pub async fn virtual_kinds(&self) -> Result<Vec<String>> {
        let ids: Vec<String> = self.get_json("/api/collections/virtual/identifiers").await?;
        let mut kinds: Vec<String> = Vec::new();
        for id in ids {
            // Pad: the server emits unpadded base64, and both alphabets occur.
            let padded = format!("{id}{}", "=".repeat((4 - id.len() % 4) % 4));
            let Some(raw) = base64::engine::general_purpose::URL_SAFE
                .decode(&padded)
                .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&padded))
                .ok()
            else {
                continue;
            };
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw)
                && let Some(k) = v.get("type").and_then(|t| t.as_str())
                && !kinds.iter().any(|s| s == k)
            {
                kinds.push(k.to_owned());
            }
        }
        Ok(kinds)
    }

    /// Every collection the server has, of all three families.
    ///
    /// Tolerant per family: a server with smart collections disabled, or a
    /// virtual kind that errors, costs that group rather than the whole sync.
    pub async fn all_collections(&self) -> Result<Vec<Collection>> {
        let mut out = self.collections().await.unwrap_or_default();
        out.extend(self.smart_collections().await.unwrap_or_default());
        for kind in self.virtual_kinds().await.unwrap_or_default() {
            out.extend(self.virtual_collections(&kind).await.unwrap_or_default());
        }
        Ok(out)
    }
}

// --- Saves and sync ---------------------------------------------------------
//
// The protocol, read out of the running server (`/backend/endpoints/saves.py`)
// rather than inferred from the spec, which documents none of it:
//
// * `POST /api/saves` with `overwrite=false` (the default) returns **409** when
//   the server's copy changed since this device last synced. That is real
//   conflict detection, so we always upload with overwrite off and surface the
//   409 rather than clobbering another machine's progress.
// * The server dedupes on `content_hash`: an identical save is discarded and
//   the existing record returned. This is why `savehash.rs` had to reproduce
//   RomM's hash byte-for-byte — get it wrong and every save re-uploads forever.
// * `autocleanup` keeps only the newest `autocleanup_limit` saves per
//   (rom, slot) and deletes the rest. Only applies when a slot is given.
// * `/api/sync/negotiate` covers **saves only** — its payload has no states
//   array. States have their own endpoints and no negotiation.

/// A device registered with the server; sync is scoped to one.
///
/// `POST /api/devices` answers with `device_id` while `GET /api/devices` lists
/// `id`, so both spellings are accepted rather than silently deserialising to
/// nothing.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    #[serde(alias = "device_id")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Save {
    pub id: i64,
    pub rom_id: i64,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size_bytes: i64,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub emulator: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// What the client believes it has, for one save.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClientSaveState {
    pub rom_id: i64,
    pub file_name: String,
    pub slot: Option<String>,
    pub emulator: Option<String>,
    pub content_hash: String,
    pub updated_at: String,
    pub file_size_bytes: i64,
}

/// What the server tells us to do about one save.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SyncOperation {
    /// `upload`, `download`, `conflict` or `no_op`.
    pub action: String,
    pub rom_id: i64,
    #[serde(default)]
    pub save_id: Option<i64>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub emulator: Option<String>,
    /// Why the server chose this action — worth showing verbatim.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub server_content_hash: Option<String>,
    #[serde(default)]
    pub server_updated_at: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SyncPlan {
    #[serde(default)]
    pub session_id: Option<i64>,
    #[serde(default)]
    pub operations: Vec<SyncOperation>,
    #[serde(default)]
    pub total_upload: i64,
    #[serde(default)]
    pub total_download: i64,
    #[serde(default)]
    pub total_conflict: i64,
    #[serde(default)]
    pub total_no_op: i64,
}

/// Upload rejected because the server's copy moved on since our last sync.
#[derive(Debug)]
pub struct Conflict {
    pub detail: String,
}

impl Client {
    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base, path);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .json(body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("POST {url} -> {status}\n  {}", body.chars().take(300).collect::<String>());
        }
        resp.json::<T>().await.with_context(|| format!("decoding {url}"))
    }

    pub async fn devices(&self) -> Result<Vec<Device>> {
        self.get_json("/api/devices").await
    }

    /// Register this machine, or return its existing registration.
    ///
    /// **`hostname` is load-bearing.** The server deduplicates on a fingerprint
    /// of `(mac_address, hostname, platform)` — verified in
    /// `/backend/endpoints/device.py`. It ignores `client_device_identifier`
    /// on create entirely. Registering without a hostname fingerprints as
    /// all-nulls, matches nothing, and mints a brand new device every call;
    /// each new device starts with empty sync bookkeeping, so every save then
    /// looks like a first-time upload.
    pub async fn register_device(&self, name: &str, hostname: &str) -> Result<Device> {
        self.post_json(
            "/api/devices",
            &serde_json::json!({
                "name": name,
                "hostname": hostname,
                "platform": std::env::consts::OS,
                "client": "romm-desktop",
                "client_version": env!("CARGO_PKG_VERSION"),
                "allow_existing": true,
            }),
        )
        .await
    }

    /// Saves the server holds, optionally for one ROM.
    pub async fn saves(&self, rom_id: Option<i64>) -> Result<Vec<Save>> {
        let path = match rom_id {
            Some(id) => format!("/api/saves?rom_id={id}"),
            None => "/api/saves".to_owned(),
        };
        self.get_json(&path).await
    }

    /// Ask the server what to do, given everything we hold locally.
    pub async fn negotiate(&self, device_id: &str, saves: &[ClientSaveState]) -> Result<SyncPlan> {
        self.post_json(
            "/api/sync/negotiate",
            &serde_json::json!({ "device_id": device_id, "saves": saves }),
        )
        .await
    }

    /// Download one save's bytes.
    pub async fn save_content(&self, save_id: i64, device_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/api/saves/{save_id}/content?device_id={device_id}", self.base);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            bail!("GET {url} -> {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }

    /// Tell the server a download landed, so its per-device bookkeeping stays
    /// accurate and the file is not offered again next time.
    pub async fn confirm_download(&self, save_id: i64) -> Result<()> {
        let url = format!("{}/api/saves/{save_id}/downloaded", self.base);
        self.http
            .post(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        Ok(())
    }

    /// Upload a save. `Ok(Err(Conflict))` means the server refused because its
    /// copy is newer — a real outcome to show the user, not an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_save(
        &self,
        rom_id: i64,
        file_name: &str,
        bytes: Vec<u8>,
        slot: Option<&str>,
        emulator: Option<&str>,
        device_id: &str,
        session_id: Option<i64>,
    ) -> Result<std::result::Result<Save, Conflict>> {
        let mut url = format!("/api/saves?rom_id={rom_id}&device_id={device_id}");
        if let Some(s) = slot {
            url.push_str(&format!("&slot={}", urlencode(s)));
        }
        if let Some(e) = emulator {
            url.push_str(&format!("&emulator={}", urlencode(e)));
        }
        if let Some(s) = session_id {
            url.push_str(&format!("&session_id={s}"));
        }
        // overwrite stays off: that is what makes the server detect conflicts.

        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_owned());
        let form = reqwest::multipart::Form::new().part("saveFile", part);

        let full = format!("{}{}", self.base, url);
        let resp = self
            .http
            .post(&full)
            .header("Authorization", format!("Basic {}", self.auth))
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("POST {full}"))?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            let detail = resp.text().await.unwrap_or_default();
            return Ok(Err(Conflict { detail }));
        }
        if !resp.status().is_success() {
            let s = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("POST {full} -> {s}\n  {}", body.chars().take(300).collect::<String>());
        }
        Ok(Ok(resp.json().await.context("decoding uploaded save")?))
    }

    /// Close a sync session so the server stops counting it as in flight.
    pub async fn complete_session(&self, session_id: i64) -> Result<()> {
        let url = format!("{}/api/sync/sessions/{session_id}/complete", self.base);
        self.http
            .post(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        Ok(())
    }
}
