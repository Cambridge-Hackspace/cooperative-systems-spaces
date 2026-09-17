# Writing firmware for a CSS device

This is the wire contract between a device — a ToolGuard, a card reader, a power
controller, a sensor, an edge coordinator, a kiosk — and a CSS server. It exists
so that implementing firmware does not require reading `server/src/api/toolguard.rs`
and guessing.

Two things you need, and this document covers both:

- **What to implement** — everything below.
- **Somewhere to test it** — [a live instance to develop against](#a-server-to-develop-against),
  seeded with a tool, a card and an invite so you can swipe something on minute one.

> **This document is checked against the code.**
> `checks/tests/the_firmware_protocol_is_documented.rs` asserts that every
> endpoint and every MQTT topic named here exists, **and** that every
> device-facing route the server exposes is named here. It fails the build in
> both directions, so this file cannot quietly become fiction and a new endpoint
> cannot land undocumented. If something here looks wrong, it is a bug worth
> reporting rather than a stale page to work around.

---

## Contents

- [Concepts](#concepts)
- [Authentication](#authentication)
- [Registration](#registration)
- [HTTP endpoints](#http-endpoints)
- [MQTT](#mqtt)
- [The lifecycle](#the-lifecycle)
- [Failure semantics](#failure-semantics)
- [Forward compatibility](#forward-compatibility)
- [A server to develop against](#a-server-to-develop-against)

---

## Concepts

**Device.** Anything that registers and holds a token. Five kinds, all
registering through the same endpoint: `edge`, `kiosk`, `card_reader`,
`power_controller`, `sensor`.

**Tool.** A machine the space controls access to. A tool has a UUID and,
usually, an `external_id` — the short string your firmware is configured with
and sends as `tool_id`. Both are accepted wherever a tool is named; the
`external_id` is matched first.

**Card.** Whatever a reader reads. The server resolves it against a configurable
user profile field (`toolguard.profile_field`, typically `card_id`), so "card"
may be an RFID UID, a fob number, or a PIN depending on the deployment. Send
what you read; do not try to interpret it.

**Module.** A device filling one role in a tool's access chain: `reader`,
`power`, or `sensor`. The modules for one tool are typically *not* wired to each
other — the edge coordinator is what connects them.

**Edge.** The coordinator. It holds the cached allow-list, evaluates interlocks
across modules, and holds the lease. If you are writing module firmware, the
edge is your counterparty as much as the server is.

---

## Authentication

Every device-facing endpoint accepts **one of two** credentials.

### 1. A device Bearer token

```http
Authorization: Bearer <auth_token>
```

Obtained once, at [registration](#registration). This is the preferred
credential and the only one accepted by `sync` and `boot-reset`.

### 2. An API key

Passed as a query parameter or a body field named `api_key`, depending on the
endpoint. Two keys are accepted:

- the tool's own `external_api_key`, or
- the server's configured global key (`toolguard.global_api_key`).

**Metered tools are the exception, and it matters.** A tool that bills for usage
must authenticate with its **own** `external_api_key`. The global key is not
accepted, and neither is a bare device token. This binds a billable report to
that specific tool's secret, so one leaked global key cannot post charges for
every tool in the building. A metered tool with no key configured can never
satisfy this and is refused rather than trusted — an unbillable, forgeable
metered tool is a worse outcome than an unusable one.

### What the failures look like

| condition | response |
|---|---|
| No credential, or a bad one | **401**, `ToolGuardResponse` with `status: "error"` |
| A credential, but the request is refused | **200**, see [denials](#denials-are-200-not-4xx) |
| Server or database fault | **500** |

A database fault is deliberately *not* folded into "not authenticated". If the
server answers 500, your credentials are probably fine and something else is
wrong — do not respond by re-registering.

---

## Registration

A device is claimed with a single-use invite code issued by an administrator.

```
POST /api/devices/register
Content-Type: application/json
```

```json
{
  "device_code": "<invite code from an administrator>",
  "name": "laser-cutter-guard",
  "kind": "power_controller",
  "mac_address": "02:00:00:00:00:01",
  "software_version": "1.2.3",
  "platform": "linux",
  "ipv4_address": "192.168.1.50",
  "ipv6_address": null
}
```

`kind` must be one of `edge`, `kiosk`, `card_reader`, `power_controller`,
`sensor` (case-insensitive). `platform` must be one of `windows`, `linux`,
`macos`, `other`. Anything else is a **400**, as is an expired or already-claimed
code. `ipv4_address` and `ipv6_address` are optional.

The response is wrapped in the standard API envelope:

```json
{
  "success": true,
  "data": {
    "device_id": "…uuid…",
    "auth_token": "…uuid…",
    "device_name": "laser-cutter-guard",
    "mqtt_config": {
      "mqtt_instance_url": "tcp://broker.example:1883",
      "mqtt_username": null,
      "mqtt_password": null,
      "mqtt_namespace": "cs/spaces"
    }
  },
  "error": null
}
```

**Persist `device_id`, `auth_token` and the whole `mqtt_config` to non-volatile
storage.** The invite is single-use: a device that loses its token needs a new
invite from an administrator, not a retry. `mqtt_config` may be `null` if the
deployment does not use MQTT.

The registration endpoint is public by design — the invite code is the
credential. Treat the code as a secret in transit.

---

## HTTP endpoints

Paths are written in full. Every device-facing route the server exposes is
documented below; the oracle named at the top of this file enforces that.

### The common response envelope

Most endpoints return `ToolGuardResponse`:

```json
{
  "status": "ok" | "error",
  "message": "human-readable, may be absent",
  "tool_on":  true | false,
  "tool_off": true
}
```

`message`, `tool_on` and `tool_off` are omitted rather than null when they do not
apply. **Branch on `status` and `tool_on`, never on `message`** — message text is
for humans and changes.

### Denials are 200, not 4xx

This is the single most common firmware mistake, so it gets its own heading.

A *denial* — unknown card, untrained user, tool already in use, tool broken — is
a successful request with an unsuccessful answer. It returns **HTTP 200** with:

```json
{ "status": "error", "message": "Training required", "tool_on": false }
```

Firmware that only checks the HTTP status code will energize a tool for a user
who is not allowed to use it. **`tool_on: true` is the only thing that means
"energize".** Absence of `tool_on` is not permission.

### `GET /api/toolguard` — status probe

No authentication. Returns `{"status":"ok"}`. Use it for reachability checks.

### `GET /api/toolguard/tool-on`

Query: `card`, `tool_id`, optional `api_key`.

Authorizes a card against a tool and, if allowed, marks the tool in use.

Returns `tool_on: true` on success. Denial reasons observed in the field, all
with `tool_on: false`:

| `message` | meaning |
|---|---|
| `Unknown card` | the card resolves to no user |
| `Card not authorized` | the card is known but disabled or released |
| `User is not active` | membership lapsed |
| `Tool not found` | no tool with that `tool_id` |
| `Tool is already in use` | another session is open |
| `Tool is under maintenance` | status is Maintenance |
| `Tool is broken` | status is Broken |
| `Tool is in repair` | status is Repair |
| `Tool is retired` | status is Retired |
| `Training required` | the user is not authorized for this specific tool |
| `Metered tool requires its own API key` | see [authentication](#authentication) |
| *(billing-specific text)* | metered tool, funds or holds unavailable |

### `GET /api/toolguard/tool-off`

Query: `card`, `tool_id`, optional `api_key`.

Ends the session and returns the tool to Idle. Returns `tool_off: true`.

Unlike `tool-on`, this endpoint answers `status: "error"` with **no** `tool_off`
field for an unknown card, a missing tool, or a bad key. Send it anyway on every
stop — an un-ended session leaves the tool unusable for the next member.

### `GET /api/toolguard/tool-log`

Query: `card`, `tool_id`, `seconds` (float), optional `temperature` (float),
optional `api_key`.

Reports usage. `message` is `Usage logged`. For metered tools this is what gets
billed; send it before `tool-off`.

### `GET /api/toolguard/sync` — the allow-list

**Device Bearer token only.** Returns the cached authorization state for this
device:

```json
{
  "device_id": "…uuid…",
  "profile_field": "card_id",
  "tools": [
    { "id": "…uuid…", "external_id": "laser-01", "name": "CO₂ Laser",
      "status": "Idle", "requires_online": false }
  ],
  "users": [
    { "profile_field_value": "04A2B3C4", "full_name": "A Member",
      "is_active": true, "authorized_tool_ids": ["…uuid…"] }
  ]
}
```

Cache this. It is what lets a device keep working when the server is
unreachable — **except** where `requires_online` is `true`. A metered tool under
`actuation_mode = OnlineSynchronous` must not be energized from the cache; call
the server first, and refuse if you cannot. Billing that cannot be recorded must
not happen.

### `POST /api/toolguard/boot-reset`

**Device Bearer token only.** No body required.

Call this once at startup, before anything else. It returns every tool the server
believes is in use to Idle, because a device that rebooted mid-session left them
stuck. Any metered session still open is settled as `abandoned` — the usage
reported so far is charged and the hold released.

`message` is `N tool(s) reset to idle`.

### `POST /api/toolguard/power-report`

Body fields, all optional so that a partial or older firmware still parses:

```json
{
  "tool_id": "laser-01",
  "draw_now": "4.2",
  "voltage_now": "119.8",
  "max_voltage": "125",
  "amperage_limit": "15",
  "self_tripped": false,
  "relay_on": true,
  "api_key": "…"
}
```

Decimal fields are **strings**, not floats, to avoid binary rounding on values
that get billed and compared against limits.

- `self_tripped: true` means *this controller shut itself off* after exceeding
  its own over-current limit. It locks out this tool only; the circuit is fine.
- `relay_on` is your own view of your output. **Absent is not `false`.** Omit it
  if you cannot report relay state — the bypass detector treats unknown as
  unknown, and a plug that claims `false` when it does not actually know will
  silently disarm the check that exists to catch a bypassed tool.

`tool_id` is validated *after* authentication: an unauthenticated request gets
401, not 422. A missing `tool_id` on an authenticated request is a 400, and an
unknown one is a 404. `message` is `Power reported`.

### `GET /api/toolguard/power-state`

Query: optional `api_key`. The lockout and circuit-topology snapshot, and the
poll fallback for the MQTT `power/state` push:

```json
{
  "as_of": "2026-09-16T12:00:00Z",
  "locked_tool_ids": ["laser-01"],
  "circuits": [{ "id": "…uuid…", "amperage_limit": "20" }],
  "tools": [{ "id": "…uuid…", "external_id": "laser-01", "circuit_id": "…uuid…" }]
}
```

**`as_of` is advisory.** Lockout is sticky and never expires on age. An old
snapshot showing a tool locked still means locked — do not decide a lockout has
lapsed because the timestamp is stale. That bias is deliberate: fail secure.

### `GET /api/toolguard/module-state`

Query: optional `api_key`. Module bindings and interlock rules; the poll fallback
for the MQTT `module/state` push. See [failure semantics](#failure-semantics) for
what the fields oblige you to do.

### `POST /api/toolguard/power-trip`

Body: `circuit_id` (UUID, required), optional `reason`, optional `api_key`.

Report that you summed draw locally across a circuit and tripped it. The server
records the lockout authoritatively with `lockout_source = edge_fast_trip` and
re-broadcasts. `message` is `Circuit tripped`. A missing `circuit_id` is a 400.

---

## MQTT

**There are two brokers, and which one you talk to depends on what you are.**

- The **site broker** carries the server ↔ device wire. Its topics are
  namespaced: the namespace comes from `mqtt_config.mqtt_namespace` at
  registration — `cs/spaces` by default. **Do not hard-code it.**
- The **local broker** carries the edge ↔ module wire, on the LAN, with
  unnamespaced topics. If you are writing module firmware — a ToolGuard, a card
  reader, a plug — this is the one you speak, and [the edge is your
  counterparty](#the-local-broker-edge--module), not the server.

A module does not need credentials for the site broker and generally should not
have them. It asks the edge; the edge asks the server.

### Device → server

| topic | payload |
|---|---|
| `{namespace}/devices/{device_id}/heartbeat` | the literal string `ping` |
| `{namespace}/devices/{device_id}/data` | system info (MAC, version, addresses) |
| `{namespace}/devices/{device_id}/doors/event` | door events |

The reference edge publishes a heartbeat **every 15 seconds** at QoS 1.

### Server → device

Published to `{namespace}/devices/{device_id}/{kind}`:

| kind | meaning |
|---|---|
| `name` | the device's assigned name |
| `toolguard/state` | authorization state; the push twin of `GET /sync` |
| `doors/state` | door state snapshot |
| `doors/unlock` | an unlock command |
| `power/state` | lockout + circuit topology; push twin of `GET /power-state` |
| `module/state` | module bindings + interlocks; push twin of `GET /module-state` |

Subscribe to the pushes **and** poll the HTTP twin on a slow timer. The push is
the fast path; the poll is what recovers you after a dropped message.

Connect with a **clean session and re-subscribe on every reconnect.** The server
does exactly this, for a reason worth repeating: with `clean_session(true)` the
broker discards subscriptions when the link drops, so a client that reconnects
without re-subscribing is *connected and deaf* — healthy from the outside, and
silently receiving nothing.

### The local broker: edge ↔ module

The topics a module actually speaks. They are **not** namespaced and carry no
`device_id` — the local broker is inside one building, and the payloads name the
tool and the card themselves.

The names come from `css_lib::wire::local`, which is the single definition
shared by the edge, the kiosk and the test tools. The oracle named at the top of
this file holds this table and that module to each other, so neither can move
without the other.

**Module → edge** (the edge subscribes):

| topic | payload |
|---|---|
| `toolguard/request/tool-on` | `{ "card", "tool_id", "api_key"? }` |
| `toolguard/request/tool-off` | `{ "card", "tool_id", "api_key"? }` |
| `toolguard/request/tool-log` | `{ "card", "tool_id", "seconds", "temperature"?, "api_key"? }` |
| `toolguard/request/power` | `{ "tool_id", "device_id"?, "draw_now"?, "voltage_now"?, "relay_on"?, "self_tripped"?, … }` |
| `door/request/scan` | `{ "door_id", "card_id" }` |
| `kiosk/refresh` | *(none)* — ask the edge to re-push `toolguard/state` |

**Edge → module** (the edge publishes):

| topic | payload |
|---|---|
| `toolguard/response/tool-on` | the server's `ToolGuardResponse` |
| `toolguard/response/tool-off` | the server's `ToolGuardResponse` |
| `toolguard/response/tool-log` | the server's `ToolGuardResponse` |
| `toolguard/response/power` | `{ "ok": true }` — an ack, nothing more |
| `door/response/unlock` | `{ "door_id", "duration_ms" }` — a momentary unlock |
| `toolguard/state` | the allow-list; the local twin of `GET /api/toolguard/sync` |
| `toolguard/lease` | permission for one power module to stay energized — see [the lease](#authorization-is-a-lease-not-a-command) |

Three things about this table that cost time if you learn them the hard way:

**The response topics carry the server's envelope unchanged.** Everything in
[denials are 200, not 4xx](#denials-are-200-not-4xx) applies here too — there is
no status code on MQTT at all, so `tool_on: true` is the *only* thing that means
energize. A response arriving is not an answer of yes.

**`toolguard/request/power` is the MQTT twin of `POST /api/toolguard/power-report`,**
with the same fields and the same rule about `relay_on`: omit it if you cannot
report relay state, because absent means "cannot report" and `false` means "I
looked and it is off". The detector needs to tell those apart.

**Send `device_id` on every power report.** It is optional so that older
firmware keeps parsing, but it is how the coordinator knows you are alive, and
a module it has not heard from is a module it will not grant a lease to. A plug
whose binding is `fail_off` — the default, and the right default — and which
never sends `device_id` is refused permanently, and from the outside that looks
identical to a tool that is simply switched off. Report it even when you have
nothing else to say: a power report carrying only `tool_id` and `device_id` is
a valid heartbeat.

**`toolguard/response/power` is an acknowledgement, not an instruction.** It says
your report was received. It is not permission to remain energized, and firmware
that treats a returning ack as a heartbeat of approval has built exactly the
fail-open design the [lease](#authorization-is-a-lease-not-a-command) exists to
avoid.

### WebSocket: `GET /api/devices/ws`

`/api/devices/ws` carries the same bodies with the same `kind` strings, wrapped:

```json
{ "kind": "module/state", "payload": { … } }
```

The `kind` is identical to the MQTT topic suffix, so one dispatch table serves
both transports.

---

## The lifecycle

```
  power on
     │
     ├─ POST /boot-reset            ← clear sessions your reboot orphaned
     ├─ GET  /sync                  ← fetch and cache the allow-list
     ├─ GET  /power-state           ← fetch and cache lockouts
     ├─ GET  /module-state          ← fetch and cache wiring + interlocks
     ├─ subscribe to the push topics
     └─ start the heartbeat
           │
   ┌───────┴───────────────────────────────────────────┐
   │  card presented                                    │
   │     └─ GET /tool-on?card=…&tool_id=…               │
   │          ├─ tool_on: true  → energize              │
   │          └─ otherwise      → refuse, show message  │
   │  while running                                     │
   │     ├─ POST /power-report   (periodically)         │
   │     └─ renew the lease      (see below)            │
   │  session ends                                      │
   │     ├─ GET /tool-log?…&seconds=…                   │
   │     └─ GET /tool-off?card=…&tool_id=…              │
   └────────────────────────────────────────────────────┘
```

---

## Failure semantics

**This is the section you cannot infer from the endpoint shapes, and the section
where being wrong is a safety problem rather than a bug.**

### Authorization is a lease, not a command

Permission to energize is **not** fire-and-forget. The edge renews it, and only
while every condition still holds. **The absence of the expected renewal is
itself the trip.**

That inversion is the whole design. An edge that dies, a broker that drops, a
sensor that goes quiet — all of them stop the renewals, so none of them can
leave a tool energized on a stale promise. A design where the edge must send a
*stop* fails open on exactly the faults most likely to happen.

**What your firmware must do:** run your own watchdog. When renewals stop
arriving, reach your safe state on your own, without being told. Do not wait for
a command that, by construction, is not coming.

#### The wire

Leases arrive on the local broker, on `toolguard/lease`, one message per
module per renewal:

```json
{
  "tool_id": "laser-01",
  "device_id": "…uuid…",
  "grant": true,
  "ttl_ms": 3000,
  "reason": null
}
```

Filter on `device_id` — every module on the broker sees every lease. Energize
only while `grant` is `true`, and only for `ttl_ms` after the message **arrived**.

`grant: false` is an instruction to de-energize now, sent when the coordinator
can still reach you and has decided you may not run. It is a courtesy, not the
mechanism: the mechanism is the silence that follows. Firmware that handles
`grant: false` but has no expiry timer is firmware that stays on forever the
moment the edge dies, which is the failure this whole design exists to prevent.

`reason` is for logs and humans. Never branch on it; the vocabulary is not part
of this contract and will change.

**There is no request topic, and no acknowledgement.** You do not ask for a
lease and you do not confirm one. The coordinator offers, and the offer ceasing
is the instruction.

#### Timing

The reference coordinator renews **every second** with a **3000 ms** TTL, so a
module tolerates two lost renewals before it cuts. Both ends are configurable
(`module_lease_interval_ms`, `module_lease_ttl_ms`) and you must not hard-code
either — read `ttl_ms` from each message.

**Measure the TTL from your own receipt, on your own monotonic timer.** The
payload deliberately carries no issue timestamp: a module computing
`issued_at + ttl_ms` against its own clock holds the lease too long whenever the
two clocks disagree, and a microcontroller with no RTC disagreeing with a
coordinator is the normal case, not the exception.

#### What a lease is not

A lease is not authorization. `tool-on` is what authorizes a *person* to use a
tool; a lease is what permits a *relay* to remain closed, and it is withheld for
reasons that have nothing to do with who swiped — a door opened, a sensor went
quiet, the coordinator lost confidence. Both must hold. Neither implies the
other.

### `on_disconnect`

Each binding in `module/state` carries `on_disconnect`, one of:

| value | obligation |
|---|---|
| `fail_off` | de-energize when the link drops. The default for power modules |
| `hold_last` | keep the current output state |
| `ignore` | the link is not safety-relevant for this module |

### `power_fails_safe`

Per tool in `module/state`: whether **every** power module bound to it reaches a
safe state on its own when it stops hearing from the coordinator.

`false` is not an error. A tool switched by an unmodifiable smart plug that holds
its last relay state genuinely cannot fail safe, and the coordinator's cut is
then a *mitigation* rather than an *interlock*. The field exists so that gap is
visible to the edge and to the administrator instead of being assumed away. If
you are writing firmware for a power module, making this `true` for your device
is the single highest-value thing you can do.

### Interlocks

`module/state` carries rules with `kind`:

- **`start_gate`** — AND-ed preconditions. Every one must be satisfied to start.
- **`trip`** — OR-ed faults. Any one fires cuts the tool.

Other fields: `condition` (`door_open`, `estop`, `flow_ok`, …), `debounce_ms`,
`latch`, `reset` (`re_auth` | `operator_ack` | `auto`), and `enforcement`
(`firmware` | `edge` | `server`) — where the trip actually executes.

Two rules that are easy to get backwards:

**Conditions have polarity.** Most conditions name a *hazard*: asserted means
unsafe (`door_open`, `estop`). Some name a *requirement*: asserted means safe
(`flow_ok`, `authorized`). A start gate is satisfied when a hazard is clear or a
requirement is met; a trip fires when a hazard appears **or a requirement is
lost**. Coolant flow stopping mid-cut is a trip, not merely a failure to start.

**An unrecognised condition is treated as a hazard.** A rule your firmware does
not understand must never be the reason a tool stays energized. Fail closed on
anything you cannot parse.

**`latch` defaults to true.** A tripped cut stays denied until an explicit reset.
Auto-resuming a cut tool when the lid falls shut is precisely the behaviour that
default exists to prevent.

### `enforcement: firmware`

Where a rule says `firmware`, the module is expected to inhibit its own relay
from a directly-wired sensor, without a round trip. The edge is the backstop, not
the mechanism. If your hardware can sense and act on the same board, say so and
implement it — that tier is faster than the network and survives the network
being gone.

### Liveness

The server watches `last_seen`, updated by your heartbeat. A device silent for
longer than `bypass.module_silence_secs` (default **90 seconds**, against a
15-second heartbeat) is recorded as silent in the audit log. The event records
silence and explicitly declines to guess the cause — unplugged, crashed, wifi
dropped, broker unreachable, and pulled off the wall on purpose are
indistinguishable from the server's side.

This is **detection, not prevention.** Nothing cuts power because you went quiet;
it is recorded, and somebody looks. Your own fail-safe behaviour is what actually
protects the tool.

---

## Forward compatibility

Nearly every field added to these payloads is optional with a default, on
purpose. **This is a promise:** a new field will not stop older firmware from
parsing a response.

What that obliges you to do in return:

- **Ignore unknown fields.** Do not use a strict parser that rejects them.
- **Do not treat a missing optional field as a zero value.** Absent means "not
  reported", which is not the same as `false` or `0` — `relay_on` is the case
  where that distinction has safety consequences.
- **Do not depend on field order** or on `message` text.

---

## A server to develop against

`reaper` brings up a complete instance — Postgres, an MQTT broker, and the
server — reachable on your network, seeded with everything you need:

```sh
reaper test --profile devlive     # brings it up, prints the URL and credentials
reaper renew                      # before the TTL expires
reaper down                       # destroy the VM, the database, and all
```

The `devseed` stage seeds an admin account and a small tool inventory with
training steps, and on top of that a **firmware fixture**: a tool that requires
no training, a member holding a card, and an unclaimed device invite. The values
are fixed, so your device config does not need editing every morning:

| | value |
|---|---|
| `tool_id` | `dev-tool-01` |
| `api_key` | `dev-tool-key` |
| card | `DEVCARD01` |
| member sign-in | `devmember` / the seeded password |
| device invite | printed at the end of the stage |

They are all printed when the stage finishes, along with the instance URL.

The fixture is **proved by use, not by assertion**: the seed calls `tool-on` with
the seeded card and then `tool-off`, and fails the stage if either does not do
what this document says it does. A fixture that exists in the database but
cannot turn a tool on would cost you a morning of debugging firmware against a
platform that was never going to answer.

The other five tools all require training, so `tool-on` against them answers
`Training required` — that is correct behaviour, not a broken seed. Use
`dev-tool-01`.

`reaper` is configured by `.reaper.toml` in this repository; `TESTING.md`
describes the wider battery.
