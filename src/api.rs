//! RomM API client.
//!
//! Findings that shape this file are in PLAN.md §3. The two that bite:
//! the list param is `platform_ids` (an array — unknown params are silently
//! ignored, so a typo pages the whole library), and connection reuse is worth
//! ~300x, which `reqwest`'s pool gives us for free as long as one Client is
//! shared.

// These structs mirror the server's schemas rather than only what today's
// commands read, so later stages don't have to re-derive the shape.
#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub fs_slug: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rom_count: i64,
}

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
}

#[derive(Debug, Deserialize)]
pub struct RomPage {
    #[serde(default)]
    pub items: Vec<Rom>,
    #[serde(default)]
    pub total: i64,
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
        let http = reqwest::Client::builder()
            .user_agent("romm-desktop/0.1")
            .build()
            .context("building HTTP client")?;
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
