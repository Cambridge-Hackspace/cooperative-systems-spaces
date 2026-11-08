// A fake Home Assistant for the bridge stage: the half-dozen WebSocket frames
// spaces-iot-bridge speaks, and nothing else. Dependency-free on purpose --
// the drivers have no node_modules, and Node has a WebSocket CLIENT but no
// server -- so the RFC 6455 handshake and framing are done here by hand. It
// is a test fixture, not a WebSocket library: text frames up to 64 KiB, one
// client at a time is all it needs, and all it does.
//
// State is a JSON file under STACK_DIR so the stage can read it; every
// call_service lands in fake-ha-calls.jsonl, one line each, so the stage can
// assert what the bridge asked for. A lock answers `unlock` with `unlocking`
// then `unlocked` 300 ms later (and `lock` likewise), as a real lock does,
// which is what proves the bridge treats the transition as not-yet-a-fact.
// Dropping a JSON object into fake-ha-inject.json -- {"entity_id":...,
// "state":...} -- pushes that state to the client, the way HA would report a
// lock it can no longer see; the file is consumed.
//
//   node fake-ha.mjs <port> <entity_id>=<initial state> ...

import fs from 'node:fs'
import path from 'node:path'
import http from 'node:http'
import crypto from 'node:crypto'

const STACK_DIR = process.env.CSS_STACK_DIR ?? '/stack'
const STATE = path.join(STACK_DIR, 'fake-ha-state.json')
const CALLS = path.join(STACK_DIR, 'fake-ha-calls.jsonl')
const INJECT = path.join(STACK_DIR, 'fake-ha-inject.json')
const TOKEN = 'e2e-fake-ha-token'
const port = Number(process.argv[2] ?? 18123)

const state = {}
for (const arg of process.argv.slice(3)) {
  const [entity, initial] = arg.split('=')
  state[entity] = initial ?? 'locked'
}
const saveState = () => fs.writeFileSync(STATE, JSON.stringify(state, null, 2))
saveState()
fs.writeFileSync(CALLS, '')

const log = (...a) => console.log(new Date().toISOString(), 'fake-ha:', ...a)

// ---- framing -------------------------------------------------------------
function encodeText(text) {
  const payload = Buffer.from(text, 'utf8')
  const len = payload.length
  let header
  if (len < 126) header = Buffer.from([0x81, len])
  else if (len < 65536) {
    header = Buffer.alloc(4)
    header[0] = 0x81
    header[1] = 126
    header.writeUInt16BE(len, 2)
  } else throw new Error('frame too large for this fixture')
  return Buffer.concat([header, payload])
}

/** Parse as many complete frames as `buf` holds; returns [frames, rest]. */
function decodeFrames(buf) {
  const frames = []
  let off = 0
  while (buf.length - off >= 2) {
    const fin = (buf[off] & 0x80) !== 0
    const opcode = buf[off] & 0x0f
    const masked = (buf[off + 1] & 0x80) !== 0
    let len = buf[off + 1] & 0x7f
    let p = off + 2
    if (len === 126) {
      if (buf.length - p < 2) break
      len = buf.readUInt16BE(p)
      p += 2
    } else if (len === 127) {
      if (buf.length - p < 8) break
      len = Number(buf.readBigUInt64BE(p))
      p += 8
    }
    const maskLen = masked ? 4 : 0
    if (buf.length - p < maskLen + len) break
    const mask = masked ? buf.subarray(p, p + 4) : null
    p += maskLen
    const payload = Buffer.from(buf.subarray(p, p + len))
    if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i & 3]
    p += len
    frames.push({ fin, opcode, payload })
    off = p
  }
  return [frames, buf.subarray(off)]
}

// ---- the server ------------------------------------------------------------
let client = null // { socket, send(obj), subId }

function sendTo(c, obj) {
  if (!c || c.socket.destroyed) return
  c.socket.write(encodeText(JSON.stringify(obj)))
}

function stateChanged(entity, newState) {
  state[entity] = newState
  saveState()
  if (client?.subId != null) {
    sendTo(client, {
      id: client.subId,
      type: 'event',
      event: {
        event_type: 'state_changed',
        data: {
          entity_id: entity,
          old_state: null,
          new_state: newState == null ? null : { entity_id: entity, state: newState, attributes: {} },
        },
      },
    })
  }
}

