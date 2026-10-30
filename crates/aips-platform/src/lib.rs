//! Shared platform services for AIPS binaries: authentication, telemetry, secrets.
pub mod auth;
pub mod telemetry;
pub mod vault;

pub use auth::{require_auth, ApiError, Authenticator, Principal};

/// Resolves on SIGINT or SIGTERM. Kubernetes and process managers stop pods with SIGTERM; without
/// this the services died mid-request and the Kafka consumer never left its group, which stalled
/// ingestion on restart until the broker expired the dead member (bug B14).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutdown signal received; draining");
}
