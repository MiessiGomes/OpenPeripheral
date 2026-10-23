//! Aviso de versão nova via API pública do GitHub (releases, senão tags).

use serde::{Deserialize, Serialize};

pub const GITHUB_REPO: &str = "MiessiGomes/OpenPeripheral";

#[derive(Clone, Debug, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub url: Option<String>,
    pub available: bool,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            current: current_version().into(),
            latest: None,
            url: None,
            available: false,
        }
    }
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn releases_page() -> String {
    format!("https://github.com/{GITHUB_REPO}/releases")
}

pub fn check() -> UpdateStatus {
    check_repo(GITHUB_REPO, current_version())
}

fn check_repo(repo: &str, current: &str) -> UpdateStatus {
    let mut status = UpdateStatus {
        current: current.into(),
        latest: None,
        url: None,
        available: false,
    };
    if let Some((tag, url)) = latest_from_github(repo) {
        status.available = is_newer(&tag, current);
        status.latest = Some(tag);
        status.url = Some(url);
    }
    status
}

fn latest_from_github(repo: &str) -> Option<(String, String)> {
    let ua = format!("OpenPeripheral/{}", current_version());
    if let Some(v) = get_json::<GhRelease>(
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        &ua,
    ) {
        if !v.tag_name.is_empty() {
            let url = if v.html_url.is_empty() {
                format!("https://github.com/{repo}/releases/tag/{}", v.tag_name)
            } else {
                v.html_url
            };
            return Some((v.tag_name, url));
        }
    }
    let tags: Vec<GhTag> = get_json(
        &format!("https://api.github.com/repos/{repo}/tags?per_page=1"),
        &ua,
    )?;
    let tag = tags.into_iter().next()?;
    if tag.name.is_empty() {
        return None;
    }
    let url = format!("https://github.com/{repo}/releases/tag/{}", tag.name);
    Some((tag.name, url))
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str, ua: &str) -> Option<T> {
    let resp = ureq::get(url)
        .set("User-Agent", ua)
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .ok()?;
    if resp.status() != 200 {
        return None;
    }
    resp.into_json().ok()
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
}

#[derive(Deserialize)]
struct GhTag {
    name: String,
}

pub fn normalize_version(s: &str) -> Vec<u64> {
    s.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let a = normalize_version(latest);
    let b = normalize_version(current);
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_semver() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
    }
}
