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
> `checks/tests/the_firmware_protocol_is_documented.rs` fails the build when it
> and the server disagree, in both directions, on:
>
> - every endpoint and MQTT topic — named here but gone, or exposed there and
>   undocumented;
> - every **field** of every payload that crosses the wire to or from a device,
>   including the two declared inside their own handlers;
> - every **enumerated value** the server accepts or sends — device and binding
>   roles, `on_disconnect`, the interlock vocabulary, and the serialized
>   spellings of tool status;
> - every literal **denial message**, and that none of them names a credential
>   this server no longer accepts;
> - the byte layout of a **signed message**, which firmware computes a MAC over;
> - every internal link in this file.
>
> So a new field cannot land here undescribed and a renamed one cannot leave a
> description behind. What no check can prove is that the prose around a name is
> *right* — if something here looks wrong, it is a bug worth reporting rather
> than a stale page to work around.

---

## Contents

- [Concepts](#concepts)
- [Device classes](#device-classes) — what your hardware can and cannot do
- [Walkthrough](#walkthrough) — the order to build it in
- [Authentication](#authentication)
- [Registration](#registration)
- [Signed commands](#signed-commands)
- [HTTP endpoints](#http-endpoints)
- [MQTT](#mqtt)
- [The lifecycle](#the-lifecycle)
- [Failure semantics](#failure-semantics)
- [Forward compatibility](#forward-compatibility)
- [A server to develop against](#a-server-to-develop-against)

---

## Concepts

**Device.** Anything that registers and holds a token. A device declares its
**capabilities** rather than a single kind (#101): a `roles` array drawn from
`reader`, `power`, `sensor` (the roles it can be *bound* to a tool as), plus
`edge` (local coordinator) and `kiosk` (display). A device may declare several —
one unit that both reads a card and switches power registers with
`"roles": ["reader", "power"]`, which the old single `kind` could not express.
The capabilities blob may also carry the firmware-enforcement descriptors
(`local_inputs`, `local_inhibit`, `countdown`, `holds_last_on_disconnect`) that
decide whether a safety interlock can be enforced in firmware.

**Resource.** Anything the space controls access to: a **tool** or a **door**.
They are one model (#101) — one set of access rules, one decision engine, and one
kind of device binding — so what you learn about binding a tool applies to a door
unchanged. Where this document says "bound to a tool", the mechanism is the same
row that binds a device to a door.

**Tool.** A machine the space controls access to. A tool has a UUID and,
usually, an `external_id` — the short string your firmware is configured with
and sends as `tool_id`. Both are accepted wherever a tool is named; the
`external_id` is matched first.

**Door.** A resource whose access is a strike rather than a relay. A door names
two places (it connects them) and is driven by exactly one device bound to it in
the **`edge`** role — its coordinator. That binding is how the server knows where
to send `doors/unlock`; a door with no `edge` binding is authored but drives
nothing, and an unlock for it is logged and published to nobody.

A door decides access through the same rules a tool does, including the
`open_access` latch that holds the strike released during a scheduled window. A
remote unlock from an administrator is **not** a bypass: it composes with the
door's rules, so revoking a door's grant revokes remote unlock for it too.

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

## Device classes

**Read this before you choose an architecture.** Most of this protocol runs on a
microcontroller. One part of it does not, and finding that out after the
hardware is in the wall is expensive.

### What every class must do

Registration, a heartbeat, the lifecycle calls, the local-broker topics, the
lease watchdog, HMAC signing, and your own fail-safe behaviour. All of it fits
comfortably on a microcontroller: the payloads are small, the decimal fields are
strings (so you need no float formatting), and the lease is deliberately
specified against a **monotonic** timer so you need no real-time clock.

### The one thing that does not fit: matching a card offline

Card identifiers never leave the server in the clear. What a device gets is
`argon2id(device_pepper, card)`, and matching a swipe means computing that same
digest from the card you just read. The server uses the argon2 crate's default
parameters — **m = 19456 KiB (19 MiB), t = 2, p = 1** — and the memory-hardness
is the whole point: it is what stops a stolen pepper from walking a 4-byte UID's
2³² space in seconds (#109, and
`checks/tests/a_device_cannot_reverse_a_card_digest.rs`).

An ESP32-C3 has roughly **400 KB of SRAM and no PSRAM**. It is short by a factor
of about fifty, and **the parameters must not be lowered to fit it** — that
throws away exactly what the digest buys.

This applies to **both** card lists, which is easy to miss because they arrive
by different routes:

| source | field | digest |
|---|---|---|
| `GET /api/toolguard/sync` | `users[].profile_field_digest` | hex `argon2id` |
| `doors/state` | `doors[].allow_cards`, `doors[].deny_cards` | the same digest, hex |

If you compare either list against the raw UID you read, you will match nothing,
ever, and the symptom is a reader that refuses every card with no error
anywhere.

### Size, separately from the hashing

`GET /api/toolguard/sync` narrows **tools** to the ones your device is bound to.
It does **not** narrow users: every account comes down, because any of them might
swipe. At the time of writing that is ~470 members at roughly 175–350 bytes of
JSON each — call it **100 KB and growing with the membership** — on top of the
~40 KB a TLS session wants. `doors/state` has the same shape: a flat card list
per door.

So even with the hashing solved, a cache-and-match design does not fit on a
400 KB part, and it is not meant to.

### The three shapes that work

**1. Module behind an edge — the normal shape, and what a microcontroller is
for.** You speak only the [local broker](#the-local-broker-edge--module). You
read a card and publish it; the edge holds the pepper, calls `sync`, hashes,
decides, and answers you. You never hash, never hold the pepper, never fetch the
roster. An ESP32-C3 is comfortable here and this is what the reference reader
hardware is.

**2. Standalone and online.** No edge, and no local cache: treat every tool as
`requires_online` and call the server per swipe. You need HTTPS and JSON and
nothing else. The cost is that a dropped link means no tool starts; the benefit
is that a part with 400 KB of RAM can do it.

**3. Edge-class coordinator.** Only a device with the RAM to run `argon2id` —
a Linux SBC, in practice — may hold the pepper and match from a cache. If you
are building an edge, you are building class 3; if your part cannot run the KDF,
you are building class 1 or 2 whatever else it can do.

### Clocks

The tool path needs **no** clock: the lease is measured from receipt on your own
monotonic timer, and `as_of` on every snapshot is advisory.

The **door** path is the exception. `doors[].hold_unlock_until` is an absolute
instant you compare against your own wall clock, so a door controller needs
SNTP or equivalent. A controller with a wrong clock either holds a door unlocked
after the window closed or refuses to honour one that is open — so if you cannot
keep time, do not implement the Open Access latch, and treat the door as
card-gated always.

---

## Walkthrough

The reference sections below specify every field. This one is the order to do
things in, with the mistake that waits at each step. It assumes you are building
the common case — a [class 1 module](#device-classes) that reads a card and
switches a relay — and points out where a standalone device differs.

### Step 0 — get a server before you write any firmware

```sh
reaper test --profile devlive
```

It prints a URL, an admin sign-in, and a **firmware fixture**: a tool that needs
no training (`dev-tool-01`), a member holding card `DEVCARD01`, and an unclaimed
device invite. See [a server to develop against](#a-server-to-develop-against).
The fixture is exercised by the seed itself, so if `tool-on` does not work
against it, the platform is broken rather than your code — which is worth
knowing on day one rather than day three.

### Step 1 — claim an invite and keep what you are given

An administrator generates one at **`/admin/devices`** → *Generate Device
Invite*. It is single-use and expires. Register with it:

```sh
curl -sX POST https://css.example/api/devices/register \
  -H 'Content-Type: application/json' \
  -d '{"device_code":"<invite>","name":"laser-guard-01",
       "capabilities":{"roles":["power"],"local_inhibit":true},
       "mac_address":"02:00:00:00:00:01","software_version":"0.1.0",
       "platform":"other"}'
```

**Persist `device_id`, `auth_token`, the whole `mqtt_config`, and
`mqtt_config.command_key`** to non-volatile storage before you do anything
else. The invite is single-use: a device that loses its token needs a *new
invite*, not a retry. See [registration](#registration) and
[signed commands](#signed-commands).

*The mistake here:* writing the token to RAM, testing happily all afternoon, and
discovering after the first power cut that you cannot get back in.

### Step 2 — have an administrator bind you to something

Registration gets you a token. A token alone authorizes nothing that names a
tool. At **`/admin/facility`** → *Tool Wiring*, an administrator binds your
device to a tool **in a role** — `reader`, `power` or `sensor`. A door is bound
the same way, in the `edge` role, at *Doors*.

Until that happens, `tool-on` will refuse you, and the refusal looks exactly
like a credential problem. If you are building for a metered tool, the binding
must be `power`: see [authentication](#authentication).

**A tool whose module sits behind an edge needs TWO bindings, and the second one
is easy to miss.** The module is bound in its own role (`power`, say), and the
**edge is bound to the same tool in the `edge` role** — the same statement a door
already requires of its coordinator. The module's binding says what the hardware
does; the edge's says who may speak for the tool.

The edge binding is what lets your reports reach the server at all. A module
behind an edge has no site-broker credentials, so its
[power reports](#post-apitoolguardpower-report) arrive signed by the edge — and
without the `edge` binding the server refuses them, because the caller is not
bound to the tool. Nothing about energizing the tool breaks (a lease is decided
on the edge), so the symptom is narrow and quiet: the server records no power
draw, no relay state, and no liveness for you.

A device that reports **directly**, with its own token and its own binding, needs
only the one.

### Step 3 — boot in the right order

**Which order depends on your [class](#device-classes), and getting this wrong is
not a style question.**

**A class 1 module behind an edge calls none of these.** You speak only the local
broker (step 4); the edge performs the whole sequence below on your behalf. In
particular **do not call `boot-reset`** — see the warning at the end of this step.

**A class 2 standalone device, or an edge, boots like this:**

```
POST /api/toolguard/boot-reset     ← first, always
GET  /api/toolguard/sync           ← the allow-list (class 2 and edge only)
GET  /api/toolguard/module-state   ← your wiring and interlocks
GET  /api/toolguard/power-state    ← lockouts
subscribe to the push topics
start the heartbeat (15 s)
```

`boot-reset` is first because your reboot may have orphaned a session and left a
tool permanently "in use" for the next member.

> **`boot-reset` is site-wide, not scoped to you.** It returns **every** tool the
> server believes is in use to Idle and settles **every** open metered session as
> `abandoned` — charging the usage reported so far — regardless of which device
> asks or what it is bound to. That is correct for the one coordinator that just
> rebooted and knows nothing is running. It is destructive for anything else: a
> module that calls it on its own boot stops and bills every machine in the
> building, including ones mid-cut on another bench. Only a device that governs
> the whole site may call it.

*The mistake here:* skipping `boot-reset` because it worked in testing. It
worked because you never crashed mid-session in testing.

### Step 4 — one swipe, end to end

As a module, you do not call the server. You publish to the **local** broker and
wait:

```
publish  toolguard/request/tool-on   {"card":"DEVCARD01","tool_id":"dev-tool-01"}
subscribe toolguard/response/tool-on
```

Then — and this is the single most common firmware defect in this protocol —
**energize only if `authorized` is exactly `true`**:

```json
{ "authorized": false, "reason": "Training required" }
```

That is a *successful* request. Over MQTT there is no status code at all, so the
field is the whole answer. A response arriving is not a yes, a response without
an `authorized` field is not a yes, and `reason` is for humans — never branch on
it.

**The field is named differently on the two wires, and this is the one place that
matters.** On the local broker the edge answers `authorized`; over HTTP the server
answers `tool_on` inside its envelope (see
[`POST /api/toolguard/tool-on`](#post-apitoolguardtool-on) and
[denials are 200, not 4xx](#denials-are-200-not-4xx)). A module speaks the local
broker and wants `authorized`; a class 2 standalone device calls HTTP and wants
`tool_on`. Looking for the wrong one finds nothing, and nothing is not permission
— so the failure is safe but total: a tool that never starts, with no error
anywhere.

If nothing answers, fail closed. A timeout is a no.

### Step 5 — stay energized only while permitted

Authorization got the tool started. Staying on is a **lease**, renewed on
`toolguard/lease`, and the renewals stopping *is* the instruction to stop:

```json
{ "tool_id":"7c9e6679-7425-40de-944b-e07fc1f90ae7", "device_id":"…",
  "grant":true, "ttl_ms":3000 }
```

1. Filter on `device_id` — every module on the broker sees every lease.
   **`tool_id` here is the UUID, never your configured `external_id`** — see
   [the wire](#the-wire).
2. On each message with `grant: true`, restart a timer for `ttl_ms`
   **from receipt, on a monotonic clock**. Do not read a timestamp; there isn't
   one, deliberately.
3. When the timer expires, de-energize. Nobody will tell you to.
4. `grant: false` means de-energize now. It is a courtesy, not the mechanism.

*The mistake here:* implementing `grant: false` and omitting the timer. That
firmware stays on forever the moment the edge dies, which is the exact failure
the design exists to prevent. See [the lease](#authorization-is-a-lease-not-a-command).

### Step 6 — report, then stop

```
publish  toolguard/request/tool-log  {"card":…,"tool_id":…,"seconds":612.5}
publish  toolguard/request/tool-off  {"card":…,"tool_id":…}
```

`tool-log` before `tool-off`: on a metered tool the log is what gets billed and
the stop settles the session. Send `tool-off` on **every** stop, including the
ugly ones — an un-ended session leaves the tool unusable for the next member.

If you switch power, also publish `toolguard/request/power` periodically, and
**always include `device_id`**: a module the coordinator has not heard from is a
module it will not grant a lease to. A report carrying only `tool_id` and
`device_id` is a valid heartbeat.

### Step 7 — doors, if that is what you are building

A door is the same model with a different output, and two differences that
matter:

- You publish `door/request/scan` `{"door_id","card_id"}` and act on
  `door/response/unlock` `{"door_id","granted","duration_ms","reason"}` — a
  *momentary* release, not a state. **Release only when `granted` is `true`**,
  the same rule as `authorized` on the tool path: a refusal arrives on this same
  topic with `granted: false`, and `reason` is for humans. `duration_ms` is `0`
  on a refusal, so firmware that pulses it blindly happens not to open the door
  — do not rely on that, check `granted`.
- A door's `doors/unlock` command from the server **may carry a `sig`**, and if
  your device holds a `command_key` you must verify it and ignore the command if
  it does not check out. See [signed commands](#signed-commands).

### Step 8 — prove it fails safe before you fit it

Bench tests that pass with everything working prove almost nothing here. Do
these four, with a relay you can watch:

1. **Pull the network mid-session.** The relay must open within `ttl_ms`
   (3 s by default), with no command telling it to.
2. **Kill the edge process mid-session.** Same outcome. If it stays on, your
   lease timer is not armed.
3. **Swipe an unknown card.** You should see `tool_on: false` and the relay must
   not close for a moment, not even briefly.
4. **Reboot mid-session, then swipe again.** The tool must be usable — which it
   will not be unless step 3's `boot-reset` actually runs.

If you can also inhibit your own relay from a directly-wired sensor, declare
`local_inhibit` at registration and implement it: see
[`enforcement: firmware`](#enforcement-firmware). That tier is faster than the
network and survives the network being gone.

---

## Authentication

There is **one** credential: a device Bearer token.

```http
Authorization: Bearer <auth_token>
```

Obtained once, at [registration](#registration).

**What authorizes what.** A token alone authorizes the *device-wide* calls —
`sync`, `boot-reset`, `power-state`, `module-state` — which name no tool. A call
that names a tool additionally requires that your device be **bound to that
tool**: a reader wired to one machine cannot energize another with its own valid
token. Binding is an explicit act an administrator performs, not something
inferred from a place or from your capabilities.

**Metered tools are stricter, and it matters.** A tool that bills for usage
requires a device bound to it in the **`power`** role. Being authorized to
*start* a tool is not being authorized to put money on it, so a `reader` binding
is refused here even though it passes the checks above. This keeps a billable
report attributable to the thing that actually switches the machine.

> **Retired:** earlier firmware could authenticate with an `api_key` — either the
> tool's own `external_api_key` or a shared `toolguard.global_api_key` — sent as a
> query parameter or body field. **Both are gone, and the field is ignored.** The
> shared key was one secret that opened every tool, so a single leak opened the
> building; and the weakest accepted credential decided the real bar. A device
> token is stored hashed on the server and is scoped by its bindings, which the
> keys were not. If your firmware still sends `api_key`, it will be refused with
> **401** — register the device and send its token instead.

### What the failures look like

| condition | response |
|---|---|
| No credential, or a bad one | **401**, `ToolGuardResponse` with `status: "error"` |
| A credential, but a field is missing | **400** — *after* the credential is checked |
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
  "capabilities": {
    "roles": ["power"],
    "local_inputs": ["door_open"],
    "local_inhibit": true,
    "countdown": false,
    "holds_last_on_disconnect": false
  },
  "mac_address": "02:00:00:00:00:01",
  "software_version": "1.2.3",
  "platform": "linux",
  "ipv4_address": "192.168.1.50",
  "ipv6_address": null
}
```

`capabilities.roles` must be a non-empty array, and every role must be one of
`reader`, `power`, `sensor`, `edge`, `kiosk`. The enforcement descriptors
(`local_inputs`, `local_inhibit`, `countdown`, `holds_last_on_disconnect`) are
optional and each defaults to "cannot" when omitted. `platform` must be one of
`windows`, `linux`, `macos`, `other`. Anything else is a **400**, as is an
expired or already-claimed code. `ipv4_address` and `ipv6_address` are optional.

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
      "mqtt_namespace": "cs/spaces",
      "command_key": "4f3c…64 hex chars…"
    }
  },
  "error": null
}
```

**Persist `device_id`, `auth_token` and the whole `mqtt_config` to non-volatile
storage.** The invite is single-use: a device that loses its token needs a new
invite from an administrator, not a retry. `mqtt_config` may be `null` if the
deployment does not use MQTT.

`mqtt_config.command_key` is the per-device message-signing key, and it is
**omitted** when the deployment cannot mint one. Persist it with the rest and
read [signed commands](#signed-commands) before acting on anything that carries
a `sig`: ignoring it means acting on unsigned unlock commands.

The registration endpoint is public by design — the invite code is the
credential. Treat the code as a secret in transit.

---

## Signed commands

Registration may hand you a fifth field inside `mqtt_config`:

```json
"command_key": "4f3c…64 hex chars…"
```

It is a **per-device HMAC-SHA256 key**, issued once, and it authenticates
individual messages on the command channel as defence in depth behind the
broker's own authentication and topic ACLs. Even a compromised or misconfigured
broker then cannot forge a command your device will act on, or an event the
server will trust.

**Persist it with the token.** If you were given one and you ignore it, two
things go wrong, and both are quiet:

- you will act on **unsigned** `doors/unlock` commands — which is to say,
  anybody who can publish to the broker can open the door; and
- every `doors/event` you publish will be **rejected by the server** and never
  reach the audit trail, with nothing said on your side.

`command_key` is absent when the deployment has no card cipher configured to
mint one. A device with no key runs unsigned and the server accepts its events,
which is what makes a rollout possible — but it is the weaker mode, not the
intended one.

### What is signed, and over what bytes

The MAC is computed over a **canonical string**, never over re-serialized JSON,
so it cannot depend on field order surviving a round-trip on either side. Build
the string exactly as below, HMAC-SHA256 it with your key, hex-encode the
result, and put it in the message's `sig` field.

**`doors/unlock`** (server → device):

```
doors/unlock|{door_id}|{duration_ms}|{reason}
```

**`doors/event`** (device → server):

```
doors/event|{door_id}|{card_id}|{granted}|{source}
```

Pipe-separated, no spaces, no trailing separator. The values are the message's
own fields as text: `door_id` is the UUID in its usual hyphenated form,
`duration_ms` is the integer in decimal, `granted` is `true` or `false` in
lower case, and an absent `card_id`, `reason` or `source` is the **empty
string** — not the word `null`.

Only round-trip-stable fields are covered. `occurred_at` and any human-readable
`reason` on an event are deliberately **outside** the MAC, so no datetime or
float formatting difference between two implementations can break verification.

Verification is a constant-time comparison of the hex strings. A malformed or
empty `sig` is simply invalid, never an error.

### The rule, in one line each

- **You hold a key and a command arrives unsigned or wrongly signed:** ignore
  the command. Do not act, and do not fall back to acting.
- **You hold no key:** act on the command as it stands. You are in the legacy
  mode and the broker ACL is your only control.
- **You hold a key and you publish an event:** sign it. An unsigned event from a
  keyed device is dropped.

### Worked example

Key `0a0b0c…`, an unlock of door `6f1e…` for 4200 ms with reason `qr`:

```
message = "doors/unlock|6f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0|4200|qr"
sig     = hex(hmac_sha256(key_bytes, message_bytes))
```

`key_bytes` is the key **decoded from hex**, not its ASCII. Getting that wrong
produces a signature that is stable, plausible, and wrong on every message —
test against the fixture server before you trust it.

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

### `POST /api/toolguard/tool-on`

JSON body: `card`, `tool_id`. Requires a device token bound to this tool.

Authorizes a card against a tool and, if allowed, marks the tool in use.

**A body, not a query string, and that is deliberate.** A card identifies a
person, and a URL is written down by every proxy, reverse proxy and access log
it passes through, plus browser history and crash reporters. It is also the
honest method: this call *changes state*, so it was never a safe `GET`.

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
| `Metered tool requires a power-bound device` | the tool bills for usage and your device is not bound to it in the `power` role — see [authentication](#authentication) |
| *(billing-specific text)* | metered tool, funds or holds unavailable |

### `POST /api/toolguard/tool-off`

JSON body: `card`, `tool_id`. Requires a device token bound to this tool.

Ends the session and returns the tool to Idle. Returns `tool_off: true`.

Unlike `tool-on`, this endpoint answers `status: "error"` with **no** `tool_off`
field for an unknown card, a missing tool, or a bad key. Send it anyway on every
stop — an un-ended session leaves the tool unusable for the next member.

### `POST /api/toolguard/tool-log`

JSON body: `card`, `tool_id`, `seconds` (float), optional `temperature`
(float). Requires a device token bound to this tool; for a metered tool that
binding must be the `power` one.

Reports usage. `message` is `Usage logged`. For metered tools this is what gets
billed; send it before `tool-off`.

On a **metered** tool the report is checked against the session it belongs to,
and two refusals are specific to this endpoint:

| `message` | meaning |
|---|---|
| `No open session for this tool` | there is no open activation to bill against — `tool-on` never succeeded, or `tool-off` already settled it. Report usage *before* stopping |
| `This card did not activate the tool` | the `card` in the report is not the card that started the session. Send the card that swiped, not the last card seen |

Both come back as `status: "error"` with HTTP **200**, like every other refusal.

### `GET /api/toolguard/sync` — the allow-list

**Device Bearer token only.** Returns the cached authorization state for this
device:

```json
{
  "device_id": "…uuid…",
  "profile_field": "card_id",
  "tools": [
    { "id": "…uuid…", "external_id": "laser-01", "name": "CO₂ Laser",
      "status": "idle", "requires_online": false }
  ],
  "users": [
    { "profile_field_digest": "9f86d081…64 hex chars…", "full_name": "A Member",
      "is_active": true, "authorized_tool_ids": ["…uuid…"] }
  ]
}
```

`status` is one of `idle`, `in_use`, `maintenance`, `broken`, `repair`,
`retired` — lower case with underscores, exactly as written here. Only `idle`
and `in_use` are workable states; the other four mean the tool is out of
service and `tool-on` will refuse it.

**`profile_field_digest` is not the card.** It is
`argon2id(device_pepper, card)`, hex-encoded (#109). To match a swipe, hash the
card that was read with the same pepper and compare against this list — do not
expect to find the card value here, because it is deliberately not sent.

**Hashing is the edge coordinator's job, not a card reader's (#146).** `argon2id`
at the server's parameters needs about **19 MiB of RAM per invocation**, and the
memory-hardness is the point: it is what stops a stolen pepper from reversing a
4-byte UID's 2^32 space in seconds (#109, and
`checks/tests/a_device_cannot_reverse_a_card_digest.rs`). A microcontroller-class
reader — an ESP32-C3 has ~400 KB of SRAM and no PSRAM — **cannot** run it and
**must not** be handed the pepper or asked to match offline. Do **not** lower the
parameters to fit such a device: that throws away exactly what the digest buys.
Two shapes are correct instead:

- **Edge-fronted (the normal shape).** The reader is a [module](#concepts) that
  reports the card it read to its [edge](#concepts) over the local broker; the
  edge holds the pepper, calls `sync`, hashes, and evaluates access. The reader
  never hashes and never holds the pepper.
- **Standalone, no edge.** Only an edge-class device with the RAM to run
  `argon2id` may hold the pepper and match from the cache. A reader that cannot
  must run **online**: treat every tool as `requires_online` and call the server
  per swipe rather than caching and matching locally.

The pepper is configured on whatever coordinator does the hashing
(`card_device_pepper` on an edge) and is the **only** card key it is given: the
keys that decrypt cards at rest and that index them server-side never leave the
server. One `argon2id` invocation per swipe is the cost, and a plug taken off a
wall yielding no member's card identifier is what it buys — but only while the
pepper lives on a device that can actually run the slow KDF.

Hash the card **exactly as read** — no trimming, no case folding. The server
digests the stored value byte-for-byte, so any normalisation on your side
produces a digest that matches nothing and a member who is simply refused.

If you have no pepper you cannot match anything, and the correct behaviour is
to refuse every card rather than fall back to comparing raw values. The
reference edge does exactly that.

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

**It is site-wide and takes no scope.** Unlike the per-tool operations, this one
needs no binding — a valid device token is the whole credential — and "every
tool" means every tool on the server, not every tool you are bound to. So it is
for a device that coordinates the site and has just established that nothing is
running. **A module behind an edge must not call it:** its edge already did, and
a second call from a rebooting controller stops and bills every machine in the
building. See [step 3](#step-3--boot-in-the-right-order).

`message` is `N tool(s) reset to idle`.

### `POST /api/toolguard/power-report`

Body fields, all optional so that a partial or older firmware still parses:

```json
{
  "tool_id": "laser-01",
  "device_id": "…your uuid…",
  "draw_now": "4.2",
  "voltage_now": "119.8",
  "max_voltage": "125",
  "amperage_limit": "15",
  "self_tripped": false,
  "relay_on": true
}
```

Decimal fields are **strings**, not floats, to avoid binary rounding on values
that get billed and compared against limits.

- `device_id` is **your** device id, and sending it is what gives you liveness.
  `last_seen_at` — what the server's silence detector reads, and what the admin
  device list shows as online — is otherwise only written by a **site-broker
  heartbeat**, which a module behind an edge does not have and should not need.
  Without this field the server's only evidence of you is your edge, so you read
  as permanently silent: the sweep records it once and then never changes its
  mind, which means it can never report you actually *going* quiet. Send it on
  every report, exactly as on the [MQTT twin](#the-local-broker-edge--module).
  An edge forwarding a module's report passes the module's id through, so the
  liveness lands on the module rather than on the edge.
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

No parameters; a device token is the credential. The lockout and
circuit-topology snapshot, and the poll fallback for the MQTT `power/state` push:

```json
{
  "as_of": "2026-09-16T12:00:00Z",
  "locked_tool_ids": ["7c9e6679-7425-40de-944b-e07fc1f90ae7"],
  "circuits": [{ "id": "…uuid…", "amperage_limit": "20" }],
  "tools": [{ "id": "…uuid…", "external_id": "laser-01", "circuit_id": "…uuid…" }]
}
```

**`locked_tool_ids` holds UUIDs, not `external_id`s.** Everywhere else in this
protocol a tool may be named either way, and this one list may not: it is the
tool's UUID as a string. Resolve your configured `external_id` to a UUID through
the `tools` array in this same payload and match on that.

Getting this wrong fails **open**, which is why it is called out: firmware that
compares `locked_tool_ids` against the `external_id` it was configured with
never matches, so a locked-out tool reads as unlocked and energizes.

**`as_of` is advisory.** Lockout is sticky and never expires on age. An old
snapshot showing a tool locked still means locked — do not decide a lockout has
lapsed because the timestamp is stale. That bias is deliberate: fail secure.

### `GET /api/toolguard/module-state`

No parameters; a device token is the credential. Your wiring and the rules that
gate it; the poll fallback for the MQTT `module/state` push.

```json
{
  "as_of": "2026-10-02T12:00:00Z",
  "tools": [
    {
      "tool_id": "7c9e6679-…",
      "external_id": "laser-01",
      "power_fails_safe": true,
      "modules": [
        {
          "id": "…uuid…",
          "device_id": "…uuid…",
          "role": "power",
          "name": "laser-contactor",
          "params": { "gpio": 7, "active_low": false },
          "on_disconnect": "fail_off"
        }
      ],
      "interlocks": [
        {
          "id": "…uuid…",
          "kind": "trip",
          "condition": "door_open",
          "source_module_id": "…uuid…",
          "debounce_ms": 250,
          "latch": true,
          "reset": "operator_ack",
          "enforcement": "firmware"
        }
      ]
    }
  ]
}
```

Only tools with at least one module bound or one interlock defined appear. Every
id is a **string**, including the UUIDs, and `tool_id` is the UUID while
`external_id` is the short name — match the way you match elsewhere:
`external_id` first, then the UUID string.

**Find yourself by `device_id`.** The snapshot describes every module on every
tool you coordinate, not just you; a module reads its own row and ignores the
rest.

Field by field:

- **`role`** — `reader`, `power`, `sensor` or `edge`. The first three are a
  module's place in a tool's chain; `edge` is the coordinator binding, and it is
  what a **door** requires (a door with no `edge` binding drives nothing).
- **`params`** — your role-specific configuration, authored by the
  administrator: which GPIO drives the relay, which receptacle you are, which
  input a sensor reads. The shape is deliberately open, because it belongs to
  your hardware rather than to this protocol — read the keys you understand and
  ignore the rest. This is the field that tells a generic build which pin to
  switch, so it is usually the one you need first.
- **`on_disconnect`** — what you must do when the link drops; see
  [`on_disconnect`](#on_disconnect).
- **`power_fails_safe`** — a property of the *tool*, not of you: whether every
  power module bound to it reaches a safe state unaided. See
  [`power_fails_safe`](#power_fails_safe).
- **`interlocks`** — see [interlocks](#interlocks) for what the rules oblige
  you to do, and note `enforcement: firmware` means the trip is **yours** to
  execute.

### `POST /api/toolguard/power-trip`

Body: `circuit_id` (UUID, required), optional `reason`. A device token is the
credential; this call names no tool, so no binding is required.

Report that you summed draw locally across a circuit and tripped it. The server
records the lockout authoritatively with `lockout_source = edge_fast_trip` and
re-broadcasts. `message` is `Circuit tripped`. A missing `circuit_id` is a 400.

---

## MQTT

**There are two brokers, and which one you talk to depends on what you are.**

- The **site broker** carries the server ↔ device wire. Its topics are
  namespaced: the namespace comes from `mqtt_config.mqtt_namespace` at
  registration. It is **deployment-specific** — whatever the server's
  `[edge.edge_mqtt_config].mqtt_namespace` says — and there is no default to fall
  back on: this repository's own sample configs disagree (`cs/spaces` in the
  edge's, `css` in the server's). **Do not hard-code it;** read it from your
  registration response. The examples below use `cs/spaces` only because the
  text needs something concrete.
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

### Payloads: `data` (device → server)

The `data` topic carries your system info. **Every field below except the
addresses is required**: a payload missing one fails to parse, and the server
drops it with a log line you cannot see. That is the whole failure — no error
comes back, and your `last_seen` and version simply never update.

```json
{
  "mac_address": "02:00:00:00:00:01",
  "software_version": "1.2.3",
  "platform": "other",
  "uptime": 86400,
  "ipv4_address": "192.168.1.50",
  "ipv6_address": null
}
```

`uptime` is seconds since your boot, as an integer. `platform` is matched
case-insensitively against `windows`, `linux`, `macos`, and anything else is
recorded as `other`.

### Payloads: `doors/state` (server → device)

The compiled door snapshot. One message carries every door this device
coordinates.

```json
{
  "snapshot_at": "2026-10-02T12:00:00Z",
  "doors": [
    {
      "id": "6f1e2d3c-…",
      "name": "Front door",
      "enabled": true,
      "unlock_duration_ms": 4200,
      "allow_cards": ["9f86d081…", "…"],
      "deny_cards": [],
      "hold_unlock_until": null
    }
  ]
}
```

- **`allow_cards` and `deny_cards` are `argon2id` digests, hex-encoded** — the
  same digest as `profile_field_digest`, and the same reason you probably cannot
  compute it on a microcontroller. See [device classes](#device-classes).
- `deny_cards` wins over `allow_cards`.
- `hold_unlock_until` is the **Open Access latch**: when set, the strike is held
  released with no card required until that instant. It is an absolute time
  compared against *your* clock, so the window self-expires even if the closing
  push never arrives — which is the fail-secure behaviour, and the one place in
  this protocol where you need real time.
- `enabled: false` means the door is authored but must not open.

### Payloads: `doors/unlock` (server → device)

A remote unlock, from an administrator or the QR check-in flow. **Momentary:**
release the strike for `duration_ms` and let it fall closed. It is not a state
to hold.

```json
{ "door_id": "6f1e2d3c-…", "duration_ms": 4200, "reason": "qr", "sig": "…" }
```

`sig` is present when your device holds a `command_key`, and then it is
mandatory: verify it and ignore the command if it does not check out. See
[signed commands](#signed-commands).

### Payloads: `doors/event` (device → server)

What you publish after deciding a swipe locally. This is how a door entry
reaches the audit trail, so it is the record of who went where.

```json
{
  "door_id": "6f1e2d3c-…",
  "card_id": "04A1B2C3",
  "granted": true,
  "reason": "allow_list",
  "source": "reader-1",
  "occurred_at": "2026-10-02T12:00:00Z",
  "sig": "…"
}
```

Only `door_id` and `granted` are required. `occurred_at` is optional and the
server substitutes its own receipt time when you omit it — which is the right
choice for a device with no clock. **If you hold a `command_key`, `sig` is
mandatory and an unsigned event is dropped.**

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
| `toolguard/request/tool-on` | `{ "card", "tool_id" }` |
| `toolguard/request/tool-off` | `{ "card", "tool_id" }` |
| `toolguard/request/tool-log` | `{ "card", "tool_id", "seconds", "temperature"? }` |
| `toolguard/request/power` | `{ "tool_id", "device_id"?, "draw_now"?, "voltage_now"?, "relay_on"?, "self_tripped"?, … }` |
| `door/request/scan` | `{ "door_id", "card_id" }` |
| `kiosk/refresh` | *(none)* — ask the edge to re-push `toolguard/state` |

**Edge → module** (the edge publishes):

| topic | payload |
|---|---|
| `toolguard/response/tool-on` | `{ "authorized": bool, "reason": string }` |
| `toolguard/response/tool-off` | `{ "ok": true }` |
| `toolguard/response/tool-log` | `{ "ok": true }` |
| `toolguard/response/power` | `{ "ok": true }` — an ack, nothing more |
| `door/response/unlock` | `{ "door_id", "granted", "duration_ms", "reason" }` — a momentary unlock |
| `toolguard/state` | the allow-list; the local twin of `GET /api/toolguard/sync` |
| `toolguard/lease` | permission for one power module to stay energized — see [the lease](#authorization-is-a-lease-not-a-command) |

Three things about this table that cost time if you learn them the hard way:

**The response topics do NOT carry the server's envelope — the local wire has its
own shape, and `tool_on` does not appear on it at all.** The edge decides (from
its cache, or by asking the server) and answers with its own verdict:

```json
{ "authorized": false, "reason": "Training required" }
```

So on the local broker, **energize only if `authorized` is exactly `true`.** The
reasoning behind [denials are 200, not 4xx](#denials-are-200-not-4xx) applies here
with more force, because there is no status code on MQTT at all: a response
arriving is not an answer of yes, a response with no `authorized` field is not a
yes, and `reason` is for humans — never branch on it. If nothing answers, fail
closed; a timeout is a no.

`tool-off` and `tool-log` answer `{ "ok": true }`, which acknowledges *receipt*
and nothing else. It is not a confirmation that the session was settled or the
usage recorded — the edge forwards both to the server after answering you, and a
later failure there is invisible on this topic.

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
   │     └─ POST /tool-on  {card, tool_id}              │
   │          ├─ tool_on: true  → energize              │
   │          └─ otherwise      → refuse, show message  │
   │  while running                                     │
   │     ├─ POST /power-report   (periodically)         │
   │     └─ renew the lease      (see below)            │
   │  session ends                                      │
   │     ├─ POST /tool-log {…, seconds}                 │
   │     └─ POST /tool-off {card, tool_id}              │
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
  "tool_id": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
  "device_id": "…uuid…",
  "grant": true,
  "ttl_ms": 3000,
  "reason": null
}
```

Filter on `device_id` — every module on the broker sees every lease. Energize
only while `grant` is `true`, and only for `ttl_ms` after the message **arrived**.

**`tool_id` on this topic is always the tool's UUID, never its `external_id`.**
Elsewhere in this protocol a tool may be named either way and a *request* you
send is resolved both ways — but the coordinator builds this field from the
wiring snapshot, which is keyed by UUID, so there is nothing to resolve. This is
the same trap as
[`locked_tool_ids`](#get-apitoolguardpower-state), and it fails in the safer
direction: match the lease against your configured `external_id` and you match
nothing, so you never energize. Resolve your `external_id` to a UUID once
through `module-state` and match on that, or filter on `device_id` alone — which
is sufficient when your device holds exactly one `power` binding.

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

Other fields: `condition`, `debounce_ms`, `latch`, `reset`
(`re_auth` | `operator_ack` | `auto`), and `enforcement`
(`firmware` | `edge` | `server`) — where the trip actually executes.

`condition` is one of eight, and which half it falls in decides what asserting
it means:

| condition | asserted means | half |
|---|---|---|
| `door_open` | a guard door is open | hazard |
| `lid_open` | an enclosure lid is open | hazard |
| `estop` | emergency stop is pressed | hazard |
| `auth_expired` | the authorization behind this session has lapsed | hazard |
| `draw_over` | measured draw is above the limit | hazard |
| `module_offline` | a module in the chain has gone quiet | hazard |
| `flow_ok` | coolant/extraction flow is present | requirement |
| `authorized` | a current authorization exists | requirement |

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

The server watches `last_seen`, and **how it gets updated depends on what you
are:**

- A device on the **site broker** (an edge, a kiosk, a class 2 standalone)
  updates it with its 15-second `heartbeat`.
- A **module behind an edge** has no site-broker credentials, so it cannot
  heartbeat. It updates `last_seen` by sending `device_id` on its
  [power reports](#post-apitoolguardpower-report) — the edge passes the id
  through, and the server credits the module. A module that omits `device_id`
  has no liveness at the server at all, and separately will never be granted a
  lease (see the local-broker tables).

A device silent for
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
| device token | printed by `devseed` when the stack comes up |
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
