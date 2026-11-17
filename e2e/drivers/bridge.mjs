// The IoT bridge against a real edge. See stage_bridge in run.sh for the
// windows; this driver does the platform setup and reads the captures.
//
//   setup   admin, tool, member+card, the EDGE device registered and bound,
//           both configs under STACK_DIR, broker logins, and an invite written
//           to bridge-invite for the bridge to claim itself with. Plus, for the
//           Home Assistant half (bridge #20): two doors between two places --
//           A momentary, B until_stopped -- each with a lock the fake HA owns,
//           and invites for the bridge to claim those identities too.
//   bind    the identities the bridge wrote -> the plug bound as power on the
//           tool; each lock bound as power AND strike (pulse 3000) on its
//           door, with the edge as coordinator and a rule admitting the member.
//   republish  the doors snapshot to the edge, as the lease stage does.
//   assert  the captured windows, one case per claim.

import fs from 'node:fs'
import path from 'node:path'
import { PATCH, POST, PUT, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'
import { parseMarks, parseTimeline } from '../journeys/timeline.mjs'
import { judgeBridgeTimeline } from '../journeys/bridge-timeline.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const FIXTURE = path.join(STACK_DIR, 'bridge-fixture.json')
const mqttPort = process.argv[3] ?? '1883'
const devicePepper = process.argv[4] ?? ''
const FRIENDLY = 'e2e/plug/extractor'
/** Where the stage runs the fake Home Assistant (stack.sh start_fake_ha). */
export const FAKE_HA_PORT = 18123
export const FAKE_HA_TOKEN = 'e2e-fake-ha-token'
const LOCKS = { 'door-a': 'lock.door_a', 'door-b': 'lock.door_b' }
/** A door contact HA owns, bridged as a sensor on door A (bridge #25, CS:S #213). */
const CONTACT = 'binary_sensor.door_a_contact'

/** Every JSON object mosquitto_sub captured, one per line, bad lines dropped. */
function captured(name) {
  const file = path.join(STACK_DIR, name)
  if (!fs.existsSync(file)) return null
  return fs
    .readFileSync(file, 'utf8')
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      try {
        return JSON.parse(l)
      } catch {
        return null
      }
    })
    .filter(Boolean)
}

