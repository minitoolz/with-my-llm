use super::*;
use std::path::Path;
use tokio::net::TcpListener;

fn access(command: &str, persistent: bool, cleanup: Option<String>) -> AccessSession {
    AccessSession::new(Some(Access {
        command: command.into(),
        persistent,
        cleanup,
        timeout_secs: 1,
        health_check_interval_secs: 30,
    }))
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

async fn listening_backend() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, url)
}

async fn unavailable_backend() -> String {
    let (listener, url) = listening_backend().await;
    drop(listener);
    url
}

#[tokio::test]
async fn preparation_command_finishes_before_session_starts() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("prepared");
    let (_listener, url) = listening_backend().await;
    let mut session = access(&format!("printf ready > {}", quote(&marker)), false, None);
    session.start(&url).await.unwrap();
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "ready");
    assert!(session
        .process
        .as_mut()
        .unwrap()
        .try_wait()
        .unwrap()
        .is_some());
    session.cleanup().await;
}

#[tokio::test]
async fn failed_preparation_reports_status_and_runs_cleanup_once() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("cleanup");
    let (_listener, url) = listening_backend().await;
    let mut session = access(
        "exit 7",
        false,
        Some(format!("printf cleanup >> {}", quote(&marker))),
    );
    let error = session.start(&url).await.unwrap_err().to_string();
    assert!(error.contains("failed"), "{error}");
    assert!(error.contains('7'), "{error}");
    session.cleanup().await;
    session.cleanup().await;
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "cleanup");
}

#[tokio::test]
async fn persistent_process_death_before_readiness_is_reported() {
    let url = unavailable_backend().await;
    let mut session = access("exit 9", true, None);
    let error = session.start(&url).await.unwrap_err().to_string();
    session.cleanup().await;
    assert!(error.contains("exited before backend readiness"), "{error}");
}

#[tokio::test]
async fn successful_session_runs_configured_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("cleanup");
    let (_listener, url) = listening_backend().await;
    let mut session = access(
        "sleep 60",
        true,
        Some(format!("printf done > {}", quote(&marker))),
    );
    session.start(&url).await.unwrap();
    assert!(!marker.exists());
    session.cleanup().await;
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "done");
    assert!(session.process.is_none());
    assert!(session.group.is_none());
}

#[tokio::test]
async fn backend_readiness_timeout_still_allows_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("cleanup");
    let url = unavailable_backend().await;
    let mut session = access(
        "true",
        false,
        Some(format!("printf done > {}", quote(&marker))),
    );
    let start = tokio::time::Instant::now();
    let error = session.start(&url).await.unwrap_err().to_string();
    session.cleanup().await;
    assert!(error.contains("did not become reachable"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(4));
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "done");
}

#[tokio::test]
async fn preparation_timeout_is_bounded_and_cleanup_runs() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("cleanup");
    let (_listener, url) = listening_backend().await;
    let mut session = access(
        "sleep 60",
        false,
        Some(format!("printf done > {}", quote(&marker))),
    );
    let error = session.start(&url).await.unwrap_err().to_string();
    session.cleanup().await;
    assert!(error.contains("command timed out"), "{error}");
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "done");
}

#[tokio::test]
async fn cleanup_terminates_persistent_descendants_even_if_they_ignore_term() {
    let directory = tempfile::tempdir().unwrap();
    let heartbeat = directory.path().join("heartbeat");
    let (_listener, url) = listening_backend().await;
    let command = format!(
        "(trap '' TERM; while :; do printf x >> {}; sleep 0.05; done) & wait",
        quote(&heartbeat)
    );
    let mut session = access(&command, true, None);
    session.start(&url).await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if std::fs::metadata(&heartbeat)
            .map(|m| m.len() >= 2)
            .unwrap_or(false)
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "descendant heartbeat did not start"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    session.cleanup().await;
    let length = std::fs::metadata(&heartbeat).unwrap().len();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        std::fs::metadata(&heartbeat).unwrap().len(),
        length,
        "a descendant kept running after cleanup"
    );
}

#[tokio::test]
async fn supervision_resumes_after_cancellation_during_restart_delay() {
    let (_listener, url) = listening_backend().await;
    let mut session = access("sleep 60", true, None);
    session.start(&url).await.unwrap();
    let bridge = crate::bridge::Bridge::start(
        crate::config::Backend {
            url: url.clone(),
            protocol: crate::config::Protocol::Responses,
            model: None,
            auth: None,
            access: None,
        },
        None,
    )
    .await
    .unwrap();
    signal_group(session.group.unwrap(), libc::SIGKILL);
    assert!(
        tokio::time::timeout(Duration::from_millis(250), session.supervise(&bridge))
            .await
            .is_err()
    );
    assert!(session.group.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(1250), session.supervise(&bridge))
            .await
            .is_err()
    );
    assert!(session.group.is_some());
    assert!(!exited_without_reaping(session.group.unwrap()).unwrap());
    session.cleanup().await;
}

#[tokio::test]
async fn unused_session_does_not_execute_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("cleanup");
    let mut session = access("true", false, Some(format!("touch {}", quote(&marker))));
    session.cleanup().await;
    assert!(!marker.exists());
}

#[tokio::test]
async fn failed_health_check_cleans_up_before_restarting_both_command_lifecycles() {
    use axum::{http::HeaderMap, routing::get, Json, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    for persistent in [false, true] {
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/v1/models",
                    get(move |headers: HeaderMap| {
                        let observed = observed.clone();
                        async move {
                            assert_eq!(headers["authorization"], "Bearer health-secret");
                            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                                // A TCP connection and HTTP 200 alone are not healthy.
                                Json(serde_json::json!({"error":"unavailable"}))
                            } else {
                                Json(serde_json::json!({"data":[{"id":"ready"}]}))
                            }
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path().join("events");
        let mut session = access(
            &format!(
                "printf S >> {}; {}",
                quote(&events),
                if persistent { "sleep 60" } else { "true" }
            ),
            persistent,
            Some(format!("printf C >> {}", quote(&events))),
        );
        session.config.as_mut().unwrap().health_check_interval_secs = 1;
        let bridge = crate::bridge::Bridge::start(
            crate::config::Backend {
                url: url.clone(),
                protocol: crate::config::Protocol::Responses,
                model: Some("explicit-model".into()),
                auth: Some(crate::config::Auth::Bearer {
                    token: "health-secret".into(),
                }),
                access: None,
            },
            Some("explicit-model".into()),
        )
        .await
        .unwrap();
        // Cached or explicitly configured models must never mask API failure.
        assert_eq!(bridge.model_ids().await.unwrap(), ["explicit-model"]);
        session.start(&url).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(3800), session.supervise(&bridge))
                .await
                .is_err()
        );
        assert!(requests.load(Ordering::SeqCst) >= 2);
        assert_eq!(std::fs::read_to_string(&events).unwrap(), "SCS");
        session.cleanup().await;
        assert_eq!(std::fs::read_to_string(&events).unwrap(), "SCSC");
        assert!(session.group.is_none());
        assert!(session.process.is_none());
        server.abort();
    }
}

#[test]
fn health_check_interval_defaults_and_can_be_disabled() {
    let config: Access = serde_json::from_str(r#"{"command":"true"}"#).unwrap();
    assert_eq!(config.health_check_interval_secs, 30);
    let config: Access =
        serde_json::from_str(r#"{"command":"true","health_check_interval_secs":0}"#).unwrap();
    assert_eq!(config.health_check_interval_secs, 0);
}
