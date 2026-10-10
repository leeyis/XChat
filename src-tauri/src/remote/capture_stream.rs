//! A one-use, native-issued capability for demand-driven local HEVC frames.
//! The socket carries no actor/session parameters and grants no other API access.
use super::{hub, Hub, Result, State};
use crate::web_server::{AppState, ServerLifetime};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, State as HttpState, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

const TOKEN_TTL: Duration = Duration::from_secs(15);
const AUTH_TIMEOUT: Duration = Duration::from_secs(3);
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct Scope {
    actor: String,
    id: String,
    revision: u64,
    origin: String,
    port: u16,
    server_generation: String,
}

struct Ticket {
    token: String,
    scope: Scope,
    expires: Instant,
}

#[derive(Clone)]
struct Permit {
    token: String,
    scope: Scope,
}

// At most one pending ticket and one active socket per Hub. Issuing a replacement
// revokes the old socket, including frames whose encoder work is still pending.
#[derive(Default)]
pub(super) struct TicketBook {
    pending: Option<Ticket>,
    active: Option<String>,
}

impl TicketBook {
    fn issue(&mut self, scope: Scope, now: Instant) -> String {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        self.active = None;
        self.pending = Some(Ticket {
            token: token.clone(),
            scope,
            expires: now + TOKEN_TTL,
        });
        token
    }

    fn matching(
        &self,
        origin: &str,
        host: &str,
        generation: &str,
        now: Instant,
    ) -> Option<&Ticket> {
        self.pending.as_ref().filter(|ticket| {
            ticket.expires > now
                && ticket.scope.origin == origin
                && host == format!("127.0.0.1:{}", ticket.scope.port)
                && ticket.scope.server_generation == generation
        })
    }

    fn claim(
        &mut self,
        token: &str,
        origin: &str,
        host: &str,
        generation: &str,
        now: Instant,
    ) -> Result<Permit> {
        if !self
            .matching(origin, host, generation, now)
            .is_some_and(|ticket| ticket.token == token)
        {
            return Err("remote_stream_unauthorized: invalid or expired capability".into());
        }
        let ticket = self.pending.take().unwrap();
        self.active = Some(ticket.token.clone());
        Ok(Permit {
            token: ticket.token,
            scope: ticket.scope,
        })
    }

    fn revoke(&mut self, token: &str) {
        if self.active.as_deref() == Some(token) {
            self.active = None;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|ticket| ticket.token == token)
        {
            self.pending = None;
        }
    }
}

#[derive(Serialize)]
pub struct Descriptor {
    url: String,
    token: String,
    format: &'static str,
    expires_in_ms: u64,
}

fn validate_scope(state: &State, scope: &Scope) -> Result<()> {
    let session = state
        .session
        .as_ref()
        .filter(|session| {
            session.view.id == scope.id && session.owner.as_deref() == Some(scope.actor.as_str())
        })
        .ok_or("remote_stream_unauthorized: session owner changed")?;
    if !session.view.live() {
        return Err("remote_session_ended: screen sharing ended".into());
    }
    if session.view.paused {
        return Err("remote_paused: screen sharing paused".into());
    }
    if session.view.revision != scope.revision {
        return Err("remote_revision_changed: screen authorization changed".into());
    }
    if session.owner_seen.elapsed() > Duration::from_secs(8) {
        return Err("remote_owner_expired: screen owner is no longer active".into());
    }
    Hub::native_session(state, &scope.actor, &scope.id)
        .map_err(|error| format!("remote_stream_unauthorized: {error}"))?;
    Ok(())
}

fn maintain(state: &mut State, permit: &Permit, renew: bool) -> Result<()> {
    if state.capture_stream.active.as_deref() != Some(permit.token.as_str()) {
        return Err("remote_stream_revoked: capture capability was replaced or closed".into());
    }
    validate_scope(state, &permit.scope)?;
    if renew {
        // A current, unexpired authorized Worker is an owner. This deliberately
        // does not depend on a minimized main WebView's JavaScript polling timer.
        let now = Instant::now();
        state.actors.get_mut(&permit.scope.actor).unwrap().seen = now;
        state.session.as_mut().unwrap().owner_seen = now;
    }
    Ok(())
}

impl Hub {
    #[cfg(feature = "desktop")]
    pub(super) fn capture_stream_descriptor(
        &self,
        actor: String,
        id: String,
        revision: u64,
        origin: String,
        port: u16,
        server_generation: String,
    ) -> Result<Descriptor> {
        if port == 0 || server_generation.is_empty() || origin.is_empty() || origin == "null" {
            return Err("remote_stream_unavailable: local capture service is unavailable".into());
        }
        let scope = Scope {
            actor,
            id,
            revision,
            origin,
            port,
            server_generation,
        };
        let mut state = self.lock();
        validate_scope(&state, &scope)?;
        let token = state.capture_stream.issue(scope, Instant::now());
        Ok(Descriptor {
            url: format!("ws://127.0.0.1:{port}/api/remote/native/stream"),
            token,
            format: "hevc-v1",
            expires_in_ms: TOKEN_TTL.as_millis() as u64,
        })
    }

