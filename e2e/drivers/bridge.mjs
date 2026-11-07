// The IoT bridge against a real edge. See stage_bridge in run.sh for the
// windows; this driver does the platform setup and reads the captures.
//
//   setup   admin, tool, member+card, the EDGE device registered and bound,
//           both configs under STACK_DIR, broker logins, and an invite written
//           to bridge-invite for the bridge to claim itself with.
//   bind    the identity the bridge wrote -> bound as power, stage 0, fail_off.
//   assert  the captured windows, one case per claim.

import fs from 'node:fs'
import path from 'node:path'
import { POST, PUT, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const FIXTURE = path.join(STACK_DIR, 'bridge-fixture.json')
const mqttPort = process.argv[3] ?? '1883'
const devicePepper = process.argv[4] ?? ''
const FRIENDLY = 'e2e/plug/extractor'

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
`,
  )

  // An invite for the bridge to claim with its OWN register subcommand.
  const bridgeInvite = await POST('/api/admin/devices/invite', { token: admin.token, body: { expires_in_hours: 1 } })
  const code = bridgeInvite.json?.data?.device_code
  ok('bridge/invite-minted', !!code, `invite -> ${bridgeInvite.status}`)
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-invite'), code ?? '')
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-tool-id'), `bridge-${tag}`)
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-card'), card)
  fs.writeFileSync(FIXTURE, JSON.stringify({ tag, toolId, adminToken: admin.token, edgeId: edge.id }, null, 2))
  record('bridge/configs-written', 'ok')
}

async function bind() {
  const fixture = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'))
  const idFile = path.join(STACK_DIR, 'bridge-identities', 'extractor.toml')
  const text = fs.readFileSync(idFile, 'utf8')
  const deviceId = /device_id\s*=\s*"([^"]+)"/.exec(text)?.[1]
  ok('bridge/identity-has-device-id', !!deviceId, `identity file: ${text.slice(0, 80)}`)
  const binding = await POST('/api/admin/device-bindings', {
    token: fixture.adminToken,
    body: {
      resource_id: fixture.toolId,
      device_id: deviceId,
      role: 'power',
      name: `bridge extractor ${fixture.tag}`,
      start_stage: 0,
      on_disconnect: 'fail_off',
    },
  })
  assertEq('bridge/bound-as-power', 201, binding.status, binding.text.slice(0, 200))
  fs.writeFileSync(path.join(STACK_DIR, 'bridge-device-id'), deviceId ?? '')
}

async function assertWindows() {
  const deviceId = fs.readFileSync(path.join(STACK_DIR, 'bridge-device-id'), 'utf8').trim()
  const tool = fs.readFileSync(path.join(STACK_DIR, 'bridge-tool-id'), 'utf8').trim()
  const mine = (rows) => (rows ?? []).filter((r) => r.device_id === deviceId)

  // A: silent plug -> relay_on omitted on every one of OUR reports.
  const a = mine(captured('bridge-power-A.txt'))
  ok('bridge/A-bridge-reports-while-plug-silent', a.length >= 1, `captured ${a.length} of our power reports`)
  ok(
    'bridge/A-silent-plug-omits-relay_on',
    a.length >= 1 && a.every((r) => !('relay_on' in r)),
    `relay_on fields: ${JSON.stringify(a.map((r) => r.relay_on))} -- "cannot report" must not be reported as off`,
  )

  // B: grant -> ON with on_time; fake plug answers -> relay_on: true.
  const setB = captured('bridge-set-B.txt') ?? []
  const onCmd = setB.find((c) => c.state === 'ON')
  ok('bridge/B-grant-commands-plug-on', !!onCmd, `set commands: ${JSON.stringify(setB)}`)
  ok('bridge/B-on-carries-on_time', onCmd?.on_time === 60, `on_time: ${JSON.stringify(onCmd?.on_time)}`)
  const b = mine(captured('bridge-power-B.txt'))
  ok(
    'bridge/B-reported-on-becomes-relay_on-true',
    b.some((r) => r.relay_on === true),
    `relay_on after the plug reported ON: ${JSON.stringify(b.map((r) => r.relay_on))}`,
  )

  // C: the signature. Restarted under a retained ON: it must /get, and omit
  // relay_on until a live report answers.
  const getC = captured('bridge-get-C.txt') ?? []
  ok('bridge/C-retained-replay-triggers-a-get', getC.length >= 1, `get requests: ${getC.length}`)
  const c1 = mine(captured('bridge-power-C1.txt'))
  ok(
    'bridge/C-retained-replay-is-not-believed',
    c1.length >= 1 && c1.every((r) => !('relay_on' in r)),
    `relay_on under a retained ON, before any live report: ${JSON.stringify(c1.map((r) => r.relay_on))}`,
  )
  const c2 = mine(captured('bridge-power-C2.txt'))
  ok(
    'bridge/C-live-report-resolves-unknown',
    c2.some((r) => r.relay_on === true),
    `relay_on after a live ON: ${JSON.stringify(c2.map((r) => r.relay_on))}`,
  )

  // D: plug gone -> stop-on-loss -> refusal -> OFF.
  const stops = (captured('bridge-stop-D.txt') ?? []).filter((s) => s.resource === tool)
  ok('bridge/D-loss-stops-the-resource', stops.length >= 1, `stops for ${tool}: ${stops.length}`)
  const setD = captured('bridge-set-D.txt') ?? []
  ok('bridge/D-refusal-switches-plug-off', setD.some((c) => c.state === 'OFF'), `set commands: ${JSON.stringify(setD)}`)
}

main(async () => {
  const cmd = process.argv[2]
  if (cmd === 'setup') await setup()
  else if (cmd === 'bind') await bind()
  else if (cmd === 'assert') await assertWindows()
  else throw new Error(`bridge.mjs: unknown command ${cmd}`)
})
