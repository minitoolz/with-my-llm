use anyhow::{bail, ensure, Context, Result};
use clap::Args;
use flate2::read::GzDecoder;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

const REPOSITORY: &str = "minitoolz/with-my-llm";
const LATEST_URL: &str = "https://api.github.com/repos/minitoolz/with-my-llm/releases/latest";
const MAX_ARCHIVE: usize = 128 * 1024 * 1024;
const MAX_BINARY: u64 = 256 * 1024 * 1024;

#[derive(Args)]
pub struct Options {
    /// Show the available version without downloading or replacing the binary
    #[arg(long)]
    check: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn target(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => bail!("self-update is not supported on {os}/{arch}; install from source"),
    }
}

impl Release {
    fn version(&self) -> Result<Version> {
        ensure!(
            !self.draft && !self.prerelease,
            "release is not a stable published release"
        );
        let version = Version::parse(self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name))
            .context("release tag is not a semantic version")?;
        ensure!(version.pre.is_empty(), "release tag is a prerelease");
        Ok(version)
    }

    fn asset_url(&self, name: &str) -> Result<&str> {
        let matches: Vec<_> = self
            .assets
            .iter()
            .filter(|asset| asset.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "release must contain exactly one {name} asset"
        );
        let url = &matches[0].browser_download_url;
        ensure!(
            *url == format!(
                "https://github.com/{REPOSITORY}/releases/download/{}/{name}",
                self.tag_name
            ),
            "unexpected download URL for {name}"
        );
        Ok(url)
    }
}

async fn download(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await.context("request failed")?;
    match response.status() {
        reqwest::StatusCode::NOT_FOUND => {
            bail!("release or asset not found; a published release with binaries is required")
        }
        reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::TOO_MANY_REQUESTS => {
            bail!("GitHub denied the request or its rate limit was reached; try again later")
        }
        _ => (),
    }
    response.error_for_status_ref().context("download failed")?;
    ensure!(
        response.content_length().unwrap_or(0) <= limit as u64,
        "download exceeds size limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= limit - bytes.len(),
            "download exceeds size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn run(options: Options) -> Result<()> {
    let target = target(std::env::consts::OS, std::env::consts::ARCH)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .user_agent(concat!("with-my-llm/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let release: Release =
        serde_json::from_slice(&download(&client, LATEST_URL, 2 * 1024 * 1024).await?)
            .context("invalid release metadata")?;
    let latest = release.version()?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    if latest.cmp_precedence(&current) != std::cmp::Ordering::Greater {
        println!("wmlm {current} is up to date (latest stable: {latest}).");
        return Ok(());
    }
    let name = format!("wmlm-{target}.tar.gz");
    let archive_url = release.asset_url(&name)?;
    let checksum_url = release.asset_url(&format!("{name}.sha256"))?;
    println!("Update available: wmlm {current} -> {latest} ({target})");
    if options.check {
        println!("Run `wmlm update` to install it.");
        return Ok(());
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    println!("Downloading {name}...");
    let checksum = download(&client, checksum_url, 4096).await?;
    let archive = download(&client, archive_url, MAX_ARCHIVE).await?;
    install(&executable, &archive, &checksum, &name)?;
    println!("Updated to wmlm {latest}. Settings were preserved.");
    Ok(())
}

fn verify_checksum(archive: &[u8], checksum: &[u8], name: &str) -> Result<()> {
    let text = std::str::from_utf8(checksum).context("invalid checksum file")?;
    let fields: Vec<_> = text.split_whitespace().collect();
    ensure!(
        fields.len() == 2 && fields[1].trim_start_matches('*') == name,
        "invalid checksum entry for {name}"
    );
    ensure!(
        fields[0].len() == 64 && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid SHA-256 digest"
    );
    ensure!(
        format!("{:x}", Sha256::digest(archive)).eq_ignore_ascii_case(fields[0]),
        "SHA-256 checksum mismatch; existing binary was not changed"
    );
    Ok(())
}

fn install(executable: &Path, archive: &[u8], checksum: &[u8], name: &str) -> Result<()> {
    verify_checksum(archive, checksum, name)?;
    let parent = executable
        .parent()
        .context("executable has no parent directory")?;
    // Stage on the same filesystem so the final rename atomically replaces the old inode.
    let mut staged = tempfile::NamedTempFile::new_in(parent).context(
        "cannot create update file beside the executable; check installation directory permissions",
    )?;
    // Bound total decompression, including tar metadata and trailing padding.
    let expanded_limit = MAX_BINARY + 1024 * 1024;
    let mut expanded = Vec::new();
    GzDecoder::new(archive)
        .take(expanded_limit + 1)
        .read_to_end(&mut expanded)
        .context("invalid gzip archive")?;
    ensure!(
        expanded.len() as u64 <= expanded_limit,
        "expanded archive exceeds size limit"
    );
    let mut archive = tar::Archive::new(expanded.as_slice());
    let mut found = false;
    for entry in archive.entries().context("invalid release archive")? {
        let mut entry = entry?;
        ensure!(
            !found
                && entry.path()?.as_ref() == Path::new("wmlm")
                && entry.header().entry_type().is_file(),
            "archive must contain only one regular file named wmlm"
        );
        ensure!(
            entry.size() > 0 && entry.size() <= MAX_BINARY,
            "invalid binary size"
        );
        std::io::copy(&mut entry, &mut staged)?;
        found = true;
    }
    ensure!(found, "archive does not contain wmlm");
    let permissions = std::fs::metadata(executable)?.permissions().mode() & 0o777;
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(permissions | 0o700))?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    staged
        .persist(executable)
        .map_err(|error| error.error)
        .context("could not replace executable; existing binary was not changed")?;
    Ok(())
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
