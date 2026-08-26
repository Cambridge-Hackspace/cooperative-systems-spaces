//! WebSocket transport between the edge client and the css-server. This is
//! a peer of [`crate::mqtt::EdgeMqttClient`]; only one is constructed at a
//! time based on `Config::remote_transport`.
//!
//! Connect URL is derived from `remote_instance_url` by rewriting
//! `http(s)://…` to `ws(s)://…/api/devices/ws`. Auth is the same
//! `Bearer <auth_token>` the existing HTTP endpoints use.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use css_lib::wire::{kinds, WireMessage};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, info, warn};

use crate::edge_inbound::EdgeInbound;
use crate::system_info::get_system_info;

/// Cap on reconnect backoff (matches MQTT client's automatic reconnect cap).
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct WsClient {
    inbound: Arc<EdgeInbound>,
    /// Per-process channel sender consumed by the writer task whenever a
    /// connection is open. While disconnected, sends still succeed (just
    /// buffer); they get drained on reconnect via the supervisor below.
    outbound: mpsc::UnboundedSender<WireMessage>,
    /// URL to connect to (already rewritten to `ws(s)://...`).
    connect_url: String,
    auth_token: String,
    start_time: std::time::Instant,
}

impl WsClient {
    /// Build the client and spawn the connection-supervisor task. Returns an
    /// `Arc<Self>` so publish helpers can be invoked from elsewhere.
    pub fn start(
        remote_instance_url: &str,
        auth_token: String,
        inbound: Arc<EdgeInbound>,
    ) -> Result<Arc<Self>> {
        let connect_url = http_to_ws_url(remote_instance_url)
            .with_context(|| format!("invalid remote_instance_url: {remote_instance_url}"))?;
        let (outbound, outbound_rx) = mpsc::unbounded_channel::<WireMessage>();
        let client = Arc::new(Self {
            inbound,
            outbound,
            connect_url,
            auth_token,
            start_time: std::time::Instant::now(),
        });
        let supervisor = client.clone();
        tokio::spawn(async move { supervisor.run(outbound_rx).await });
        Ok(client)
    }

