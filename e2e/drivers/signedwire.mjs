// The local wire, signed (#249): css-edge against a real broker, on a tool an
// administrator set to Require signed messages.
//
// The lease stage already runs an edge and a broker and holds a member with a
// card. This driver adds one tool to that edge, and plays its reader in
// JavaScript: canonical bytes, HMAC and nonces written again here from
// FIRMWARE.md, not taken from css_wire, so the edge is checked against the
// document a firmware author reads rather than against its own code.
//
// Phases, each a separate run with its state in signedwire.json:
//   setup   before the edge starts: the tool, set to identify and to require
//           signed messages, bound to the lease edge, with a reader and a plug
//           registered here so their keys are known.
//   alive   the reader's first message, sealed, naming no edge nonce yet.
//   scans   after the stage captured the edge's hello: checks it, learns the
//           edge's nonce from it, and writes three scans with the member's
//           card -- unsigned, sealed, and that sealed one again (a replay).
//   assert  after the stage published each scan in its own window.
//
// The claims, each with its control:
//   - a stale signed message is answered with a hello, sealed for the reader
//     with ITS key, naming ITS nonce;
//   - an unsigned scan, the card perfectly good, is not acted on: no reply,
//     and the plug stays refused (the leases it IS sent are the control);
//   - a sealed scan is: a reply sealed for each device bound, a yes naming the
//     member, and leases to the plug, sealed with the plug's key, granting and
//     naming the member;
//   - the same sealed scan again is not: no reply.
//
// WHAT THIS DOES NOT PROVE: a forged message from a device not bound to the
// tool, or with another device's key. Those arms are edge/src/seal.rs's, unit
// tested; here every refusal looks alike from outside -- silence.

import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { PATCH, POST, RUN_TAG, adminAccount, assertEq, main, ok, record } from './lib.mjs'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const STATE = path.join(STACK_DIR, 'signedwire.json')
const load = () => JSON.parse(fs.readFileSync(STATE, 'utf8'))
const save = (s) => fs.writeFileSync(STATE, JSON.stringify(s))

// ── the seal, from FIRMWARE.md ("Signing the local wire") ─────────────────────

const item = (v) => `${Buffer.byteLength(v, 'utf8')}:${v}`
const opt = (v) => (v === undefined || v === null ? '~' : item(String(v)))

/** Each topic's own fields, in the document's order. */
const FIELDS = {
  'resource/request/alive': () => '',
  'resource/request/scan': (m) => item(m.resource) + item(m.card),
  'resource/hello': () => '',
  'resource/response/scan': (m) =>
    item(m.resource) +
    item(String(m.authorized)) +
    item(m.reason) +
    opt(m.strike?.pulse_ms) +
    (m.member ? item(m.member.id) + item(m.member.name) : '~~') +
    (m.sign_in
      ? item(m.sign_in.kind) + item(m.sign_in.code ?? m.sign_in.ticket ?? m.sign_in.reason)
      : '~~'),
  'resource/lease': (m) =>
    item(m.resource) +
    item(String(m.grant)) +
    item(String(m.ttl_ms)) +
    (m.member ? item(m.member.id) + item(m.member.name) : '~~'),
}

function canonical(topic, m, seal) {
  return (
    item('css-local/1') +
    item(topic) +
    opt(m.device_id) +
    item(seal.from) +
    item(seal.to) +
    item(String(seal.seq)) +
    FIELDS[topic](m)
  )
}

/** The key is the command_key string's own bytes. */
const mac = (key, bytes) => crypto.createHmac('sha256', Buffer.from(key, 'utf8')).update(bytes, 'utf8').digest('hex')

function seal(topic, m, key, from, to, seq) {
  const s = { from, to, seq }
  return { ...m, seal: { ...s, sig: mac(key, canonical(topic, m, s)) } }
}

/** Whether `m`, received on `topic`, is sealed correctly under `key`. */
function verifies(topic, m, key) {
  if (!m?.seal) return false
  const { sig, ...s } = m.seal
  return mac(key, canonical(topic, m, s)) === sig
}

// ── phases ────────────────────────────────────────────────────────────────────

async function claim(adminToken, name, role, mac) {
  const invite = await POST('/api/admin/devices/invite', { token: adminToken, body: { expires_in_hours: 1 } })
  const reg = await POST('/api/devices/register', {
    body: {
      device_code: invite.json?.data?.device_code, name, capabilities: { roles: [role] },
      mac_address: mac, software_version: '0.0.0-e2e', platform: 'linux',
    },
  })
  return {
    id: reg.json?.data?.device_id,
    key: reg.json?.data?.mqtt_config?.command_key ?? null,
    text: `register ${reg.status} ${reg.text.slice(0, 160)}`,
  }
}

/**
 * The oracle checked before it is trusted: FIRMWARE.md's worked example,
 * bytes and signature, and a changed field breaking it. A canonical() that
 * disagreed with the document would fail every arm below for the wrong reason.
 */
