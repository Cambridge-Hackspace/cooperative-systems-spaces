//! The MQTT service shares a runtime with the HTTP server, so it must never
//! block a thread.
//!
//! `MqttService::start` is spawned onto the same tokio runtime that serves
//! every HTTP request. It used to consume messages through
//! `AsyncClient::start_consuming()`, which hands back a *synchronous* channel,
//! and then called `rx.recv()` on it inside an `async fn`. That parks the OS
//! thread rather than the task. A parked thread cannot be preempted and its
//! work cannot be stolen by another worker, so the `sleep().await` written
//! directly underneath it was never reached while the receive was blocked.
//!
//! While the broker was healthy this merely wasted a worker. When the broker
//! went away it was fatal: paho's auto-reconnect churned, `recv()` stopped
//! returning, the runtime starved, and axum stopped calling `accept()`. The
//! process stayed up, the port stayed open, and the listening socket's
//! accept queue filled with connections nobody would ever answer -- a site
//! that is "running" and serving nothing. Tokio sizes its pool at one worker
//! per core, so on the 2-core host where this was found a single blocked
//! worker was half the runtime and the outage was immediate and total. On a
//! bigger developer machine the same bug reads as an intermittent stall, which
//! is why it survived so long.
//!
//! A text-level check on purpose: it needs no broker, no database and no
//! compiler, so it runs on the FreeBSD workstation where `css-server` cannot be
//! built. Reproducing the real failure needs a live broker to be killed
//! mid-flight, which no unit test can do -- but the defect is entirely visible
//! in the source, and it is a defect that gets reintroduced by copying an
//! example from the paho README.
//!
//! What this does NOT prove: that the service is correct, that it reconnects,
//! or that messages are handled. It proves the two specific constructs that
//! caused the outage are absent, and that the async ones that replaced them are
//! still there.

use css_checks::read;

fn mqtt_source() -> String {
    read("server/src/mqtt.rs")
}

/// Source with `//` line comments stripped.
///
/// Both directions matter here. The prose above and in `mqtt.rs` explains the
/// bug by naming the very constructs being banned, so an unstripped scan reads
/// the explanation as the offence. The mirror case is worse: a presence
/// assertion satisfied by a comment that merely mentions the call it wants.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_consumer_is_an_async_stream_not_a_blocking_channel() {
    let code = code_only(&mqtt_source());

    assert!(
        !code.contains("start_consuming()"),
        "server/src/mqtt.rs calls start_consuming(), which returns a synchronous \
         channel. Receiving from it blocks the tokio worker thread and starves \
         the HTTP server when the broker goes away. Use get_stream(), whose \
         receiver is awaited."
    );

    assert!(
        code.contains("get_stream("),
        "server/src/mqtt.rs no longer calls get_stream(). The MQTT consumer has \
         to be an async stream, otherwise receiving blocks a runtime worker."
    );
}

#[test]
fn every_receive_is_awaited() {
    let code = code_only(&mqtt_source());

    // The blocking form is `rx.recv()` with no `.await`. Catch it by looking
    // for a recv() that is not immediately awaited.
    for (n, line) in code.lines().enumerate() {
        if let Some(pos) = line.find(".recv()") {
            let rest = &line[pos + ".recv()".len()..];
            assert!(
                rest.trim_start().starts_with(".await"),
                "server/src/mqtt.rs:{}: `.recv()` is not awaited. On the \
                 synchronous receiver this parks the worker thread; even on the \
                 async one, a non-awaited receive is not doing what it looks \
                 like it is doing.\n  {}",
                n + 1,
                line.trim()
            );
        }
    }
}

#[test]
fn nothing_in_the_mqtt_service_blocks_on_a_token() {
    let code = code_only(&mqtt_source());

    // paho's Token is a Future. `.wait()` drives it to completion by blocking
    // the calling thread, which is exactly what must not happen on a runtime
    // shared with the HTTP server. connect/subscribe are awaited; publish hands
    // the token to the runtime instead.
    let offenders: Vec<_> = code
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(".wait()"))
        .map(|(n, l)| format!("  line {}: {}", n + 1, l.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "server/src/mqtt.rs blocks a thread waiting on a paho token. Await the \
         token (it is a Future) or hand it to the runtime; do not `.wait()` on \
         the runtime that serves HTTP.\n{}",
        offenders.join("\n")
    );
}

#[test]
fn a_reconnect_restores_the_subscriptions() {
    let code = code_only(&mqtt_source());

    // The connection is made with clean_session(true), so the broker throws our
    // subscriptions away when the link drops. A client that reconnects without
    // re-subscribing is connected and deaf: it looks healthy and silently stops
    // delivering heartbeats, device data and door events. The original loop
    // logged "connection lost" and slept, and never re-subscribed.
    assert!(
        code.contains("clean_session(true)"),
        "This check assumes clean_session(true); if the session is now \
         persistent the broker keeps subscriptions across a reconnect and this \
         check should be revisited rather than deleted."
    );

    let subscribe_calls = code.matches("subscribe_all(").count();
    assert!(
        subscribe_calls >= 2,
        "server/src/mqtt.rs calls subscribe_all() {} time(s). It has to run at \
         least twice -- once at startup and again after a reconnect -- because \
         clean_session(true) means the broker discards subscriptions when the \
         connection drops.",
        subscribe_calls
    );

    assert!(
        code.contains("is_connected()"),
        "server/src/mqtt.rs never checks is_connected(). After a dropped \
         connection it has to wait for the link to come back before \
         re-subscribing."
    );
}
