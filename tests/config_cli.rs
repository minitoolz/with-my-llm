use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output, Stdio},
};

fn invoke(path: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wmlm"))
        .arg("cfg")
        .args(args)
        .arg("--config")
        .arg(path)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn success(path: &Path, args: &[&str], stdin: Option<&str>) -> Value {
    let output = invoke(path, args, stdin);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn saved(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn configuration_can_be_created_edited_inspected_and_deleted_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let empty = success(&path, &["show"], None);
    assert_eq!(empty["backends"], json!({}));
    assert!(!path.exists());
    success(
        &path,
        &[
            "backend",
            "add",
            "local",
            "--url",
            "http://localhost:{PORT}/v1",
            "--protocol",
            "chat_completions",
            "--model",
            "first",
            "--auth-file",
            "-",
            "--access-command",
            "sleep 60",
            "--persistent",
            "true",
            "--timeout-secs",
            "12",
            "--health-check-interval-secs",
            "0",
            "--cleanup",
            "true",
        ],
        Some(r#"{"type":"bearer","token":"private-token"}"#),
    );
    success(
        &path,
        &["bind", "opencode", "local", "--model", "override"],
        None,
    );
    success(&path, &["ports", "3000, 8000-8010"], None);
    let key = dir.path().join("brave-key");
    fs::write(&key, "private-search-key\r\n").unwrap();
    success(
        &path,
        &[
            "web-search",
            "brave",
            "--api-key-file",
            key.to_str().unwrap(),
        ],
        None,
    );
    success(&path, &["web-search", "brave"], None);
    success(
        &path,
        &["backend", "edit", "local", "--model", "second"],
        None,
    );
    let value = saved(&path);
    assert_eq!(value["backends"]["local"]["auth"]["token"], "private-token");
    assert_eq!(value["backends"]["local"]["access"]["persistent"], true);
    assert_eq!(value["backends"]["local"]["access"]["timeout_secs"], 12);
    assert_eq!(
        value["backends"]["local"]["access"]["health_check_interval_secs"],
        0
    );
    assert_eq!(value["backends"]["local"]["model"], "second");
    assert_eq!(value["agents"]["opencode"]["model"], "override");
    assert_eq!(value["random_port_candidates"], "3000, 8000-8010");
    assert_eq!(value["web_search"]["api_key"], "private-search-key");
    let shown = success(&path, &["show"], None).to_string();
    assert!(!shown.contains("private-token"));
    assert!(!shown.contains("private-search-key"));
    assert_eq!(success(&path, &["show", "--show-secrets"], None), value);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    success(
        &path,
        &[
            "backend",
            "edit",
            "local",
            "--clear-model",
            "--clear-auth",
            "--clear-cleanup",
            "--persistent",
            "false",
        ],
        None,
    );
    let value = saved(&path);
    assert!(value["backends"]["local"]["model"].is_null());
    assert!(value["backends"]["local"]["auth"].is_null());
    assert!(value["backends"]["local"]["access"]["cleanup"].is_null());
    assert_eq!(value["backends"]["local"]["access"]["persistent"], false);
    success(&path, &["backend", "edit", "local", "--direct"], None);
    assert!(saved(&path)["backends"]["local"]["access"].is_null());
    success(&path, &["bind", "opencode", "local"], None);
    assert!(saved(&path)["agents"]["opencode"]["model"].is_null());
    success(&path, &["unbind", "opencode"], None);
    success(&path, &["bind", "pi", "local"], None);
    success(
        &path,
        &["web-search", "searxng", "--url", "http://localhost:8080"],
        None,
    );
    assert_eq!(saved(&path)["web_search"]["provider"], "searxng");
    success(&path, &["web-search", "public"], None);
    success(&path, &["backend", "delete", "local"], None);
    let value = saved(&path);
    assert_eq!(value["backends"], json!({}));
    assert_eq!(value["agents"], json!({}));
}

#[test]
fn invalid_edits_do_not_change_the_saved_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    success(
        &path,
        &[
            "backend",
            "add",
            "local",
            "--url",
            "http://localhost:8000/v1",
            "--protocol",
            "responses",
        ],
        None,
    );
    let before = fs::read(&path).unwrap();
    for args in [
        vec![
            "backend",
            "add",
            "local",
            "--url",
            "http://localhost",
            "--protocol",
            "anthropic",
        ],
        vec!["backend", "edit", "missing", "--model", "new"],
        vec!["backend", "delete", "missing"],
        vec!["backend", "add", "incomplete", "--url", "http://localhost"],
        vec!["backend", "edit", "local", "--url", "ftp://localhost"],
        vec!["backend", "edit", "local", "--persistent", "true"],
        vec![
            "backend",
            "edit",
            "local",
            "--access-command",
            "true",
            "--timeout-secs",
            "0",
        ],
        vec![
            "backend",
            "edit",
            "local",
            "--access-command",
            "true",
            "--direct",
        ],
        vec![
            "backend",
            "edit",
            "local",
            "--model",
            "new",
            "--clear-model",
        ],
        vec!["bind", "opencode", "missing"],
        vec!["unbind", "claude"],
        vec!["ports", "0"],
        vec!["web-search", "brave"],
        vec!["web-search", "searxng", "--url", "file:///tmp"],
    ] {
        let output = invoke(&path, &args, None);
        assert!(!output.status.success(), "unexpected success: {args:?}");
        assert_eq!(fs::read(&path).unwrap(), before, "modified file: {args:?}");
    }
    let output = invoke(
        &path,
        &["backend", "edit", "local", "--auth-file", "-"],
        Some(r#"{"type":"my-private-secret"}"#),
    );
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("my-private-secret"));
    assert_eq!(fs::read(&path).unwrap(), before);
    success(
        &path,
        &["backend", "edit", "local", "--auth-file", "-"],
        Some(r#"{"type":"basic","username":"private-user","password":"private-password"}"#),
    );
    let shown = success(&path, &["show"], None).to_string();
    assert!(!shown.contains("private-user"));
    assert!(!shown.contains("private-password"));
}

#[tokio::test]
async fn backend_test_checks_models_and_cleans_up_without_changing_configuration() {
    use axum::{routing::get, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/models",
                get(|| async { Json(json!({"data":[{"id":"test-model"}]})) }),
            ),
        )
        .await
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let cleanup = dir.path().join("cleaned");
    fs::write(&path, serde_json::to_vec(&json!({"backends":{"local":{"url":format!("http://{address}/v1"),"protocol":"responses","access":{"command":"echo access-diagnostic","cleanup":format!("echo cleanup-diagnostic; touch '{}'",cleanup.display())}}}})).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        let output = success(&path, &["backend", "test", "local"], None);
        assert_eq!(fs::read(path).unwrap(), before);
        output
    })
    .await
    .unwrap();
    assert_eq!(output["models"], json!(["test-model"]));
    assert!(cleanup.exists());
    server.abort();
}