async function setup() {
  const admin = await adminAccount('bridge_admin')
  const tag = admin.username
  const base = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'

  const tool = await POST('/api/tools', {
    token: admin.token,
    body: { name: `Bridge Laser ${tag}`, category: 'laser_cutting', external_id: `bridge-${tag}` },
  })
  const toolId = tool.json?.data?.id
  ok('bridge/tool-created', !!toolId, `POST /api/tools -> ${tool.status}`)

  // The edge itself, as the lease stage does it: registered, bound as `edge`.
  const invite = await POST('/api/admin/devices/invite', { token: admin.token, body: { expires_in_hours: 1 } })
  const reg = await POST('/api/devices/register', {
    body: {
      device_code: invite.json?.data?.device_code,
      name: `bridge-edge-${tag}`,
      capabilities: { roles: ['edge'] },
      mac_address: '02:00:00:00:84:01',
      software_version: '0.0.0-e2e',
      platform: 'linux',
    },
  })
  const edge = {
    id: reg.json?.data?.device_id ?? reg.json?.device_id,
    token: reg.json?.data?.auth_token ?? reg.json?.auth_token,
    commandKey: reg.json?.data?.mqtt_config?.command_key ?? null,
  }
  ok('bridge/edge-registered', !!edge.id, `register -> ${reg.status}`)
  const edgeBinding = await POST('/api/admin/device-bindings', {
    token: admin.token,
    body: { resource_id: toolId, device_id: edge.id, role: 'edge', name: `bridge edge ${tag}` },
  })
  assertEq('bridge/edge-bound', 201, edgeBinding.status, edgeBinding.text.slice(0, 200))

  // A member with a card, so a tool-on at the edge can open a session.
  const member = await account(`bridge_member_${tag}`)
  const card = `BRIDGE-CARD-${tag}`.toUpperCase()
  const cardSet = await PUT(`/api/profiles/${member.user.id}`, {
    token: member.token,
    body: { profile: { card_id: card } },
  })
  assertEq('bridge/member-card-set', 200, cardSet.status, cardSet.text.slice(0, 160))

  // Two doors for the Home Assistant locks (#20): A stays `momentary`, so
  // its lease is exactly the strike pulse; B becomes `until_stopped` in bind,
  // so a lease on it outlives the pulse and stop-on-loss has something to stop.
  const mkPlace = async (name) => {
    const res = await POST('/api/admin/places', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_type: 'Outside', is_special: true },
    })
    return res.json?.data?.id
  }
  const outside = await mkPlace('bridge-outside')
  const inside = await mkPlace('bridge-inside')
  ok('bridge/places-created', !!outside && !!inside, 'two places for the doors to connect')
  const mkDoor = async (name) => {
    const res = await POST('/api/admin/doors', {
      token: admin.token,
      body: { name: `${name}-${tag}`, place_id_from: outside, place_id_to: inside },
    })
    return { id: res.json?.data?.id, status: res.status }
  }
  const doorA = await mkDoor('bridge-door-a')
  const doorB = await mkDoor('bridge-door-b')
  ok('bridge/doors-created', !!doorA.id && !!doorB.id, `${doorA.status} ${doorB.status}`)
  for (const [label, door] of [['a', doorA], ['b', doorB]]) {
    const rule = await POST(`/api/admin/doors/${door.id}/rules`, {
      token: admin.token,
      body: { kind: 'user', value: member.user.id, effect: 'allow' },
    })
    ok(`bridge/door-${label}-admits-the-member`, rule.status === 201, rule.text.slice(0, 120))
  }
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-ha-token'), `${FAKE_HA_TOKEN}\n`)

  // The TOOL gets a door_open trip, read from a contact the bridge will
  // report as a sensor module (#25). Edge-enforced; the default latch stays.
  // (A door cannot carry an interlock rule today: tool-interlocks validates
  // against tools only -- filed as a CS:S issue from this stage's first run.)
  const trip = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: { tool_id: toolId, kind: 'trip', condition: 'door_open', enforcement: 'edge' },
  })
  assertEq('bridge/the-tool-has-a-door-open-trip', 201, trip.status, trip.text.slice(0, 160))

  // The edge's config. Renew twice per TTL; a long offline window so a report
  // never ages out underneath an assertion -- the bridge's alive is every 2 s.
  fs.writeFileSync(
    path.join(STACK_DIR, 'edge-bridge.config.toml'),
    `name = "e2e-bridge-edge"
auth_status = "Approved"
remote_transport = "websocket"
toolguard_sync_interval_secs = 5
calendar_mqtt_topic = "cs/spaces/calendar/events"
calendar_sync_interval_secs = 300
card_device_pepper = "${devicePepper}"
command_key = "${edge.commandKey ?? ''}"
module_lease_ttl_ms = 3000
module_lease_interval_ms = 500
module_offline_ms = 120000

[local_mqtt_config]
mqtt_instance_url = "tcp://127.0.0.1:${mqttPort}"
mqtt_client_id = "e2e-bridge-edge"
mqtt_namespace = "cs/spaces"

[remote_device_info]
remote_id = "${edge.id}"
remote_auth_token = "${edge.token}"
remote_instance_url = "${base}"
`,
  )

  // The bridge's config, in the paths the BRIDGE will see. Under podman the
  // bridge runs in a container with STACK_DIR mounted at /stack, and
  // CSS_STACK_DIR is unset here, so STACK_DIR is '/stack'. Under
  // --provision=external (CI) the bridge runs on the host, CSS_STACK_DIR is
  // the host path, and so is STACK_DIR. Writing the literal '/stack' here was
  // the first CI run's failure: "Permission denied creating /stack/...".
  // The broker is anonymous; the logins are present because the bridge
  // requires the files, not because the broker reads them.
  for (const f of ['bridge-iot.toml', 'bridge-edge.toml']) {
    fs.writeFileSync(path.join(STACK_DIR, f), 'username = "e2e"\npassword = "e2e"\n')
  }
  fs.writeFileSync(
    path.join(STACK_DIR, 'bridge.config.toml'),
    `[server]
url = "${base}"

[edge]
broker = "mqtt://127.0.0.1:${mqttPort}"
credentials = "${STACK_DIR}/bridge-edge.toml"

[iot]
broker = "mqtt://127.0.0.1:${mqttPort}"
credentials = "${STACK_DIR}/bridge-iot.toml"
base = "zigbee2mqtt"

[[device]]
name = "extractor"
backend = "zigbee2mqtt"
friendly = "${FRIENDLY}"
role = "power"
resource = "bridge-${tag}"
identity = "${STACK_DIR}/bridge-identities/extractor.toml"
countdown = true
on_time_s = 60
rearm_s = 15
stop_resource_on_loss = true

# Home Assistant (#20): the fake the stage runs, on the loopback both the
# host and a --network host container share.
[homeassistant]
url = "ws://127.0.0.1:${FAKE_HA_PORT}/api/websocket"
token_file = "${STACK_DIR}/bridge-ha-token"

[[device]]
name = "door-a"
backend = "homeassistant"
entity = "${LOCKS['door-a']}"
role = "power"
resource = "${doorA.id}"
identity = "${STACK_DIR}/bridge-identities/door-a.toml"
stop_resource_on_loss = true

[[device]]
name = "door-b"
backend = "homeassistant"
entity = "${LOCKS['door-b']}"
role = "power"
resource = "${doorB.id}"
identity = "${STACK_DIR}/bridge-identities/door-b.toml"
stop_resource_on_loss = true

# The tool's enclosure contact, as a sensor module (#25): HA's on is open,
# which is door_open asserted.
[[device]]
name = "door-a-contact"
backend = "homeassistant"
entity = "${CONTACT}"
role = "sensor"
condition = "door_open"
resource = "bridge-${tag}"
identity = "${STACK_DIR}/bridge-identities/door-a-contact.toml"
`,
  )

  // Invites for the bridge to claim with its OWN register subcommand: one
  // per device, since each is its own identity on the platform.
  const invites = {}
  for (const [name, file] of [['extractor', 'bridge-invite'], ['door-a', 'bridge-invite-door-a'], ['door-b', 'bridge-invite-door-b'], ['door-a-contact', 'bridge-invite-door-a-contact']]) {
    const inv = await POST('/api/admin/devices/invite', { token: admin.token, body: { expires_in_hours: 1 } })
    invites[name] = inv.json?.data?.device_code
    fs.writeFileSync(path.join(STACK_DIR, file), invites[name] ?? '')
  }
  ok('bridge/invite-minted', Object.values(invites).every(Boolean), `invites: ${JSON.stringify(Object.keys(invites))}`)
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-tool-id'), `bridge-${tag}`)
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-card'), card)
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-door-a-id'), doorA.id ?? '')
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-door-b-id'), doorB.id ?? '')
  fs.writeFileSync(
    FIXTURE,
    JSON.stringify({ tag, toolId, adminToken: admin.token, edgeId: edge.id, doorAId: doorA.id, doorBId: doorB.id }, null, 2),
  )
  record('bridge/configs-written', 'ok')
}

