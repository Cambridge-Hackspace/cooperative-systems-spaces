// A card tap DECIDED AT THE EDGE carries the server's sign-in (#241, stage 2).
//
// The oidc stage proves the server's half: a scan the server decides carries
// sign_in, and its code redeems. This proves the edge's: a tool set to sign in
// is decided by the edge from its principals, and the edge then asks the
// server for the sign-in and puts it on the reply it publishes on
// resource/response/scan -- the bytes a reader's host actually receives.
//
// Phases, run by the lease stage, which already has an edge, a broker and a
// member with a card:
//   setup   before the edge starts: a sign-in app, and a tool bound to the
//           lease edge, set to identify and to sign in, so the edge's first
//           policy sync carries it.
//   assert  after the stage published a scan with the member's card and
//           captured the reply: a yes, the member named, a code; the tool
//           still idle; and the code is the server's, shown by redeeming it
//           once it has expired -- "expired", which only a code the server
//           issued can be, and not "unknown".
//
// THE PLUG. A sign-in opens no session at the edge, and an edge session is
// visible from outside in exactly one way: the edge leases the tool's power
// devices. The server's tool status cannot show it -- a mutant that opened
// the session anyway left the status idle, since the scan it forwards is
// answered as one more sign-in. So the tool has a plug, bound `ignore` so it
// needs no power reports to be leased, and the tap must grant it nothing. The
// same capture is the control: the edge refuses that plug NotInUse while it
// is idle, so rows for it prove the window watched the right device.
//
// WHAT THIS DOES NOT PROVE: the edge answering with no sign-in when the server
// is unreachable. That arm is with_server_sign_in's, unit-tested in
// edge/src/mqtt.rs; taking the server down mid-stage would cost every lease
// window after it.

import fs from 'node:fs'
import path from 'node:path'
import { GET, PATCH, POST, RUN_TAG, adminAccount, boundDevice, main, ok, assertEq, record } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const FIXTURE = path.join(STACK_DIR, 'edgesignin.json')
const BASE = process.env.CSS_BASE_URL ?? 'http://127.0.0.1:4399'

async function setup() {
  const lease = JSON.parse(fs.readFileSync(path.join(STACK_DIR, 'lease-fixture.json'), 'utf8'))
  const admin = await adminAccount('edgesignin_admin')
  const T = { token: admin.token }
  const ext = `edge-sign-in-${RUN_TAG}`
  const tool = await POST('/api/tools', {
    ...T,
    body: {
      name: `E2E edge sign-in ${RUN_TAG}`, category: 'other', requires_training: false, external_id: ext,
      usage_flat_fee: null, usage_rate_per_min: null, usage_max_session_minutes: null,
    },
  })
  const toolId = tool.json?.data?.id
  ok('edgesignin/tool-created', !!toolId, tool.text.slice(0, 200))
  const bound = await POST('/api/admin/device-bindings', {
    ...T, body: { resource_id: toolId, device_id: lease.edgeId, role: 'edge', name: `sign-in edge ${RUN_TAG}` },
  })
  assertEq('edgesignin/edge-bound', 201, bound.status, bound.text.slice(0, 200))
  const plug = await boundDevice(admin.token, toolId, {
    name: `sign-in plug ${RUN_TAG}`, mac: '02:00:00:00:f1:01', onDisconnect: 'ignore',
  })
  ok('edgesignin/plug-bound', !!plug.id && plug.bindStatus === 201, plug.text)
  const app = (await POST('/api/admin/oauth/clients', {
    ...T,
    body: {
      name: `Edge POP ${RUN_TAG}`, redirect_uris: ['https://pop.e2e.invalid/auth/callback'], skip_consent: true,
      allowed_scopes: ['openid', 'profile', 'training:read'],
    },
  })).json?.data
  ok('edgesignin/app-registered', !!app?.client?.id, JSON.stringify(app).slice(0, 200))
  const set = await PATCH(`/api/admin/resources/${toolId}`, {
    ...T, body: { identify_member: true, sign_in_app: app?.client?.id },
  })
  assertEq('edgesignin/tool-signs-in', 200, set.status, set.text.slice(0, 200))
  fs.writeFileSync(FIXTURE, JSON.stringify({
    toolId, ext, admin: admin.token, member: lease.memberId, plug: plug.id,
    client: app?.client?.client_id, secret: app?.client_secret,
  }))
  fs.writeFileSync(path.join(STACK_DIR, 'edgesignin-tool'), ext)
}

