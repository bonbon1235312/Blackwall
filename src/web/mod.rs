pub mod routes;
pub mod templates;

use std::sync::Arc;

use tokio::net::TcpListener;

use crate::state::AppState;

pub async fn run(state: Arc<AppState>, bind_addr: String) {
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(source) => {
            tracing::error!(?source, %bind_addr, "failed to bind verification web server");
            return;
        }
    };

    tracing::info!(%bind_addr, "verification web server listening");

    if let Err(source) = axum::serve(listener, routes::router(state)).await {
        tracing::error!(?source, "verification web server stopped unexpectedly");
    }
}