    /// Internal supervisor loop: connect, run the read+write halves until
    /// either side errors, then reconnect with exponential backoff.
    async fn run(self: Arc<Self>, mut outbound_rx: mpsc::UnboundedReceiver<WireMessage>) {
        let mut backoff = Duration::from_secs(1);
        loop {
            match self.connect_once(&mut outbound_rx).await {
                Ok(()) => {
                    info!("WebSocket session ended cleanly; reconnecting");
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    warn!(
                        "WebSocket session failed ({}); reconnecting in {:?}",
                        e, backoff
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                }
            }
        }
    }

    /// One connection lifetime. Returns `Ok(())` on clean close, `Err` on
    /// any failure that should trigger a reconnect.
    async fn connect_once(
        self: &Arc<Self>,
        outbound_rx: &mut mpsc::UnboundedReceiver<WireMessage>,
    ) -> Result<()> {
        let mut req = self
            .connect_url
            .as_str()
            .into_client_request()
            .context("Failed to build WebSocket client request")?;
        req.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {}", self.auth_token)
                .parse()
                .context("Bearer header invalid")?,
        );

        info!("WebSocket: connecting to {}", self.connect_url);
        let (stream, response) = tokio_tungstenite::connect_async(req)
            .await
            .context("WebSocket connect failed")?;
        debug!("WebSocket: upgraded ({:?})", response.status());

        let (mut sink, mut source) = stream.split();

        // Send an initial heartbeat + device data so the server sees us.
        let _ = sink
            .send(Message::Text(
                serde_json::to_string(&WireMessage::new(kinds::HEARTBEAT, json!(null)))?.into(),
            ))
            .await;
        if let Ok(info) = get_system_info() {
            let data = json!({
                "mac_address": info.mac_address,
                "software_version": env!("CARGO_PKG_VERSION"),
                "ipv4_address": info.ipv4_address,
                "ipv6_address": info.ipv6_address,
                "uptime": self.start_time.elapsed().as_secs(),
                "platform": info.platform,
            });
            let _ = sink
                .send(Message::Text(
                    serde_json::to_string(&WireMessage::new(kinds::DATA, data))?.into(),
                ))
                .await;
        }

        let mut heartbeat = interval(Duration::from_secs(15));
        heartbeat.tick().await; // skip immediate first tick (we already sent one)

        loop {
            tokio::select! {
                maybe_in = source.next() => {
                    let msg = match maybe_in {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => return Err(anyhow::anyhow!("read error: {e}")),
                        None => return Ok(()),
                    };
                    match msg {
                        Message::Text(t) => {
                            match serde_json::from_str::<WireMessage>(&t) {
                                Ok(wm) => {
                                    // Re-serialize the payload to bytes so we can
                                    // reuse the byte-oriented handler shapes.
                                    let bytes = wm.payload.to_string().into_bytes();
                                    self.inbound.dispatch(&wm.kind, &bytes).await;
                                }
                                Err(e) => warn!("Invalid WireMessage from server: {}", e),
                            }
                        }
                        Message::Binary(b) => {
                            match serde_json::from_slice::<WireMessage>(&b) {
                                Ok(wm) => {
                                    let bytes = wm.payload.to_string().into_bytes();
                                    self.inbound.dispatch(&wm.kind, &bytes).await;
                                }
                                Err(e) => warn!("Invalid binary WireMessage from server: {}", e),
                            }
                        }
                        Message::Ping(p) => {
                            if sink.send(Message::Pong(p)).await.is_err() {
                                return Err(anyhow::anyhow!("failed to send pong"));
                            }
                        }
                        Message::Pong(_) => {}
                        Message::Close(_) => return Ok(()),
                        Message::Frame(_) => {}
                    }
                }
                maybe_out = outbound_rx.recv() => {
                    let wm = match maybe_out {
                        Some(m) => m,
                        None => return Ok(()), // sender dropped — process shutdown
                    };
                    let text = serde_json::to_string(&wm)
                        .context("serialize outbound WireMessage")?;
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        return Err(anyhow::anyhow!("write error"));
                    }
                }
                _ = heartbeat.tick() => {
                    let wm = WireMessage::new(kinds::HEARTBEAT, json!(null));
                    let text = serde_json::to_string(&wm)?;
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        return Err(anyhow::anyhow!("heartbeat write error"));
                    }
                }
            }
        }
    }

    // ---- public publish helpers (used by main + bridge tasks) ------------

    pub fn publish(&self, wm: WireMessage) {
        let _ = self.outbound.send(wm);
    }

    pub fn publish_doors_event(&self, event: &crate::doors::DoorsEvent) -> Result<()> {
        let payload = serde_json::to_value(event).context("serialize DoorsEvent")?;
        self.publish(WireMessage::new(kinds::DOORS_EVENT, payload));
        Ok(())
    }
}

/// Rewrite an HTTP base URL to its WebSocket equivalent and append the
/// device WS path.
fn http_to_ws_url(base: &str) -> Result<String> {
    let trimmed = base.trim_end_matches('/');
    let ws = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else {
        anyhow::bail!("URL must start with http://, https://, ws:// or wss://");
    };
    Ok(format!("{ws}/api/devices/ws"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_rewrites_to_wss() {
        assert_eq!(
            http_to_ws_url("https://example.org").unwrap(),
            "wss://example.org/api/devices/ws"
        );
    }

    #[test]
    fn http_rewrites_to_ws() {
        assert_eq!(
            http_to_ws_url("http://localhost:4399/").unwrap(),
            "ws://localhost:4399/api/devices/ws"
        );
    }

    #[test]
    fn ws_passthrough() {
        assert_eq!(
            http_to_ws_url("ws://broker:9001").unwrap(),
            "ws://broker:9001/api/devices/ws"
        );
    }

    #[test]
    fn bad_scheme_rejected() {
        assert!(http_to_ws_url("mqtt://x").is_err());
    }
}