/** Every JSON object on its own line in a mosquitto_sub capture. */
function replies(file) {
  let raw
  try {
    raw = fs.readFileSync(path.join(STACK_DIR, file), 'utf8')
  } catch {
    return []
  }
  return raw.split('\n').map((l) => l.trim()).filter(Boolean).flatMap((l) => {
    try {
      return [JSON.parse(l)]
    } catch {
      return []
    }
  })
}

async function assertReply() {
  if (!fs.existsSync(FIXTURE)) {
    record('edgesignin/setup-ran', 'fail', 'no fixture: the setup phase did not finish, so nothing below means anything')
    return
  }
  const f = JSON.parse(fs.readFileSync(FIXTURE, 'utf8'))
  const ours = replies('edgesignin-reply.txt').filter((r) => r.resource === f.ext)
  // The precondition first: the edge answered at all.
  if (ours.length === 0) {
    record('edgesignin/the-edge-answers', 'fail', `nothing on resource/response/scan for ${f.ext}`)
    return
  }
  record('edgesignin/the-edge-answers', 'ok', `${ours.length} reply(ies)`)
  const r = ours[0]
  assertEq('edgesignin/a-tap-decided-at-the-edge-is-a-yes', true, r.authorized, JSON.stringify(r).slice(0, 200))
  assertEq('edgesignin/it-names-the-member', f.member, r.member?.id, JSON.stringify(r).slice(0, 200))
  assertEq('edgesignin/it-carries-the-server-s-code', 'code', r.sign_in?.kind, JSON.stringify(r).slice(0, 200))
  // A sign-in leases nothing: control first, then the absence.
  const plugRows = replies('edgesignin-lease.txt').filter((l) => l.device_id === f.plug)
  if (plugRows.length === 0) {
    record('edgesignin/the-plug-is-leased-to-at-all', 'fail',
      `no lease row for the plug ${f.plug} during the tap, so its absence of a grant proves nothing`)
  } else {
    record('edgesignin/the-plug-is-leased-to-at-all', 'ok', `${plugRows.length} row(s)`)
    ok('edgesignin/a-sign-in-leases-nothing', plugRows.every((l) => l.grant === false),
      `the tap granted the plug: ${JSON.stringify(plugRows.find((l) => l.grant !== false) ?? null).slice(0, 200)}`)
  }
  const status = (await GET(`/api/tools/${f.toolId}`, { token: f.admin })).json?.data?.status
  assertEq('edgesignin/a-sign-in-at-the-edge-occupies-nothing', 'idle', status)
  // The code is the server's: once expired it is "expired", which only a
  // code the server issued can be, never "unknown".
  await new Promise((done) => setTimeout(done, 6000))
  const res = await fetch(new URL('/api/oauth/token', BASE), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Authorization: 'Basic ' + Buffer.from(`${encodeURIComponent(f.client)}:${encodeURIComponent(f.secret)}`).toString('base64'),
    },
    body: new URLSearchParams({ grant_type: 'urn:css:card-sign-in', code: r.sign_in?.code ?? '' }).toString(),
  })
  const body = await res.json().catch(() => ({}))
  ok('edgesignin/the-code-is-one-the-server-issued', res.status === 400 && /expired/.test(body.error_description ?? ''),
    `${res.status} ${JSON.stringify(body).slice(0, 200)}`)
}

main(async () => {
  const phase = process.argv[2]
  if (phase === 'setup') return setup()
  if (phase === 'assert') return assertReply()
  record('edgesignin/driver-phase', 'fail', `unknown phase ${JSON.stringify(phase)}`)
})