function handle(c, msg) {
  if (!c.authed) {
    if (msg.type === 'auth' && msg.access_token === TOKEN) {
      c.authed = true
      sendTo(c, { type: 'auth_ok', ha_version: 'fake' })
      log('authenticated')
    } else {
      sendTo(c, { type: 'auth_invalid', message: 'Invalid access token' })
      c.socket.end()
    }
    return
  }
  const id = msg.id
  switch (msg.type) {
    case 'subscribe_events':
      c.subId = id
      sendTo(c, { id, type: 'result', success: true, result: null })
      break
    case 'get_states':
      sendTo(c, {
        id,
        type: 'result',
        success: true,
        result: Object.entries(state).map(([entity_id, s]) => ({ entity_id, state: s, attributes: {} })),
      })
      break
    case 'call_service': {
      fs.appendFileSync(CALLS, JSON.stringify({ at: new Date().toISOString(), ...msg }) + '\n')
      const entity = msg.target?.entity_id
      log(`call_service ${msg.domain}.${msg.service} ${entity}`)
      if (!(entity in state)) {
        sendTo(c, { id, type: 'result', success: false, error: { code: 'not_found', message: `no entity ${entity}` } })
        break
      }
      sendTo(c, { id, type: 'result', success: true, result: null })
      if (msg.domain === 'lock') {
        const target = msg.service === 'unlock' ? 'unlocked' : 'locked'
        stateChanged(entity, msg.service === 'unlock' ? 'unlocking' : 'locking')
        setTimeout(() => stateChanged(entity, target), 300)
      } else if (msg.domain === 'switch' || msg.domain === 'light') {
        stateChanged(entity, msg.service === 'turn_on' ? 'on' : 'off')
      } else if (msg.domain === 'homeassistant' && msg.service === 'update_entity') {
        // A refresh that changes nothing produces no event, as in HA.
      }
      break
    }
    case 'ping':
      sendTo(c, { id, type: 'pong' })
      break
    default:
      sendTo(c, { id, type: 'result', success: false, error: { code: 'unknown_command', message: msg.type } })
  }
}

const server = http.createServer((req, res) => {
  res.writeHead(426, { 'Content-Type': 'text/plain' })
  res.end('WebSocket only\n')
})
server.on('upgrade', (req, socket) => {
  const key = req.headers['sec-websocket-key']
  if (!key || (req.headers.upgrade ?? '').toLowerCase() !== 'websocket') {
    socket.end('HTTP/1.1 400 Bad Request\r\n\r\n')
    return
  }
  const accept = crypto
    .createHash('sha1')
    .update(key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11')
    .digest('base64')
  socket.write(
    'HTTP/1.1 101 Switching Protocols\r\n' +
      'Upgrade: websocket\r\n' +
      'Connection: Upgrade\r\n' +
      `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
  )
  const c = { socket, authed: false, subId: null }
  if (client && !client.socket.destroyed) {
    log('a second client connected; the first is dropped (the fixture serves one)')
    client.socket.destroy()
  }
  client = c
  let buf = Buffer.alloc(0)
  log(`client connected from ${socket.remoteAddress}`)
  sendTo(c, { type: 'auth_required', ha_version: 'fake' })
  socket.on('data', (chunk) => {
    buf = Buffer.concat([buf, chunk])
    const [frames, rest] = decodeFrames(buf)
    buf = rest
    for (const f of frames) {
      if (f.opcode === 0x8) {
        socket.end()
        return
      }
      if (f.opcode === 0x9) {
        // ping -> pong, same payload
        const pong = Buffer.concat([Buffer.from([0x8a, f.payload.length]), f.payload])
        socket.write(pong)
        continue
      }
      if (f.opcode !== 0x1) continue
      let msg
      try {
        msg = JSON.parse(f.payload.toString('utf8'))
      } catch {
        continue
      }
      handle(c, msg)
    }
  })
  socket.on('close', () => {
    log('client gone')
    if (client === c) client = null
  })
  socket.on('error', (e) => log(`socket error: ${e.message}`))
})

// The stage's hand on the fixture: a state to push, as HA would report it.
setInterval(() => {
  if (!fs.existsSync(INJECT)) return
  let inj
  try {
    inj = JSON.parse(fs.readFileSync(INJECT, 'utf8'))
  } catch {
    return
  }
  fs.unlinkSync(INJECT)
  if (inj?.entity_id) {
    log(`inject ${inj.entity_id} = ${inj.state}`)
    stateChanged(inj.entity_id, inj.state)
  }
}, 200)

server.listen(port, '127.0.0.1', () => log(`listening on ws://127.0.0.1:${port}/api/websocket with ${JSON.stringify(state)}`))