function identityOf(name) {
  const text = fs.readFileSync(path.join(STACK_DIR, 'bridge-identities', `${name}.toml`), 'utf8')
  return { deviceId: /device_id\s*=\s*"([^"]+)"/.exec(text)?.[1], text }
}

async function bind() {
  const fixture = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'))
  const bindTo = (body) => POST('/api/admin/device-bindings', { token: fixture.adminToken, body })

  const { deviceId, text } = identityOf('extractor')
  ok('bridge/identity-has-device-id', !!deviceId, `identity file: ${text.slice(0, 80)}`)
  const binding = await bindTo({
    resource_id: fixture.toolId,
    device_id: deviceId,
    role: 'power',
    name: `bridge extractor ${fixture.tag}`,
    start_stage: 0,
    on_disconnect: 'fail_off',
  })
  assertEq('bridge/bound-as-power', 201, binding.status, binding.text.slice(0, 200))
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-device-id'), deviceId ?? '')

  // The locks (#20). Each door: the edge as coordinator, the lock as POWER
  // (what is leased) and as STRIKE (what gives the door its pulse -- a door
  // with no strike binding has a pulse of zero and never leases anything).
  // hold_open_access is false: the bridge is not on the scan-reply path an
  // open-access hold travels.
  for (const [name, doorId] of [['door-a', fixture.doorAId], ['door-b', fixture.doorBId]]) {
    const lock = identityOf(name)
    ok(`bridge/${name}-identity-has-device-id`, !!lock.deviceId, lock.text.slice(0, 80))
    const [edgeB, powerB, strikeB] = await Promise.all([
      bindTo({ resource_id: doorId, device_id: fixture.edgeId, role: 'edge', name: `${name} coordinator ${fixture.tag}` }),
      bindTo({ resource_id: doorId, device_id: lock.deviceId, role: 'power', name: `${name} lock ${fixture.tag}`, on_disconnect: 'fail_off' }),
      bindTo({
        resource_id: doorId,
        device_id: lock.deviceId,
        role: 'strike',
        name: `${name} pulse ${fixture.tag}`,
        pulse_ms: 3000,
        hold_open_access: false,
      }),
    ])
    ok(
      `bridge/${name}-bound-as-power-and-strike`,
      edgeB.status === 201 && powerB.status === 201 && strikeB.status === 201,
      `edge ${edgeB.status} power ${powerB.status} strike ${strikeB.status}: ${strikeB.text.slice(0, 160)}`,
    )
    fs.writeFileSync(path.join(STACK_DIR, `bridge-${name}-device-id`), lock.deviceId ?? '')
  }
  // The contact, bound as sensor on the TOOL (#25). fail_off: a contact the
  // edge has not heard from denies the tool, which is the point of one.
  const contact = identityOf('door-a-contact')
  ok('bridge/door-a-contact-identity-has-device-id', !!contact.deviceId, contact.text.slice(0, 80))
  const contactB = await bindTo({
    resource_id: fixture.toolId,
    device_id: contact.deviceId,
    role: 'sensor',
    name: `door-a contact ${fixture.tag}`,
    on_disconnect: 'fail_off',
  })
  assertEq('bridge/door-a-contact-bound-as-sensor', 201, contactB.status, contactB.text.slice(0, 160))
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-door-a-contact-device-id'), contact.deviceId ?? '')

  // Door B outlives its pulse, so stop-on-loss has a lease to end.
  const roomed = await PATCH(`/api/admin/resources/${fixture.doorBId}`, {
    token: fixture.adminToken,
    body: { session_profile: 'until_stopped' },
  })
  assertEq('bridge/door-b-is-until-stopped', 200, roomed.status, roomed.text.slice(0, 160))
}

