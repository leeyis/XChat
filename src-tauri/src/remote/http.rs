use super::*;
use crate::web_server::AppState;
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use std::net::SocketAddr;

fn response<T: Serialize>(result: Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":error})),
        )
            .into_response(),
    }
}
fn local_origin(headers: &HeaderMap, address: SocketAddr) -> bool {
    if !address.ip().is_loopback() {
        return false;
    }
    let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let local = url.host_str().is_some_and(|h| {
        h == "localhost"
            || h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !local {
        return false;
    }
    match headers.get("origin").and_then(|h| h.to_str().ok()) {
        Some(origin) => origin == format!("http://{host}"),
        None => headers
            .get("sec-fetch-site")
            .and_then(|h| h.to_str().ok())
            .is_none_or(|site| site == "same-origin" || site == "none"),
    }
}
async fn local_only(request: Request, next: Next) -> Response {
    let address = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|a| a.0);
    if !address.is_some_and(|address| local_origin(request.headers(), address)) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"远程协助请在本机桌面客户端或 localhost 页面操作"})),
        )
            .into_response();
    }
    next.run(request).await
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .merge(capture_stream::routes())
        .merge(
            Router::new()
                .route("/api/remote/ui/bootstrap", post(bootstrap))
                .route("/api/remote/ui/poll", post(poll))
                .route("/api/remote/ui/start", post(start))
                .route("/api/remote/ui/action", post(action))
                .layer(middleware::from_fn(local_only)),
        )
        .route("/api/remote/peer/signal", post(signal))
        .route("/api/remote/peer/challenge", post(challenge))
        .layer(DefaultBodyLimit::max(128 * 1024))
}
async fn bootstrap(State(state): State<Arc<AppState>>) -> Response {
    response(match hub(&state.pool, &state.peer_manager).await {
        Ok(h) => h.bootstrap(false),
        Err(e) => Err(e),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Poll {
    pub actor: String,
    pub id: Option<String>,
    pub after: u64,
}
async fn poll(State(state): State<Arc<AppState>>, Json(request): Json<Poll>) -> Response {
    response(match hub(&state.pool, &state.peer_manager).await {
        Ok(h) => h.poll(&request.actor, request.id.as_deref(), request.after),
        Err(e) => Err(e),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub actor: String,
    pub peer_id: String,
    pub invitation: Invite,
}
async fn start(State(state): State<Arc<AppState>>, Json(request): Json<Start>) -> Response {
    let result = async {
        hub(&state.pool, &state.peer_manager)
            .await?
            .start(&request.actor, &request.peer_id, request.invitation)
            .await
    }
    .await;
    response(result)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Act {
    pub actor: String,
    pub id: String,
    pub action: Action,
}
async fn action(State(state): State<Arc<AppState>>, Json(request): Json<Act>) -> Response {
    let result = async {
        hub(&state.pool, &state.peer_manager)
            .await?
            .action(&request.actor, &request.id, request.action)
            .await
    }
    .await;
    response(result)
}
async fn signal(State(state): State<Arc<AppState>>, Json(request): Json<Envelope>) -> Response {
    let result = async {
        hub(&state.pool, &state.peer_manager)
            .await?
            .receive(request)
            .await
    }
    .await;
    response(result)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    id: String,
    secret: String,
    invitation: Invite,
}
async fn challenge(State(state): State<Arc<AppState>>, Json(request): Json<Challenge>) -> Response {
    match hub(&state.pool, &state.peer_manager).await {
        Ok(h) if h.challenge(&request.id, &request.secret, &request.invitation) => {
            Json(serde_json::json!({"verified":true})).into_response()
        }
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_web_consent_is_local_and_same_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:18888".parse().unwrap());
        assert!(local_origin(&headers, "127.0.0.1:10000".parse().unwrap()));
        assert!(!local_origin(
            &headers,
            "192.168.1.2:10000".parse().unwrap()
        ));
        headers.insert("origin", "https://example.com".parse().unwrap());
        assert!(!local_origin(&headers, "127.0.0.1:10000".parse().unwrap()));
        headers.insert("origin", "http://127.0.0.1:18888".parse().unwrap());
        assert!(local_origin(&headers, "127.0.0.1:10000".parse().unwrap()));
        headers.insert("host", "rebound.example.com:18888".parse().unwrap());
        headers.remove("origin");
        assert!(!local_origin(&headers, "127.0.0.1:10000".parse().unwrap()));
    }
}
