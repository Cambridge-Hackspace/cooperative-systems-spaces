# MQTT broker security (REQUIRED in production)

The server↔edge control plane runs over MQTT. The application code hands each
device **its own** broker credentials (#120 / #121): the MQTT username is the
device's UUID and the password is its auth token, returned once at registration
(`server/src/api/devices.rs`). It never hands out the server's own credential.

That is only half of the fix. The credential is worthless unless the **broker**
authenticates it and restricts what each device may do. The broker configuration
below is a **mandatory operational step** — the shipped `server/mosquitto*.conf`
files set `allow_anonymous true` for local development and e2e only, and **must
not be used in production**.

Without this configuration, anyone who can reach the broker can:

- enumerate device UUIDs from heartbeat topics,
- publish `{"door_id":…,"duration_ms":…}` to any device's `doors/unlock` topic
  and open **any** door with no user credential,
- publish forged `doors/event` payloads that land in the audit trail as genuine
  `DoorUnlockedCard` events, poisoning the system's evidentiary record.

## 1. Require authentication (no anonymous access)

```
allow_anonymous false
```

Authenticate the per-device credentials the server issues. The recommended path
is an HTTP auth backend (e.g. `mosquitto-go-auth`) that verifies
`(username = device UUID, password = auth token)` against the server — the
server already resolves a device by its token
(`DatabaseManager::find_device_by_auth_token`, which hashes the presented token
and matches the stored SHA-256, #120/#14), so the broker never needs the token
at rest. A static password file is acceptable for a small deployment but must be
regenerated whenever a device is re-provisioned.

## 2. Per-device topic ACLs

A device may publish and subscribe **only** under its own UUID. With
`mosquitto-go-auth` (or an ACL file), grant each authenticated device:

```
# %u is the authenticated username = the device UUID; %c/pattern scopes by it.
pattern readwrite cs/spaces/devices/%u/#
```

and nothing else. This is what actually stops one compromised controller from
driving or impersonating the rest of the fleet: it cannot publish to another
device's `doors/unlock`, and it cannot forge another device's `doors/event`.

## 3. TLS

Terminate the broker over TLS so per-device credentials are not sent in the
clear and traffic cannot be sniffed or tampered with in transit:

```
listener 8883
cafile   /etc/mosquitto/certs/ca.crt
certfile /etc/mosquitto/certs/server.crt
keyfile  /etc/mosquitto/certs/server.key
require_certificate false   # password auth over TLS; see below for mTLS
```

Point `edge.edge_mqtt_config.mqtt_instance_url` at the TLS listener
(`mqtts://…`).

## 4. Defense-in-depth: signed commands (recommended follow-up)

Broker auth + ACLs (steps 1–3) are the primary control and close the finding's
attacks. As defence-in-depth for a compromised or misconfigured broker, the
`doors/unlock` command and inbound `doors/event` can additionally be
HMAC-signed with a per-device signing key established at registration, so the
edge/server rejects any message not signed by the expected peer. This needs a
per-device signing key kept alongside the device record and is tracked
separately; it is not a substitute for the broker configuration above.
