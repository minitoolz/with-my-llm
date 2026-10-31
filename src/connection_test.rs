use crate::config::{Backend, SearchProvider};
use anyhow::Result;

// Use the same access lifecycle and authenticated discovery as an agent launch.
// Passing no override ensures even an explicitly configured model is tested remotely.
pub async fn test_connection(backend: &Backend, candidates: &str) -> Result<Vec<String>> {
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let backend = if backend.uses_random_port() {
        backend.with_port(crate::random_port::choose(candidates)?)
    } else {
        backend.clone()
    };
    let mut access =
        crate::access::AccessSession::new(backend.access.clone()).with_output_to_stderr();
    let result = tokio::select! {
        result = async {
            access.start(&backend.url).await?;
            let bridge = crate::bridge::Bridge::start_with_search(
                backend.clone(), None, SearchProvider::default(),
            ).await?;
            tokio::select! {
                result = bridge.model_ids() => result,
                never = access.supervise(&bridge) => match never {},
            }
        } => result,
        _ = interrupt.recv() => Err(anyhow::anyhow!("connection test interrupted")),
        _ = terminate.recv() => Err(anyhow::anyhow!("connection test terminated")),
    };
    access.cleanup().await;
    result
}
