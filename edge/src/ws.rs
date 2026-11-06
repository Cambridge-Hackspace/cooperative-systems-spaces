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
use tokio_tungstenite::MaybeTlsStream;
use tracing::{debug, error, info, warn};

use crate::edge_inbound::EdgeInbound;
use crate::system_info::get_system_info;
use crate::transport_health::TransportHealth;

/// Cap on reconnect backoff (matches MQTT client's automatic reconnect cap).
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The outbound queue, shared rather than owned by the supervisor task. It has
/// to outlive that task: a supervisor that panics takes everything it owns with
/// it, and the whole point of the watchdog below is that a replacement can pick
/// the queue back up instead of the edge losing its upstream for good.
type Outbound = Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<WireMessage>>>;

/// What to do about a supervisor task that finished.
#[derive(Debug, PartialEq, Eq)]
enum SupervisorVerdict {
    Restart,
    Stop,
}

/// Split out of `supervise` so the policy can be driven in a unit test without
/// a server to connect to. The distinction that matters is the one #207 turned
/// on: a PANIC is not an `Err` from the supervisor's own point of view, it is an
/// `Err` from the JOIN HANDLE's -- so it is only visible to a caller that kept
/// the handle and awaited it.
fn verdict(outcome: &Result<(), tokio::task::JoinError>) -> SupervisorVerdict {
    match outcome {
        // `run` loops forever, so a clean return is itself a fault.
        Ok(()) => SupervisorVerdict::Restart,
        Err(e) if e.is_panic() => SupervisorVerdict::Restart,
        // Cancelled: the runtime is shutting down, not a fault to recover from.
        Err(_) => SupervisorVerdict::Stop,
    }
}

/// The TLS key exchange group this connection negotiated, where it can be seen.
///
/// Reported because the edge only *prefers* post-quantum: it offers hybrid
/// X25519+ML-KEM, and whatever terminates TLS in front of css-server may not
/// offer it, in which case the handshake quietly settles on classical X25519.
/// A plaintext `ws://` connection has no group at all, hence the Option.
fn negotiated_key_exchange(stream: &MaybeTlsStream<tokio::net::TcpStream>) -> Option<String> {
    match stream {
        MaybeTlsStream::Rustls(tls) => {
            let (_io, conn) = tls.get_ref();
            conn.negotiated_key_exchange_group()
                .map(|group| format!("{:?}", group.name()))
        }
        _ => None,
    }
}

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
    health: Arc<TransportHealth>,
}

impl WsClient {
    /// Build the client and spawn the connection-supervisor task. Returns an
    /// `Arc<Self>` so publish helpers can be invoked from elsewhere.
    pub fn start(
        remote_instance_url: &str,
        auth_token: String,
        inbound: Arc<EdgeInbound>,
        health: Arc<TransportHealth>,
    ) -> Result<Arc<Self>> {
        let connect_url = http_to_ws_url(remote_instance_url)
            .with_context(|| format!("invalid remote_instance_url: {remote_instance_url}"))?;
        let (outbound, outbound_rx) = mpsc::unbounded_channel::<WireMessage>();
        health.declare_tracked("websocket");
        let client = Arc::new(Self {
            inbound,
            outbound,
            connect_url,
            auth_token,
            start_time: std::time::Instant::now(),
            health,
        });
        let outbound_rx: Outbound = Arc::new(tokio::sync::Mutex::new(outbound_rx));
        let supervisor = client.clone();
        tokio::spawn(async move { supervisor.supervise(outbound_rx).await });
        Ok(client)
    }

