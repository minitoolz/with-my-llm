use super::*;
use flate2::{write::GzEncoder, Compression};

fn release(tag: &str) -> Release {
    Release {
        tag_name: tag.into(),
        draft: false,
        prerelease: false,
        assets: vec![],
    }
}

fn fixture(files: &[(&str, &[u8])]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, *content).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn checksum(bytes: &[u8]) -> Vec<u8> {
    format!("{:x}  test.tar.gz\n", Sha256::digest(bytes)).into_bytes()
}

#[test]
fn selects_supported_platforms_and_rejects_others() {
    for (os, arch, expected) in [
        ("linux", "x86_64", "x86_64-unknown-linux-musl"),
        ("linux", "aarch64", "aarch64-unknown-linux-musl"),
        ("macos", "x86_64", "x86_64-apple-darwin"),
        ("macos", "aarch64", "aarch64-apple-darwin"),
    ] {
        assert_eq!(target(os, arch).unwrap(), expected);
    }
    assert!(target("linux", "arm").is_err());
    assert!(target("windows", "x86_64").is_err());
}

#[test]
fn accepts_stable_semver_and_compares_numerically() {
    assert!(release("v0.0.10").version().unwrap() > release("0.0.9").version().unwrap());
    for tag in ["latest", "v0.0.2-beta.1", "../../bad"] {
        assert!(release(tag).version().is_err());
    }
    let mut candidate = release("v0.0.2");
    candidate.draft = true;
    assert!(candidate.version().is_err());
    candidate.draft = false;
    candidate.prerelease = true;
    assert!(candidate.version().is_err());
    assert_eq!(
        release("v0.0.1+build.2")
            .version()
            .unwrap()
            .cmp_precedence(&Version::new(0, 0, 1)),
        std::cmp::Ordering::Equal
    );
}

#[test]
fn requires_unique_assets_from_the_expected_repository_and_tag() {
    let mut candidate = release("v0.0.2");
    assert!(candidate.asset_url("binary").is_err());
    candidate.assets.push(Asset {
        name: "binary".into(),
        browser_download_url: "https://example.com/binary".into(),
    });
    assert!(candidate.asset_url("binary").is_err());
    candidate.assets[0].browser_download_url =
        format!("https://github.com/{REPOSITORY}/releases/download/v0.0.2/binary");
    assert!(candidate.asset_url("binary").is_ok());
    candidate.assets.push(Asset {
        name: "binary".into(),
        browser_download_url: candidate.assets[0].browser_download_url.clone(),
    });
    assert!(candidate.asset_url("binary").is_err());
}

#[test]
fn replacement_is_atomic_and_preserves_settings_and_executable_mode() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("wmlm");
    std::fs::write(&executable, "old").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut old_handle = std::fs::File::open(&executable).unwrap();
    let config = dir.path().join("credential.json");
    std::fs::write(&config, "settings").unwrap();
    let bytes = fixture(&[("wmlm", b"new binary")]);
    install(&executable, &bytes, &checksum(&bytes), "test.tar.gz").unwrap();
    assert_eq!(std::fs::read(&executable).unwrap(), b"new binary");
    let mut old = String::new();
    old_handle.read_to_string(&mut old).unwrap();
    assert_eq!(old, "old");
    assert_eq!(
        std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(std::fs::read_to_string(config).unwrap(), "settings");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn bad_checksums_and_archives_leave_existing_binary_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("wmlm");
    std::fs::write(&executable, "old").unwrap();
    let good = fixture(&[("wmlm", b"new")]);
    let mut truncated = good.clone();
    truncated.truncate(truncated.len() - 6);
    for bytes in [
        fixture(&[]),
        fixture(&[("unexpected", b"new")]),
        fixture(&[("wmlm", b"")]),
        fixture(&[("wmlm", b"new"), ("wmlm", b"duplicate")]),
        fixture(&[("subdir/wmlm", b"new")]),
        b"not gzip".to_vec(),
        truncated,
    ] {
        assert!(install(&executable, &bytes, &checksum(&bytes), "test.tar.gz").is_err());
        assert_eq!(std::fs::read(&executable).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    for invalid in [
        b"garbage".to_vec(),
        checksum(b"different"),
        checksum(&good)
            .iter()
            .copied()
            .chain(b"extra".iter().copied())
            .collect(),
    ] {
        assert!(install(&executable, &good, &invalid, "test.tar.gz").is_err());
        assert_eq!(std::fs::read(&executable).unwrap(), b"old");
    }
}

#[tokio::test]
async fn downloads_enforce_status_and_size_limits() {
    use axum::{routing::get, Router};
    let app = Router::new()
        .route("/ok", get(|| async { "data" }))
        .route(
            "/missing",
            get(|| async { axum::http::StatusCode::NOT_FOUND }),
        )
        .route(
            "/limited",
            get(|| async { axum::http::StatusCode::TOO_MANY_REQUESTS }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    assert_eq!(
        download(&client, &format!("{origin}/ok"), 4).await.unwrap(),
        b"data"
    );
    assert!(download(&client, &format!("{origin}/ok"), 3).await.is_err());
    assert!(download(&client, &format!("{origin}/missing"), 100)
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));
    assert!(download(&client, &format!("{origin}/limited"), 100)
        .await
        .unwrap_err()
        .to_string()
        .contains("rate limit"));
    server.abort();
}
