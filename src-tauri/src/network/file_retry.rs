//! Retry only transport failures and explicit temporary HTTP responses.
use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

pub(super) const ATTEMPTS: usize = 3;
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct RequestError {
    pub detail: String,
    pub retryable: bool,
}

impl RequestError {
    pub fn permanent(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            retryable: false,
        }
    }

    pub fn transient(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            retryable: true,
        }
    }

    pub fn transport(error: reqwest::Error) -> Self {
        Self {
            retryable: error.is_connect()
                || error.is_timeout()
                || (error.is_request() && !error.is_body() && !error.is_builder()),
            detail: format!("文件传输网络请求失败: {error}"),
        }
    }
}

pub(super) fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 502 | 503 | 504)
}

pub(super) fn backoff(attempt: usize) -> Duration {
    Duration::from_millis(250 * (1u64 << attempt.min(3)))
}

pub(super) async fn decode<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, RequestError> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(RequestError::permanent("文件传输控制响应超过 64 KiB"));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|error| RequestError::transient(format!("读取传输响应失败: {error}")))?;
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err(RequestError::permanent("文件传输控制响应超过 64 KiB"));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(RequestError {
            detail: format!(
                "接收端拒绝传输 ({status}): {}",
                String::from_utf8_lossy(&body)
                    .chars()
                    .take(512)
                    .collect::<String>()
            ),
            retryable: retryable_status(status),
        });
    }
    serde_json::from_slice(&body)
        .map_err(|error| RequestError::permanent(format!("解析传输响应失败: {error}")))
}

pub(super) async fn post_once<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    request: &impl Serialize,
) -> Result<T, RequestError> {
    let response = client
        .post(url)
        .json(request)
        .timeout(RESPONSE_TIMEOUT)
        .send()
        .await
        .map_err(RequestError::transport)?;
    decode(response).await
}

pub(super) async fn post<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    request: &impl Serialize,
) -> Result<T, String> {
    for attempt in 0..ATTEMPTS {
        match post_once(client, url, request).await {
            Ok(value) => return Ok(value),
            Err(error) if error.retryable && attempt + 1 < ATTEMPTS => {
                tokio::time::sleep(backoff(attempt)).await;
            }
            Err(error) => return Err(error.detail),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_fixed_and_chunked_responses_are_rejected_without_retry() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let router = axum::Router::new().route(
            "/:mode",
            axum::routing::post(
                move |axum::extract::Path(mode): axum::extract::Path<String>| {
                    let counted = counted.clone();
                    async move {
                        counted.fetch_add(1, Ordering::SeqCst);
                        if mode == "fixed" {
                            return axum::response::Response::new(axum::body::Body::from(
                                vec![b'x'; MAX_RESPONSE_BYTES + 1],
                            ));
                        }
                        let body = futures_util::stream::iter(
                            (0..17).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 4096])),
                        );
                        axum::response::Response::builder()
                            .status(if mode == "busy" { 503 } else { 200 })
                            .body(axum::body::Body::from_stream(body))
                            .unwrap()
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::new();
        for mode in ["fixed", "chunked", "busy"] {
            let error = post::<serde_json::Value>(&client, &format!("{base}/{mode}"), &())
                .await
                .unwrap_err();
            assert!(error.contains("64 KiB"), "{mode}: {error}");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        server.abort();
    }

    #[test]
    fn disk_identity_protocol_and_validation_errors_are_not_retried() {
        for code in [400, 401, 403, 404, 409, 413, 422, 500, 507] {
            assert!(!retryable_status(
                reqwest::StatusCode::from_u16(code).unwrap()
            ));
        }
        for code in [408, 429, 502, 503, 504] {
            assert!(retryable_status(
                reqwest::StatusCode::from_u16(code).unwrap()
            ));
        }
    }

    #[tokio::test]
    async fn completion_confirmation_retries_are_bounded_and_idempotent() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let router = axum::Router::new()
            .route(
                "/complete",
                axum::routing::post(move || {
                    let count = count.clone();
                    async move {
                        let status = if count.fetch_add(1, Ordering::SeqCst) == 0 {
                            axum::http::StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            axum::http::StatusCode::OK
                        };
                        (
                            status,
                            axum::Json(serde_json::json!({"status": "already_exists"})),
                        )
                    }
                }),
            )
            .route(
                "/busy",
                axum::routing::post(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::new();
        let reply: serde_json::Value = post(&client, &format!("{base}/complete"), &())
            .await
            .unwrap();
        assert_eq!(reply["status"], "already_exists");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            post::<serde_json::Value>(&client, &format!("{base}/busy"), &())
                .await
                .unwrap_err()
                .contains("503")
        );
        server.abort();
    }
}
