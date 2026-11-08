// A sensor's reading reaches the coordinator over the wire (#213), against a
// real edge. See stage_conditions in run.sh for the windows; this driver does
// the platform setup and reads the captures.
//
//   setup   admin, a tool, member+card, the EDGE registered and bound, a plug
//           (power, fail_off), a sensor (sensor, fail_off), a `door_open` trip
//           rule on the tool, the edge's config under STACK_DIR.
//   assert  the captured lease windows, one case per claim.

import fs from 'node:fs'
import path from 'node:path'
import { POST, PUT, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const mqttPort = process.argv[3] ?? '1883'
const devicePepper = process.argv[4] ?? ''

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
  const admin = await adminAccount('cond_admin')
  const tag = admin.username
  const base = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'

  const tool = await POST('/api/tools', {
    token: admin.token,
    body: { name: `Condition Mill ${tag}`, category: 'other', external_id: `cond-${tag}` },
  })
  const toolId = tool.json?.data?.id
  ok('conditions/tool-created', !!toolId, `POST /api/tools -> ${tool.status}`)

  const claim = async (name, roles, mac) => {
    const invite = await POST('/api/admin/devices/invite', { token: admin.token, body: { expires_in_hours: 1 } })
    const reg = await POST('/api/devices/register', {
      body: {
        device_code: invite.json?.data?.device_code,
        name,
        capabilities: { roles },
        mac_address: mac,
        software_version: '0.0.0-e2e',
        platform: 'linux',
      },
    })
    return {
      id: reg.json?.data?.device_id ?? reg.json?.device_id,
      token: reg.json?.data?.auth_token ?? reg.json?.auth_token,
      commandKey: reg.json?.data?.mqtt_config?.command_key ?? null,
      status: reg.status,
    }
  }
  const edge = await claim(`cond-edge-${tag}`, ['edge'], '02:00:00:00:85:01')
  const plug = await claim(`cond-plug-${tag}`, ['power'], '02:00:00:00:85:02')
  const sensor = await claim(`cond-sensor-${tag}`, ['sensor'], '02:00:00:00:85:03')
  ok('conditions/devices-registered', !!edge.id && !!plug.id && !!sensor.id, `${edge.status} ${plug.status} ${sensor.status}`)

  const bind = (body) => POST('/api/admin/device-bindings', { token: admin.token, body })
  const [edgeB, plugB, sensorB] = await Promise.all([
    bind({ resource_id: toolId, device_id: edge.id, role: 'edge', name: `cond coordinator ${tag}` }),
    bind({ resource_id: toolId, device_id: plug.id, role: 'power', name: `cond plug ${tag}`, on_disconnect: 'fail_off' }),
    bind({ resource_id: toolId, device_id: sensor.id, role: 'sensor', name: `cond door contact ${tag}`, on_disconnect: 'fail_off' }),
  ])
  ok('conditions/devices-bound', [edgeB, plugB, sensorB].every((b) => b.status === 201), `${edgeB.status} ${plugB.status} ${sensorB.status}`)

  // The rule the reading is for. Edge-enforced; the default latch stays: a
  // trip holds until re-authorization, which window 5 asserts.
  const trip = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: { tool_id: toolId, kind: 'trip', condition: 'door_open', enforcement: 'edge' },
  })
  assertEq('conditions/trip-rule-created', 201, trip.status, trip.text.slice(0, 160))

  const member = await account(`cond_member_${tag}`)
  const card = `COND-CARD-${tag}`.toUpperCase()
  const cardSet = await PUT(`/api/profiles/${member.user.id}`, { token: member.token, body: { profile: { card_id: card } } })
  assertEq('conditions/member-card-set', 200, cardSet.status, cardSet.text.slice(0, 160))

  // A long offline window: one alive pulse per device at the start of the
  // stage keeps liveness out of every denial's reason below.
  fs.writeFileSync(
    path.join(STACK_DIR, 'edge-cond.config.toml'),
    `name = "e2e-cond-edge"
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
mqtt_client_id = "e2e-cond-edge"
mqtt_namespace = "cs/spaces"

[remote_device_info]
remote_id = "${edge.id}"
remote_auth_token = "${edge.token}"
remote_instance_url = "${base}"
`,
  )
  fs.writeFileSync(path.join(STACK_DIR, 'cond-tool'), `cond-${tag}`)
  fs.writeFileSync(path.join(STACK_DIR, 'cond-card'), card)
  fs.writeFileSync(path.join(STACK_DIR, 'cond-plug-id'), plug.id)
  fs.writeFileSync(path.join(STACK_DIR, 'cond-sensor-id'), sensor.id)
  record('conditions/configs-written', 'ok')
}

async function assertWindows() {
  const plug = fs.readFileSync(path.join(STACK_DIR, 'cond-plug-id'), 'utf8').trim()
  const forPlug = (name) => (captured(name) ?? []).filter((l) => l.device_id === plug)
  const grants = (rows) => rows.map((l) => l.grant)
  const reasons = (rows) => [...new Set(rows.map((l) => l.reason).filter(Boolean))]

  // 1. Signed on, sensor alive, nothing observed: refused, and the reason is
  //    the unobserved condition -- not "offline", not "not in use".
  const w1 = forPlug('cond-lease-1.txt')
  ok('conditions/1-unobserved-condition-refuses', w1.length >= 2 && w1.every((l) => l.grant === false), `grants: ${JSON.stringify(grants(w1))}`)
  ok('conditions/1-the-reason-names-the-condition', reasons(w1).some((r) => r.includes('Unobserved') && r.includes('door_open')), `reasons: ${JSON.stringify(reasons(w1))}`)

  // 2. A stranger said the door is shut. It must not count.
  const w2 = forPlug('cond-lease-2.txt')
  ok('conditions/2-a-strangers-reading-changes-nothing', w2.length >= 2 && w2.every((l) => l.grant === false), `grants after the stranger: ${JSON.stringify(grants(w2))}`)
  ok('conditions/2-still-unobserved', reasons(w2).some((r) => r.includes('Unobserved')), `reasons: ${JSON.stringify(reasons(w2))}`)

  // 3. The bound sensor says the door is shut: granted.
  const w3 = forPlug('cond-lease-3.txt')
  ok('conditions/3-the-sensors-reading-is-observed-and-grants', w3.some((l) => l.grant === true), `grants: ${JSON.stringify(grants(w3))}`)

  // 4. The door opens mid-session: tripped, by name.
  const w4 = forPlug('cond-lease-4.txt')
  ok('conditions/4-door-open-trips-the-tool', w4.length >= 2 && w4.slice(-2).every((l) => l.grant === false), `grants: ${JSON.stringify(grants(w4))}`)
  ok('conditions/4-the-reason-is-the-trip', reasons(w4).some((r) => r.includes('Tripped') && r.includes('door_open')), `reasons: ${JSON.stringify(reasons(w4))}`)

  // 5. The door shuts again: a trip latches, so still refused until re-auth.
  const w5 = forPlug('cond-lease-5.txt')
  ok('conditions/5-a-trip-latches-past-the-condition-clearing', w5.length >= 2 && w5.every((l) => l.grant === false), `grants: ${JSON.stringify(grants(w5))}`)
  ok('conditions/5-the-reason-says-latched', reasons(w5).some((r) => r.includes('Latched') && r.includes('door_open')), `reasons: ${JSON.stringify(reasons(w5))}`)
}

main(async () => {
  const cmd = process.argv[2]
  if (cmd === 'setup') await setup()
  else if (cmd === 'assert') await assertWindows()
  else throw new Error(`conditions.mjs: unknown command ${cmd}`)
})