    fn claim_capture_stream(
        &self,
        token: &str,
        origin: &str,
        host: &str,
        generation: &str,
    ) -> Result<Permit> {
        let mut state = self.lock();
        // Validate under the same lock as consumption: a stale ticket cannot
        // revive its session or claim a replacement session after consent changes.
        let ticket = state
            .capture_stream
            .matching(origin, host, generation, Instant::now())
            .filter(|ticket| ticket.token == token)
            .ok_or("remote_stream_unauthorized: invalid or expired capability")?;
        validate_scope(&state, &ticket.scope)?;
        state
            .capture_stream
            .claim(token, origin, host, generation, Instant::now())
    }
}

struct Lease {
    hub: Arc<Hub>,
    permit: Permit,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.hub.lock().capture_stream.revoke(&self.permit.token);
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Auth {
        token: String,
    },
    Next {
        #[serde(default)]
        request_keyframe: bool,
    },
    Stop,
}

fn request(message: Message) -> Result<Request> {
    let Message::Text(text) = message else {
        return Err("remote_stream_protocol: expected a text request".into());
    };
    serde_json::from_str(&text).map_err(|_| "remote_stream_protocol: invalid request".into())
}

fn origin_and_host(headers: &HeaderMap, address: SocketAddr) -> Option<(String, String)> {
    if !address.ip().is_loopback() {
        return None;
    }
    let origin = headers.get("origin")?.to_str().ok()?;
    let host = headers.get("host")?.to_str().ok()?;
    Some((origin.to_owned(), host.to_owned()))
}

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/api/remote/native/stream", get(upgrade))
}

async fn upgrade(
    HttpState(state): HttpState<Arc<AppState>>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Extension(lifetime): Extension<ServerLifetime>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    let Some((origin, host)) = origin_and_host(&headers, address) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let Ok(hub) = hub(&state.pool, &state.peer_manager).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if hub
        .lock()
        .capture_stream
        .matching(&origin, &host, &state.media_token, Instant::now())
        .is_none()
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Tiny requests only. There is one encoder request and one bounded binary
    // reply in flight; a slow reader cannot build an unbounded frame queue.
    websocket
        .max_message_size(1024)
        .max_frame_size(1024)
        .max_write_buffer_size(MAX_FRAME + 128 * 1024)
        .on_upgrade(move |socket| serve(socket, hub, state, origin, host, lifetime))
}

type Writer = futures_util::stream::SplitSink<WebSocket, Message>;
type PendingFrame = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;

async fn send(writer: &mut Writer, message: Message) -> bool {
    tokio::time::timeout(SEND_TIMEOUT, writer.send(message))
        .await
        .is_ok_and(|result| result.is_ok())
}

async fn error(writer: &mut Writer, detail: &str) -> bool {
    let retryable = detail == "remote_capture_busy";
    let code = detail.split(':').next().unwrap_or("remote_stream_error");
    send(
        writer,
        Message::Text(
            serde_json::json!({
                "type":"error", "code":code, "error":detail, "retryable":retryable,
            })
            .to_string(),
        ),
    )
    .await
}

async fn toolbar_visible(state: &AppState, id: &str) -> Result<()> {
    #[cfg(all(
        feature = "desktop",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    {
        use tauri::Manager;

        let app = state
            .app_handle
            .clone()
            .ok_or("remote_stream_unavailable: native application missing")?;
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            // A Worker may renew an occluded owner's lease, but must not keep
            // capturing after the issuing native window has been destroyed.
            // Visibility/minimization of main is deliberately not a condition.
            if app.get_webview_window("main").is_none() {
                return Err("native owner window has been destroyed".into());
            }
            super::commands::visible_toolbar_handle(&app, &id)
        })
        .await
        .map_err(|error| format!("remote_state_changed: {error}"))?
        .map_err(|error| format!("remote_state_changed: {error}"))
    }
    #[cfg(not(all(
        feature = "desktop",
        not(any(target_os = "android", target_os = "ios"))
    )))]
    {
        let _ = (state, id);
        Err("remote_stream_unavailable: native capture is unavailable".into())
    }
}