function selfTest() {
  const lease = {
    resource: 'res-1', device_id: 'dev-1', grant: true, ttl_ms: 3000, reason: null,
    member: { id: 'u-1', name: 'Cal' },
  }
  const s = { from: 'n-edge', to: 'n-mod', seq: 7 }
  assertEq('signedwire/the-oracle-builds-the-documented-bytes',
    '11:css-local/114:resource/lease5:dev-16:n-edge5:n-mod1:75:res-14:true4:30003:u-13:Cal',
    canonical('resource/lease', lease, s))
  const sealed = seal('resource/lease', lease, '0a0b0c0d', s.from, s.to, s.seq)
  assertEq('signedwire/the-oracle-signs-the-documented-signature',
    '7ae3d8164ad6034d6e8e3950a07adb684b52433806630c547da9d9ae8c56647f', sealed.seal.sig)
  ok('signedwire/the-oracle-refuses-a-changed-field',
    verifies('resource/lease', sealed, '0a0b0c0d') && !verifies('resource/lease', { ...sealed, ttl_ms: 3001 }, '0a0b0c0d'),
    'a tampered lease must not verify, and the untampered one must')
}

async function setup() {
  selfTest()
  const lease = JSON.parse(fs.readFileSync(path.join(STACK_DIR, 'lease-fixture.json'), 'utf8'))
  const admin = await adminAccount('signedwire_admin')
  const T = { token: admin.token }
  const ext = `signed-wire-${RUN_TAG}`
  const tool = await POST('/api/tools', {
    ...T,
    body: {
      name: `E2E signed wire ${RUN_TAG}`, category: 'other', requires_training: false, external_id: ext,
      usage_flat_fee: null, usage_rate_per_min: null, usage_max_session_minutes: null,
    },
  })
  const toolId = tool.json?.data?.id
  ok('signedwire/tool-created', !!toolId, tool.text.slice(0, 200))
  const bind = (device_id, role, extra = {}) => POST('/api/admin/device-bindings', {
    ...T, body: { resource_id: toolId, device_id, role, name: `signed ${role} ${RUN_TAG}`, ...extra },
  })
  assertEq('signedwire/edge-bound', 201, (await bind(lease.edgeId, 'edge')).status)
  const reader = await claim(admin.token, `signed-reader-${RUN_TAG}`, 'reader', '02:00:00:00:f2:01')
  const plug = await claim(admin.token, `signed-plug-${RUN_TAG}`, 'power', '02:00:00:00:f2:02')
  ok('signedwire/the-devices-hold-keys', !!reader.key && !!plug.key, `${reader.text}; ${plug.text}`)
  assertEq('signedwire/reader-bound', 201, (await bind(reader.id, 'reader')).status)
  // `ignore`, so the plug needs no reports of its own to be leased: the lease
  // is what the claims are about, not the plug's liveness.
  assertEq('signedwire/plug-bound', 201, (await bind(plug.id, 'power', { on_disconnect: 'ignore' })).status)
  const set = await PATCH(`/api/admin/resources/${toolId}`, { ...T, body: { identify_member: true, require_signed: true } })
  assertEq('signedwire/tool-requires-signed', 200, set.status, set.text.slice(0, 200))
  save({
    toolId, ext, card: lease.card, member: lease.memberId,
    reader: { id: reader.id, key: reader.key, nonce: crypto.randomBytes(16).toString('hex'), seq: 0 },
    plug: { id: plug.id, key: plug.key },
  })
  fs.writeFileSync(path.join(STACK_DIR, 'signedwire-tool'), ext)
}

/** The reader's next sealed message, naming the edge's nonce as last heard. */
function next(s, topic, m) {
  s.reader.seq += 1
  return seal(topic, m, s.reader.key, s.reader.nonce, s.reader.edge ?? '', s.reader.seq)
}

async function alive() {
  const s = load()
  const msg = next(s, 'resource/request/alive', { device_id: s.reader.id })
  save(s)
  fs.writeFileSync(path.join(STACK_DIR, 'signedwire-alive.json'), JSON.stringify(msg))
}

