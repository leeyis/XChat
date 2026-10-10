//! WebView2's public shared-memory bridge. Pixels never traverse its HTTP-shaped
//! IPC response. Each request owns one immutable mapping until JS releases it.
use super::{Hub, Result};
use std::sync::Arc;
use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17,
    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
};
use windows_core_061::{Interface, HSTRING, PWSTR};

struct Mapping(ICoreWebView2SharedBuffer);

impl Drop for Mapping {
    fn drop(&mut self) {
        // The renderer has an independent view after PostSharedBufferToScript.
        // Closing our handle cannot detach that view; JS must releaseBuffer too.
        unsafe {
            let _ = self.0.Close();
        }
    }
}

pub async fn publish(
    window: &WebviewWindow,
    hub: Arc<Hub>,
    actor: String,
    id: String,
    revision: u64,
    request_id: String,
    expected_url: String,
    bytes: Vec<u8>,
) -> Result<Vec<u8>> {
    // Unchanged requests already have only a small binary header.
    if bytes.len() <= 44 {
        return Ok(bytes);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |webview| {
            let result = (|| -> Result<Vec<u8>> {
                // Recheck on the UI thread immediately before disclosing a frame:
                // a pause/revoke/navigation may happen while the callback queues.
                let state = hub.lock();
                let session = Hub::native_session(&state, &actor, &id)
                    .map_err(|error| format!("remote_state_changed: {error}"))?;
                if session.view.revision != revision {
                    return Err("remote_state_changed: 画面已过期".into());
                }
                unsafe {
                    let core = webview
                        .controller()
                        .CoreWebView2()
                        .map_err(|e| e.to_string())?;
                    let mut source = PWSTR::null();
                    core.Source(&mut source).map_err(|e| e.to_string())?;
                    let source = webview2_com::take_pwstr(source);
                    if source != expected_url {
                        return Err("remote_state_changed: 共享页面已导航".into());
                    }
                    let environment = webview.environment().cast::<ICoreWebView2Environment12>();
                    let target = core.cast::<ICoreWebView2_17>();
                    let (environment, target) = match (environment, target) {
                        (Ok(environment), Ok(target)) => (environment, target),
                        // Only an unavailable interface may fall back to raw IPC.
                        (Err(error), _) | (_, Err(error)) if error.code().0 == -2147467262 => {
                            return Ok(bytes);
                        }
                        (Err(error), _) | (_, Err(error)) => return Err(error.to_string()),
                    };
                    let mapping = Mapping(
                        environment
                            .CreateSharedBuffer(bytes.len() as u64)
                            .map_err(|e| e.to_string())?,
                    );
                    let mut destination = std::ptr::null_mut();
                    mapping
                        .0
                        .Buffer(&mut destination)
                        .map_err(|e| e.to_string())?;
                    if destination.is_null() {
                        return Err("共享屏幕帧映射不可用".into());
                    }
                    // Own the mapping on this UI thread; the read-only renderer
                    // cannot observe it until the complete copy has finished.
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
                    let sequence = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
                    let metadata = HSTRING::from(
                        serde_json::json!({
                            "type": "xchat-frame-v1",
                            "requestId": request_id,
                            "sessionId": id,
                            "revision": revision.to_string(),
                            "sequence": sequence.to_string(),
                        })
                        .to_string(),
                    );
                    target
                        .PostSharedBufferToScript(
                            &mapping.0,
                            COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                            &metadata,
                        )
                        .map_err(|e| e.to_string())?;
                    let mut ack = b"XRS1".to_vec();
                    ack.extend_from_slice(&sequence.to_le_bytes());
                    Ok(ack)
                }
            })();
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}