async fn serve(
    socket: WebSocket,
    hub: Arc<Hub>,
    state: Arc<AppState>,
    origin: String,
    host: String,
    mut lifetime: ServerLifetime,
) {
    let (mut writer, mut reader) = socket.split();
    let first = tokio::select! {
        _ = lifetime.0.changed() => return,
        message = tokio::time::timeout(AUTH_TIMEOUT, reader.next()) => message,
    };
    let permit = match first {
        Ok(Some(Ok(message))) => match request(message) {
            Ok(Request::Auth { token }) if token.len() == 64 => {
                hub.claim_capture_stream(&token, &origin, &host, &state.media_token)
            }
            _ => Err("remote_stream_unauthorized: first request must authenticate".into()),
        },
        _ => Err("remote_stream_unauthorized: authentication timed out".into()),
    };
    let permit = match permit {
        Ok(permit) => permit,
        Err(detail) => {
            error(&mut writer, &detail).await;
            return;
        }
    };
    let lease = Lease { hub, permit };
    if let Err(detail) = toolbar_visible(&state, &lease.permit.scope.id).await {
        error(&mut writer, &detail).await;
        return;
    }
    if !send(
        &mut writer,
        Message::Text(
            serde_json::json!({
                "type":"ready", "format":"hevc-v1", "revision":lease.permit.scope.revision,
            })
            .to_string(),
        ),
    )
    .await
    {
        return;
    }

    let mut pending: Option<PendingFrame> = None;
    let mut check = tokio::time::interval(Duration::from_secs(1));
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = lifetime.0.changed() => break,
            _ = check.tick() => {
                let result = maintain(&mut lease.hub.lock(), &lease.permit, false);
                let result = match result {
                    Ok(()) => toolbar_visible(&state, &lease.permit.scope.id).await,
                    error => error,
                };
                if let Err(detail) = result {
                    error(&mut writer, &detail).await;
                    break;
                }
            }
            message = reader.next() => {
                let message = match message {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Ping(bytes))) => {
                        if !send(&mut writer, Message::Pong(bytes)).await { break; }
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => continue,
                    Some(Ok(message)) => message,
                };
                match request(message) {
                    Ok(Request::Stop) => break,
                    Ok(Request::Next { request_keyframe }) if pending.is_none() => {
                        let result = maintain(&mut lease.hub.lock(), &lease.permit, true);
                        if let Err(detail) = result {
                            error(&mut writer, &detail).await;
                            break;
                        }
                        let hub = lease.hub.clone();
                        let scope = lease.permit.scope.clone();
                        pending = Some(Box::pin(async move {
                            hub.frame(&scope.actor, &scope.id, scope.revision, Some("hevc-v1"), request_keyframe).await
                        }));
                    }
                    _ => {
                        error(&mut writer, "remote_stream_protocol: one frame request may be in flight").await;
                        break;
                    }
                }
            }
            result = async { pending.as_mut().unwrap().await }, if pending.is_some() => {
                pending = None;
                // Reissuing a capability, stopping, pausing or changing screen
                // during native encoding must discard that old result as well.
                let current = maintain(&mut lease.hub.lock(), &lease.permit, false);
                if let Err(detail) = current {
                    error(&mut writer, &detail).await;
                    break;
                }
                match result {
                    Ok(bytes) if bytes.len() <= MAX_FRAME => {
                        if !send(&mut writer, Message::Binary(bytes)).await { break; }
                    }
                    Ok(_) => {
                        error(&mut writer, "remote_stream_frame_limit: encoded frame exceeds limit").await;
                        break;
                    }
                    Err(detail) => {
                        let retryable = detail == "remote_capture_busy";
                        if !error(&mut writer, &detail).await || !retryable { break; }
                    }
                }
            }
        }
    }
    // Cancel queued work and revoke before the closing handshake. An already
    // running native call may finish, but its result has no receiver or sender.
    drop(pending.take());
    drop(lease);
    let _ = send(&mut writer, Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{
        model::{Invite, Mode, Screen, View},
        Actor, Session,
    };

    fn scope() -> Scope {
        Scope {
            actor: "owner".into(),
            id: "session".into(),
            revision: 4,
            origin: "http://tauri.localhost".into(),
            port: 18888,
            server_generation: "server-a".into(),
        }
    }

    fn claim(book: &mut TicketBook, token: &str, now: Instant) -> Result<Permit> {
        book.claim(
            token,
            "http://tauri.localhost",
            "127.0.0.1:18888",
            "server-a",
            now,
        )
    }

    #[test]
    fn ticket_is_single_use_short_lived_and_bound_to_native_origin_listener() {
        let now = Instant::now();
        let mut book = TicketBook::default();
        let token = book.issue(scope(), now);
        assert_eq!(token.len(), 64);
        for (origin, host, generation) in [
            ("https://evil.example", "127.0.0.1:18888", "server-a"),
            (
                "http://tauri.localhost",
                "rebound.example:18888",
                "server-a",
            ),
            ("http://tauri.localhost", "127.0.0.1:8888", "server-a"),
            ("http://tauri.localhost", "127.0.0.1:18888", "server-b"),
        ] {
            assert!(book.claim(&token, origin, host, generation, now).is_err());
        }
        assert!(claim(&mut book, &"f".repeat(64), now).is_err());
        let permit = claim(&mut book, &token, now).unwrap();
        assert!(claim(&mut book, &token, now).is_err());
        assert_eq!(book.active.as_deref(), Some(permit.token.as_str()));
        let expired = book.issue(scope(), now);
        assert!(claim(&mut book, &expired, now + TOKEN_TTL).is_err());
    }

    #[test]
    fn replacing_capability_revokes_old_frames_and_old_close_cannot_revoke_replacement() {
        let now = Instant::now();
        let mut book = TicketBook::default();
        let old = book.issue(scope(), now);
        claim(&mut book, &old, now).unwrap();
        let new = book.issue(scope(), now);
        assert!(book.active.is_none());
        claim(&mut book, &new, now).unwrap();
        book.revoke(&old);
        assert_eq!(book.active.as_deref(), Some(new.as_str()));
        book.revoke(&new);
        assert!(book.active.is_none());
        assert!(claim(&mut book, &new, now).is_err());
    }

    #[test]
    fn only_current_authorized_scope_renews_owner_and_expired_or_paused_scope_cannot_revive() {
        let scope = scope();
        let mut state = State::default();
        let mut view = View::new(
            scope.id.clone(),
            "peer".into(),
            "Peer".into(),
            true,
            Invite {
                mode: Mode::Help,
                note: String::new(),
                screen: Some(Screen {
                    id: "screen".into(),
                    name: "Display".into(),
                    width: 1920,
                    height: 1080,
                }),
                control: false,
                voice: false,
                native_host: true,
            },
        );
        view.accept(None, false, false, true).unwrap();
        view.revision = scope.revision;
        let mut session = Session::new(
            view,
            "s".repeat(64),
            Some(scope.actor.clone()),
            "127.0.0.1:9".into(),
        );
        let stale = Instant::now() - Duration::from_secs(3);
        session.owner_seen = stale;
        state.session = Some(session);
        state.actors.insert(
            scope.actor.clone(),
            Actor {
                native: true,
                seen: stale,
            },
        );
        let token = state.capture_stream.issue(scope, Instant::now());
        let permit = claim(&mut state.capture_stream, &token, Instant::now()).unwrap();
        maintain(&mut state, &permit, true).unwrap();
        assert!(state.session.as_ref().unwrap().owner_seen > stale);
        assert!(state.actors["owner"].seen > stale);

        let unchanged = state.session.as_ref().unwrap().owner_seen;
        state.session.as_mut().unwrap().view.revision += 1;
        assert!(maintain(&mut state, &permit, true)
            .unwrap_err()
            .starts_with("remote_revision_changed"));
        assert_eq!(state.session.as_ref().unwrap().owner_seen, unchanged);
        state.session.as_mut().unwrap().view.revision -= 1;
        state.session.as_mut().unwrap().view.paused = true;
        assert!(maintain(&mut state, &permit, true)
            .unwrap_err()
            .starts_with("remote_paused"));
        assert_eq!(state.session.as_ref().unwrap().owner_seen, unchanged);
        state.session.as_mut().unwrap().view.paused = false;
        let expired = Instant::now() - Duration::from_secs(9);
        state.session.as_mut().unwrap().owner_seen = expired;
        assert!(maintain(&mut state, &permit, true)
            .unwrap_err()
            .starts_with("remote_owner_expired"));
        assert_eq!(state.session.as_ref().unwrap().owner_seen, expired);
        state.session.as_mut().unwrap().view.stop("ended");
        assert!(maintain(&mut state, &permit, true)
            .unwrap_err()
            .starts_with("remote_session_ended"));
    }

    #[test]
    fn socket_transport_rejects_lan_peers_and_client_scope_overrides() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:18888".parse().unwrap());
        headers.insert("origin", "http://tauri.localhost".parse().unwrap());
        assert!(origin_and_host(&headers, "127.0.0.1:9000".parse().unwrap()).is_some());
        assert!(origin_and_host(&headers, "192.168.1.1:9000".parse().unwrap()).is_none());
        headers.remove("origin");
        assert!(origin_and_host(&headers, "127.0.0.1:9000".parse().unwrap()).is_none());
        assert!(request(Message::Text(r#"{"type":"next","actor":"other"}"#.into())).is_err());
        assert!(request(Message::Text(r#"{"type":"next","revision":5}"#.into())).is_err());
        assert!(matches!(
            request(Message::Text(
                r#"{"type":"next","request_keyframe":true}"#.into()
            )),
            Ok(Request::Next {
                request_keyframe: true
            })
        ));
    }
}
