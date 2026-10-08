use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::key::to_hex;

const REPO: &str = "margual56/Sobre";
const MAX_SIZE: u64 = 400 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub size: u64,
    #[serde(skip)]
    pub url: String,
    #[serde(skip)]
    pub sha256: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// The AppImage file this process was started from, if any.
pub fn running_appimage() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPIMAGE").filter(|v| !v.is_empty())?);
    path.is_file().then_some(path)
}

fn parse_version(v: &str) -> Option<[u64; 3]> {
    let core = v.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let out = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(out)
}

pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (parse_version(candidate), parse_version(installed)) {
        (Some(c), Some(i)) => c > i,
        _ => false,
    }
}

/// Pick the AppImage out of a GitHub "latest release" document.
pub fn pick_update(release_json: &[u8], installed: &str) -> Result<Option<UpdateInfo>> {
    let release: Release =
        serde_json::from_slice(release_json).context("reading the release list")?;
    if !is_newer(&release.tag_name, installed) {
        return Ok(None);
    }
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    let Some(asset) = release
        .assets
        .into_iter()
        .find(|a| a.name.ends_with(".AppImage") && a.browser_download_url.starts_with(&prefix))
    else {
        return Ok(None);
    };
    if asset.size == 0 || asset.size > MAX_SIZE {
        bail!("the published file has an implausible size");
    }
    Ok(Some(UpdateInfo {
        version: release.tag_name.trim_start_matches('v').to_string(),
        notes: release.body.unwrap_or_default(),
        size: asset.size,
        url: asset.browser_download_url,
        sha256: asset
            .digest
            .and_then(|d| d.strip_prefix("sha256:").map(str::to_ascii_lowercase)),
    }))
}

pub async fn check(http: &reqwest::Client, installed: &str) -> Result<Option<UpdateInfo>> {
    let response = http
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("User-Agent", "sobre-updater")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!("GitHub answered HTTP {}", response.status());
    }
    pick_update(&response.bytes().await?, installed)
}

fn staging_path(target: &Path) -> Result<PathBuf> {
    let dir = target
        .parent()
        .ok_or_else(|| anyhow!("the AppImage has no parent folder"))?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("odd AppImage file name"))?;
    Ok(dir.join(format!(".{name}.update")))
}

/// Check a downloaded file and move it over `target`. The running program is
/// not disturbed: it keeps the old file open until it exits.
pub fn put_in_place(
    staged: &Path,
    target: &Path,
    info: &UpdateInfo,
    actual_sha256: &str,
) -> Result<()> {
    let meta = std::fs::metadata(staged)?;
    if meta.len() != info.size {
        bail!(
            "the download is incomplete ({} of {} bytes)",
            meta.len(),
            info.size
        );
    }
    match &info.sha256 {
        Some(expected) if expected == actual_sha256 => {}
        Some(_) => bail!("the downloaded file does not match the published checksum. Nothing was changed."),
        None => bail!("this release has no published checksum, so the download cannot be checked. Nothing was changed."),
    }
    let mut head = [0u8; 4];
    std::io::Read::read_exact(&mut std::fs::File::open(staged)?, &mut head)?;
    if &head != b"\x7fELF" {
        bail!("the downloaded file is not a program. Nothing was changed.");
    }
    let mode = std::fs::metadata(target)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o755)
        | 0o100;
    std::fs::set_permissions(staged, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(staged, target).context("replacing the AppImage")?;
    Ok(())
}

/// Download the release and replace the AppImage at `target` with it.
pub async fn install(
    info: &UpdateInfo,
    target: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    let staged = staging_path(target)?;
    let result = async {
        // GitHub redirects downloads to its file servers.
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("sobre-updater")
            .build()?;
        let mut response = client.get(&info.url).send().await?;
        if !response.status().is_success() {
            bail!("the download failed with HTTP {}", response.status());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&staged)
            .with_context(|| {
                format!(
                    "{} cannot be written to",
                    staged.parent().unwrap_or(target).display()
                )
            })?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        while let Some(chunk) = response.chunk().await? {
            done += chunk.len() as u64;
            if done > info.size {
                bail!("the download is larger than the published file");
            }
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            progress(done, info.size);
        }
        file.sync_all()?;
        drop(file);
        put_in_place(&staged, target, info, &to_hex(&hasher.finalize()))
    }
    .await;
    if result.is_err() {
        std::fs::remove_file(&staged).ok();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "tag_name": "v0.3.0", "body": "- thing",
        "assets": [
          {"name": "Sobre_0.3.0_amd64.deb", "browser_download_url": "https://github.com/margual56/Sobre/releases/download/v0.3.0/Sobre_0.3.0_amd64.deb", "size": 10, "digest": "sha256:aa"},
          {"name": "Sobre_0.3.0_amd64.AppImage", "browser_download_url": "https://github.com/margual56/Sobre/releases/download/v0.3.0/Sobre_0.3.0_amd64.AppImage", "size": 8, "digest": "sha256:ABCD"}
        ]}"#;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("nightly", "0.2.0"));
        assert!(!is_newer("1.2", "0.2.0"));
    }

    #[test]
    fn picks_the_appimage_only_when_newer() {
        let info = pick_update(RELEASE.as_bytes(), "0.2.1").unwrap().unwrap();
        assert_eq!(info.version, "0.3.0");
        assert!(info.url.ends_with(".AppImage"));
        assert_eq!(info.sha256.as_deref(), Some("abcd"));
        assert!(pick_update(RELEASE.as_bytes(), "0.3.0").unwrap().is_none());
        // A download link pointing anywhere else is ignored.
        let elsewhere = RELEASE.replace(
            "github.com/margual56/Sobre/releases/download/v0.3.0/Sobre_0.3.0_amd64.AppImage",
            "evil.example/x.AppImage",
        );
        assert!(pick_update(elsewhere.as_bytes(), "0.2.1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_bad_download_never_replaces_the_app() {
        let dir = std::env::temp_dir().join(format!(
            "sobre-upd-{}",
            to_hex(&crate::db::key::random_bytes::<6>())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Sobre.AppImage");
        std::fs::write(&target, b"\x7fELFold!").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        let staged = staging_path(&target).unwrap();
        let good = b"\x7fELFnew!";
        let sum = to_hex(&Sha256::digest(good));
        let info = |sha: Option<&str>, size| UpdateInfo {
            version: "0.3.0".into(),
            notes: String::new(),
            size,
            url: String::new(),
            sha256: sha.map(str::to_string),
        };

        std::fs::write(&staged, good).unwrap();
        assert!(put_in_place(&staged, &target, &info(Some("00"), 8), &sum).is_err());
        assert!(put_in_place(&staged, &target, &info(None, 8), &sum).is_err());
        assert!(put_in_place(&staged, &target, &info(Some(&sum), 9), &sum).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"\x7fELFold!");

        std::fs::write(&staged, b"<html>!!").unwrap();
        let html_sum = to_hex(&Sha256::digest(b"<html>!!"));
        assert!(put_in_place(&staged, &target, &info(Some(&html_sum), 8), &html_sum).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"\x7fELFold!");

        std::fs::write(&staged, good).unwrap();
        put_in_place(&staged, &target, &info(Some(&sum), 8), &sum).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), good);
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!staged.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