    /// Watchdog over the reconnect supervisor.
    ///
    /// WHY A TASK WATCHING A TASK. `run` below already reconnects forever with
    /// backoff, and it was useless against #207, because a PANIC IS NOT AN
    /// `Err`: it unwound the supervisor task, so the `match` never saw it and
    /// the reconnect loop died with the task. The `JoinHandle` was dropped at
    /// the `spawn`, so nothing observed the death either -- the edge went on
    /// serving its local broker and its HTTP sync, showed offline in the portal,
    /// and said nothing above INFO about why. That silence was the defect that
    /// made a one-line dependency fault take three weeks to notice.
    ///
    /// So the handle is kept and awaited. `run` never returns, which makes a
    /// completed handle diagnostic in itself: `Err` is a panic, and even `Ok`
    /// means the loop stopped looping, which should never happen.
    ///
    /// It RESTARTS rather than exiting the process, deliberately. Exiting is the
    /// cleaner signal when something will restart the container, but that is a
    /// deployment property this code cannot see -- the edge runs under rootless
    /// podman with no restart policy guaranteed -- and turning a degraded edge
    /// into a dead one is the worse failure for a box that still answers scans.
    /// The restart count is reported through `/api/status` so a link that is
    /// only up because it keeps being resurrected does not read as healthy.
    async fn supervise(self: Arc<Self>, outbound: Outbound) {
        use SupervisorVerdict::{Restart, Stop};
        let mut backoff = Duration::from_secs(1);
        loop {
            let inner = self.clone();
            let queue = outbound.clone();
            let outcome = tokio::spawn(async move { inner.run(queue).await }).await;
            match verdict(&outcome) {
                Restart => match &outcome {
                    Ok(()) => error!("WebSocket supervisor returned, which it never should"),
                    Err(e) => error!("WebSocket supervisor PANICKED: {e}"),
                },
                Stop => {
                    // The runtime is going away; a replacement would only die too.
                    warn!("WebSocket supervisor cancelled; not restarting");
                    self.health.mark_disconnected();
                    return;
                }
            }
            let n = self.health.note_supervisor_restart();
            error!(
                "WebSocket transport is DOWN; restarting its supervisor in {:?} (restart #{})",
                backoff, n
            );
            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
        }
    }

    /// Internal supervisor loop: connect, run the read+write halves until
    /// either side errors, then reconnect with exponential backoff.
    async fn run(self: Arc<Self>, outbound: Outbound) {
        let mut backoff = Duration::from_secs(1);
        loop {
            let outcome = self.connect_once(&outbound).await;
            // Whatever ended the session, the link is down and must stop
            // reporting a negotiated group -- a stale one reads as present tense.
            self.health.mark_disconnected();
            match outcome {
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
    async fn connect_once(self: &Arc<Self>, outbound: &Outbound) -> Result<()> {
        let mut outbound_rx = outbound.lock().await;
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
        let key_exchange = negotiated_key_exchange(stream.get_ref());
        match &key_exchange {
            Some(group) => info!("WebSocket: TLS key exchange {group}"),
            None => info!("WebSocket: no TLS layer to report a key exchange for"),
        }
        self.health.mark_connected(key_exchange);

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

    /// What this does NOT prove: that `supervise` wires the verdict up
    /// correctly, which needs a reachable server. It proves the decision, and
    /// that a panicking task is distinguishable from a cancelled one at all --
    /// the distinction the old code could not make because it never held the
    /// handle.
    #[tokio::test]
    async fn a_panicking_supervisor_is_restarted() {
        let outcome = tokio::spawn(async { panic!("the provider is missing") }).await;
        let err = outcome
            .as_ref()
            .expect_err("a panicking task reported success");
        assert!(err.is_panic(), "a panic must be visible as a panic");
        assert_eq!(verdict(&outcome), SupervisorVerdict::Restart);
    }

    #[tokio::test]
    async fn a_supervisor_that_returns_is_also_restarted() {
        // `run` is an infinite loop; returning means it stopped looping.
        let outcome = tokio::spawn(async {}).await;
        assert!(outcome.is_ok());
        assert_eq!(verdict(&outcome), SupervisorVerdict::Restart);
    }

    #[tokio::test]
    async fn a_cancelled_supervisor_is_left_alone() {
        let handle = tokio::spawn(async { std::future::pending::<()>().await });
        handle.abort();
        let outcome = handle.await;
        let err = outcome
            .as_ref()
            .expect_err("an aborted task reported success");
        assert!(err.is_cancelled());
        assert_eq!(
            verdict(&outcome),
            SupervisorVerdict::Stop,
            "respawning into a dying runtime is a restart loop, not a recovery"
        );
    }
}
