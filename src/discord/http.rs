use std::sync::Arc;

use twilight_http::Client as HttpClient;

/// Builds the Discord REST API client.
///
/// This is separate from the gateway connection: the gateway is a
/// long-lived websocket that *receives* events (messages, joins, etc.),
/// while this HTTP client is what we use to *do* things — send messages,
/// register commands, change roles, ban users.
pub fn build_client(token: String) -> Arc<HttpClient> {
    Arc::new(HttpClient::new(token))
}