/** Every JSON object on its own line in a mosquitto_sub capture. */
function captured(file) {
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

async function scans() {
  const s = load()
  const hellos = captured('signedwire-hello.txt').filter((h) => h.device_id === s.reader.id)
  if (hellos.length === 0) {
    record('signedwire/a-stale-message-is-answered-with-a-hello', 'fail',
      'no resource/hello for the reader after its first sealed message; it could never name the edge')
    return
  }
  record('signedwire/a-stale-message-is-answered-with-a-hello', 'ok', `${hellos.length} hello(s)`)
  const h = hellos[0]
  ok('signedwire/the-hello-is-sealed-with-the-readers-key', verifies('resource/hello', h, s.reader.key),
    JSON.stringify(h).slice(0, 300))
  assertEq('signedwire/the-hello-names-the-readers-nonce', s.reader.nonce, h.seal?.to)
  s.reader.edge = h.seal?.from
  const body = { resource: s.ext, card: s.card, device_id: s.reader.id }
  fs.writeFileSync(path.join(STACK_DIR, 'signedwire-scan-unsigned.json'), JSON.stringify(body))
  const sealed = next(s, 'resource/request/scan', body)
  fs.writeFileSync(path.join(STACK_DIR, 'signedwire-scan-sealed.json'), JSON.stringify(sealed))
  save(s)
}

async function assertWire() {
  if (!fs.existsSync(STATE)) {
    record('signedwire/setup-ran', 'fail', 'no state: setup did not finish, so nothing below means anything')
    return
  }
  const s = load()
  const mine = (file) => captured(file).filter((r) => r.resource === s.ext || r.resource === s.toolId)
  const plugLeases = (file) => captured(file).filter((l) => l.device_id === s.plug.id)

  // ── the unsigned scan: nothing ──────────────────────────────────────────────
  const unsignedLeases = plugLeases('signedwire-unsigned-leases.txt')
  if (unsignedLeases.length === 0) {
    record('signedwire/the-plug-is-leased-to-at-all', 'fail',
      'no lease to the plug in the unsigned window, so its staying refused would prove nothing')
  } else {
    record('signedwire/the-plug-is-leased-to-at-all', 'ok', `${unsignedLeases.length} lease(s)`)
    ok('signedwire/an-unsigned-scan-opens-no-session', unsignedLeases.every((l) => l.grant === false),
      JSON.stringify(unsignedLeases.find((l) => l.grant !== false) ?? null).slice(0, 200))
  }
  assertEq('signedwire/an-unsigned-scan-gets-no-reply', 0, mine('signedwire-unsigned-reply.txt').length,
    JSON.stringify(mine('signedwire-unsigned-reply.txt')).slice(0, 300))

  // ── the sealed scan: acted on, and everything it causes is sealed ──────────
  const replies = mine('signedwire-sealed-reply.txt')
  const toReader = replies.filter((r) => r.device_id === s.reader.id)
  const toPlug = replies.filter((r) => r.device_id === s.plug.id)
  assertEq('signedwire/a-sealed-scan-gets-one-reply-per-device', '1,1', `${toReader.length},${toPlug.length}`,
    JSON.stringify(replies).slice(0, 400))
  ok('signedwire/no-reply-goes-out-unsigned', replies.every((r) => r.seal && r.device_id),
    JSON.stringify(replies.find((r) => !r.seal || !r.device_id) ?? null).slice(0, 200))
  const r = toReader[0]
  ok('signedwire/the-readers-reply-is-sealed-with-its-key', verifies('resource/response/scan', r, s.reader.key),
    JSON.stringify(r ?? null).slice(0, 300))
  ok('signedwire/the-plugs-reply-is-sealed-with-its-key', verifies('resource/response/scan', toPlug[0], s.plug.key),
    JSON.stringify(toPlug[0] ?? null).slice(0, 300))
  assertEq('signedwire/the-reply-names-the-readers-nonce', s.reader.nonce, r?.seal?.to)
  assertEq('signedwire/a-sealed-scan-is-a-yes', true, r?.authorized, JSON.stringify(r ?? null).slice(0, 200))
  assertEq('signedwire/the-reply-names-the-member', s.member, r?.member?.id)
  const granted = plugLeases('signedwire-sealed-leases.txt').filter((l) => l.grant === true)
  if (granted.length === 0) {
    record('signedwire/the-scan-leases-the-plug', 'fail',
      `no grant to the plug after the sealed scan: ${JSON.stringify(plugLeases('signedwire-sealed-leases.txt')).slice(0, 300)}`)
  } else {
    record('signedwire/the-scan-leases-the-plug', 'ok', `${granted.length} grant(s)`)
    ok('signedwire/every-lease-is-sealed-with-the-plugs-key',
      plugLeases('signedwire-sealed-leases.txt').every((l) => verifies('resource/lease', l, s.plug.key)),
      JSON.stringify(plugLeases('signedwire-sealed-leases.txt').find((l) => !verifies('resource/lease', l, s.plug.key)) ?? null).slice(0, 300))
    assertEq('signedwire/the-lease-names-the-member', s.member, granted[0].member?.id, JSON.stringify(granted[0] ?? null).slice(0, 300))
    const seqs = granted.map((l) => l.seal?.seq)
    ok('signedwire/each-lease-has-a-higher-seq', seqs.every((q, i) => i === 0 || q > seqs[i - 1]), JSON.stringify(seqs))
  }

  // ── the same sealed scan again: a replay, refused ──────────────────────────
  assertEq('signedwire/a-replayed-scan-gets-no-reply', 0, mine('signedwire-replay-reply.txt').length,
    JSON.stringify(mine('signedwire-replay-reply.txt')).slice(0, 300))
}

main(async () => {
  const mode = process.argv[2]
  if (mode === 'setup') await setup()
  else if (mode === 'alive') await alive()
  else if (mode === 'scans') await scans()
  else if (mode === 'assert') await assertWire()
  else throw new Error(`unknown mode ${mode}`)
})