async function republish() {
  const fixture = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'))
  for (const id of [fixture.doorAId, fixture.doorBId]) {
    const r = await POST(`/api/admin/doors/${id}/republish`, { token: fixture.adminToken })
    ok(`bridge/door-republished-${id === fixture.doorAId ? 'a' : 'b'}`, r.status === 200 || r.status === 202, `${r.status} ${r.text.slice(0, 120)}`)
  }
}

async function assertWindows() {
  const read = (name) => fs.readFileSync(path.join(STACK_DIR, name), 'utf8').trim()
  const ids = {
    deviceId: read('bridge-device-id'),
    toolId: read('bridge-tool-id'),
    friendly: FRIENDLY,
    doorA: read('bridge-door-a-id'),
    doorB: read('bridge-door-b-id'),
    lockA: read('bridge-door-a-device-id'),
    lockB: read('bridge-door-b-device-id'),
    contactId: read('bridge-door-a-contact-device-id'),
  }

  // Windows A-G, from the stage timeline (journeys/bridge-timeline.mjs says
  // why they are no longer captures of their own).
  const timelineFile = path.join(STACK_DIR, 'bridge-timeline.txt')
  const marksFile = `${timelineFile}.marks`
  const timeline = parseTimeline(fs.existsSync(timelineFile) ? fs.readFileSync(timelineFile, 'utf8') : '')
  const marks = parseMarks(fs.existsSync(marksFile) ? fs.readFileSync(marksFile, 'utf8') : '')
  for (const c of judgeBridgeTimeline(timeline, marks, ids)) ok(c.name, c.pass, c.detail)

  // ---- Home Assistant (#20): what the bridge asked HA to do -----------------
  // Not on MQTT, so not in the timeline: the fake HA's own record of calls.
  const calls = captured('fake-ha-calls.jsonl') ?? []
  const callsFor = (entity) => calls.filter((c) => c.target?.entity_id === entity).map((c) => `${c.domain}.${c.service}`)

  // E: door A's lock follows its lease both ways.
  const callsA = callsFor(LOCKS['door-a'])
  ok('bridge/E-lease-unlocks-the-door', callsA[0] === 'lock.unlock', `calls on ${LOCKS['door-a']}: ${JSON.stringify(callsA)}`)
  ok('bridge/E-withdrawal-relocks-the-door', callsA.length >= 2 && callsA[callsA.length - 1] === 'lock.lock', `calls: ${JSON.stringify(callsA)}`)
  const stateE = JSON.parse(fs.readFileSync(path.join(STACK_DIR, 'fake-ha-state.json'), 'utf8'))
  ok('bridge/E-door-a-ends-locked', stateE[LOCKS['door-a']] === 'locked', `final: ${stateE[LOCKS['door-a']]}`)

  // F: the until_stopped door unlocks on the badge, and the stop relocks it.
  const callsB = callsFor(LOCKS['door-b'])
  ok('bridge/F-badge-unlocks-the-room-door', callsB[0] === 'lock.unlock', `calls on ${LOCKS['door-b']}: ${JSON.stringify(callsB)}`)
  ok('bridge/F-stop-relocks-the-door', callsB.length >= 2 && callsB[callsB.length - 1] === 'lock.lock', `calls: ${JSON.stringify(callsB)}`)
}

main(async () => {
  const cmd = process.argv[2]
  if (cmd === 'setup') await setup()
  else if (cmd === 'bind') await bind()
  else if (cmd === 'republish') await republish()
  else if (cmd === 'assert') await assertWindows()
  else throw new Error(`bridge.mjs: unknown command ${cmd}`)
})
