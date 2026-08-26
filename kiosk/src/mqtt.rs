use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use paho_mqtt as mqtt;

use crate::calendar::CalEvent;
use crate::toolguard::SyncPayload;

const KIOSK_REFRESH_TOPIC: &str = "kiosk/refresh";

pub enum MqttEvent {
    Connected,
    Disconnected,
    ToolGuardState(SyncPayload),
    CalendarEvents(Vec<CalEvent>),
    Error(String),
}

pub fn spawn_mqtt_thread(
    broker_url: String,
    client_id: String,
    toolguard_topic: String,
    calendar_topic: Option<String>,
) -> Receiver<MqttEvent> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        run_subscriber(broker_url, client_id, toolguard_topic, calendar_topic, tx);
    });
    rx
}

fn subscribe_all(client: &mqtt::Client, toolguard_topic: &str, calendar_topic: Option<&str>) {
    let _ = client.subscribe(toolguard_topic, 1);
    if let Some(t) = calendar_topic {
        let _ = client.subscribe(t, 1);
    }
}

fn run_subscriber(
    broker_url: String,
    client_id: String,
    toolguard_topic: String,
    calendar_topic: Option<String>,
    tx: Sender<MqttEvent>,
) {
    let create_opts = mqtt::CreateOptionsBuilder::new()
        .server_uri(&broker_url)
        .client_id(&client_id)
        .finalize();

    let client = match mqtt::Client::new(create_opts) {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(MqttEvent::Error(format!(
                "Failed to create MQTT client: {e}"
            )));
            return;
        }
    };

    let conn_opts = mqtt::ConnectOptionsBuilder::new()
        .keep_alive_interval(Duration::from_secs(30))
        .clean_session(true)
        .automatic_reconnect(Duration::from_secs(1), Duration::from_secs(30))
        .finalize();

    if let Err(e) = client.connect(conn_opts) {
        let _ = tx.send(MqttEvent::Error(format!("MQTT connect failed: {e}")));
        return;
    }

    let _ = tx.send(MqttEvent::Connected);
    subscribe_all(&client, &toolguard_topic, calendar_topic.as_deref());
    let _ = client.publish(mqtt::Message::new(KIOSK_REFRESH_TOPIC, b"1".as_ref(), 0));

    let receiver = client.start_consuming();

    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(msg)) => {
                let topic = msg.topic();
                if topic == toolguard_topic {
                    match serde_json::from_slice::<SyncPayload>(msg.payload()) {
                        Ok(payload) => {
                            let _ = tx.send(MqttEvent::ToolGuardState(payload));
                        }
                        Err(e) => {
                            let _ =
                                tx.send(MqttEvent::Error(format!("ToolGuard parse error: {e}")));
                        }
                    }
                } else if calendar_topic.as_deref() == Some(topic) {
                    match serde_json::from_slice::<Vec<CalEvent>>(msg.payload()) {
                        Ok(events) => {
                            let _ = tx.send(MqttEvent::CalendarEvents(events));
                        }
                        Err(e) => {
                            let _ = tx.send(MqttEvent::Error(format!("Calendar parse error: {e}")));
                        }
                    }
                }
            }
            Ok(None) => {
                let _ = tx.send(MqttEvent::Disconnected);
                if let Err(e) = client.reconnect() {
                    let _ = tx.send(MqttEvent::Error(format!("Reconnect failed: {e}")));
                } else {
                    let _ = tx.send(MqttEvent::Connected);
                    subscribe_all(&client, &toolguard_topic, calendar_topic.as_deref());
                    let _ =
                        client.publish(mqtt::Message::new(KIOSK_REFRESH_TOPIC, b"1".as_ref(), 0));
                }
            }
            Err(e) if e.is_timeout() => continue,
            Err(_) => break,
        }
    }
}
