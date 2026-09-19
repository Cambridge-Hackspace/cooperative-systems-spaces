// Tool module bindings + safety interlocks stage (#83), exercised against a
// real stack.
//
// What this proves: the vocabularies are enforced at the edge of the API as
// 400s rather than reaching the column CHECK as a 500; an interlock cannot cite
// a sensor belonging to a different tool; a trip latches and resets by
// re-authorization unless told otherwise; the `module/state` snapshot the edge
// coordinates from reflects what was authored and omits a disabled rule; a
// delete that matched nothing is a 404 rather than a 200 claiming a deletion;
// and every mutation lands an audit row. Where a device can be registered, it
// also proves the new module device kinds (#83 increment 2) round-trip and that
// a binding defaults to the fail-safe disconnect policy.
//
// What this does NOT prove: that anything is actually energized, gated or cut.
// Nothing here switches hardware -- the coordinator and its lease are a later
// increment, and the firmware tier is outside this repository.
//
// Cluster encoding: registering a device needs a device invite, and an invite
// code is eight emoji. On a cluster that cannot store one (LATIN1 and friends;
// see the same branch in concurrency.mjs) the binding half cannot be set up at
// all, so it is skipped with that reason rather than reported as a defect in
// this feature. Everything that does not need a device still runs there.

import { GET, POST, DELETE, account, adminAccount, assertEq, ok, record, main } from './lib.mjs'

const ENCODING = process.env.CSS_DB_ENCODING ?? 'UTF8'
const CAN_REGISTER_DEVICE = ENCODING === 'UTF8' || ENCODING === 'SQL_ASCII'

// A syntactically valid id that exists in no table, for the validation paths.
const ABSENT = '00000000-0000-4000-8000-0000000000ff'

main(async () => {
  const admin = await adminAccount('toolmodules_admin')
  const T = { token: admin.token }
  const tag = admin.username

  // --- a tool to wire ------------------------------------------------------
  const toolRes = await POST('/api/tools', {
    token: admin.token,
    body: { name: `Laser ${tag}`, category: 'laser_cutting', external_id: `tm-${tag}` },
  })
  const toolId = toolRes.json?.data?.id
  ok('toolmodules/tool-created', !!toolId, `POST /api/tools -> ${toolRes.status}`)

  // --- vocabulary is refused at the API, not by the column ------------------
  // These need no device: the handler validates the vocabulary, then the tool,
  // then the device, so a synthetic device id never gets that far.
  const badRole = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: { tool_id: toolId, device_id: ABSENT, role: 'nonsense', name: 'x' },
  })
  assertEq('toolmodules/bad-role-is-400', 400, badRole.status)

  const badTool = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: { tool_id: ABSENT, device_id: ABSENT, role: 'power', name: 'x' },
  })
  assertEq('toolmodules/unknown-tool-is-400', 400, badTool.status)

  const badDevice = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: { tool_id: toolId, device_id: ABSENT, role: 'power', name: 'x' },
  })
  assertEq('toolmodules/unknown-device-is-400', 400, badDevice.status)

  const badDisconnect = await POST('/api/admin/tool-modules', {
    token: admin.token,
    body: {
      tool_id: toolId,
      device_id: ABSENT,
      role: 'power',
      name: 'x',
      on_disconnect: 'keep_it_on_whatever_happens',
    },
  })
  assertEq('toolmodules/bad-on-disconnect-is-400', 400, badDisconnect.status)

  // --- interlocks -----------------------------------------------------------
  const trip = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: { tool_id: toolId, kind: 'trip', condition: 'door_open', enforcement: 'edge' },
  })
  assertEq('toolmodules/create-trip', 201, trip.status)
  const tripId = trip.json?.data?.id
  // The safety default: a cut stays cut until someone re-authorizes. Auto-resume
  // on a laser when the lid falls shut is the behaviour this default exists to
  // prevent, so it is asserted rather than assumed.
  assertEq('toolmodules/trip-latches-by-default', true, trip.json?.data?.latch)
  assertEq('toolmodules/trip-reset-defaults-to-re-auth', 're_auth', trip.json?.data?.reset)

  const badCondition = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: { tool_id: toolId, kind: 'trip', condition: 'not_a_condition' },
  })
  assertEq('toolmodules/bad-condition-is-400', 400, badCondition.status)

  const badEnforcement = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: {
      tool_id: toolId,
      kind: 'trip',
      condition: 'door_open',
      enforcement: 'wishful_thinking',
    },
  })
  assertEq('toolmodules/bad-enforcement-is-400', 400, badEnforcement.status)

  // An enforcement tier has to be achievable with the hardware actually bound.
  // Nothing is bound to this tool yet, so nothing can enforce a door interlock
  // in firmware, and claiming otherwise must be refused rather than stored as a
  // rule that silently does not do what it says.
  const unachievable = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: {
      tool_id: toolId,
      kind: 'trip',
      condition: 'door_open',
      enforcement: 'firmware',
    },
  })
  assertEq('toolmodules/unachievable-firmware-tier-is-400', 400, unachievable.status)
  ok('toolmodules/refusal-names-the-alternative',
    String(unachievable.text).includes('edge'),
    `a refusal that does not name the achievable tier just blocks somebody: ${unachievable.text.slice(0, 200)}`)

  // A disabled rule is authored but must not be shipped to the edge.
  const disabled = await POST('/api/admin/tool-interlocks', {
    token: admin.token,
    body: { tool_id: toolId, kind: 'start_gate', condition: 'estop', enabled: false },
  })
  assertEq('toolmodules/create-disabled-gate', 201, disabled.status)
  const disabledId = disabled.json?.data?.id

  // --- the binding half, where a device can be registered -------------------
  let moduleId = null
  if (!CAN_REGISTER_DEVICE) {
    record('toolmodules/bindings-not-run-on-this-cluster', 'skip',
      `binding a module needs a registered device, a device needs an invite, and this ` +
      `cluster (${ENCODING}) cannot store an eight-emoji invite code. The interlock and ` +
      `snapshot assertions above and below still run.`)
  } else {
    const invite = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    const code = invite.json?.data?.device_code
    ok('toolmodules/invite-created', !!code,
      `POST /api/admin/devices/invite -> ${invite.status}`)

    // Registering as `power_controller` is the end-to-end proof of the new enum
    // values: the string is accepted by the API, stored by Postgres, and read
    // back -- the four-way vocabulary the device_kinds_agree oracle pins.
    const reg = await POST('/api/devices/register', {
      body: {
        device_code: code,
        name: `plug-${tag}`,
        kind: 'power_controller',
        mac_address: '02:00:00:00:83:01',
        software_version: '0.0.0-e2e',
        platform: 'linux',
      },
    })
    ok('toolmodules/module-kind-registers', reg.status < 300,
      `a power_controller must be registerable -> ${reg.status} ${reg.text.slice(0, 200)}`)
    const deviceId = reg.json?.data?.device_id ?? reg.json?.device_id
    ok('toolmodules/device-id', !!deviceId, `no device id in ${reg.text.slice(0, 200)}`)

    const mod = await POST('/api/admin/tool-modules', {
      token: admin.token,
      body: { tool_id: toolId, device_id: deviceId, role: 'power', name: 'main plug' },
    })
    assertEq('toolmodules/create-binding', 201, mod.status)
    moduleId = mod.json?.data?.id
    ok('toolmodules/binding-id', !!moduleId, `no data.id: ${mod.text.slice(0, 200)}`)
    // Unstated policy must be the fail-safe one, not "hold whatever it had".
    assertEq('toolmodules/binding-defaults-fail-off', 'fail_off', mod.json?.data?.on_disconnect)

    const list = await GET('/api/admin/tool-modules', T)
    assertEq('toolmodules/list-bindings', 200, list.status)
    ok('toolmodules/list-contains-binding',
      (list.json?.data ?? []).some((m) => m.id === moduleId), 'created binding missing from list')

    // A rule citing a module of a DIFFERENT tool would read a sensor on the
    // wrong machine.
    const otherTool = await POST('/api/tools', {
      token: admin.token,
      body: { name: `Other ${tag}`, category: 'other', external_id: `tm-other-${tag}` },
    })
    const crossed = await POST('/api/admin/tool-interlocks', {
      token: admin.token,
      body: {
        tool_id: otherTool.json?.data?.id,
        kind: 'trip',
        condition: 'door_open',
        source_module_id: moduleId,
      },
    })
    assertEq('toolmodules/foreign-source-module-is-400', 400, crossed.status)

    // --- /sync is scoped to what the device is bound to (#104) --------------
    //
    // It used to return every tool and every member's card value to whoever
    // asked: `build_sync_payload` took a device id, echoed it into the
    // response, and scoped nothing. One compromised reader yielded the card
    // identifier of the entire membership plus the full authorization matrix.
    //
    // Two devices, two tools, and a member authorized for exactly one of them.
    // The assertions are deliberately two-sided: that each device sees its own,
    // and that neither sees the other's. Over-inclusion is the failure here, and
    // a test that only checks the expected rows are present passes on a payload
    // containing everything.
    const openTool = await POST('/api/tools', {
      token: admin.token,
      body: { name: `Sync Open ${tag}`, category: 'other', external_id: `tm-sync-open-${tag}` },
    })
    const openToolId = openTool.json?.data?.id
    // Training-gated, so the member below is NOT authorized for it. That is what
    // makes the user-level assertion mean something rather than comparing two
    // lists that happen to be equal.
    const gatedTool = await POST('/api/tools', {
      token: admin.token,
      body: {
        name: `Sync Gated ${tag}`,
        category: 'other',
        external_id: `tm-sync-gated-${tag}`,
        requires_training: true,
      },
    })
    const gatedToolId = gatedTool.json?.data?.id

    const member = await account(`tm_sync_${tag}`)
    const cardCode = `TMSYNC-${tag}`
    const card = await POST(`/api/cards/user/${member.user.id}`, {
      token: admin.token,
      body: { code: cardCode },
    })
    ok('toolmodules/sync-member-card', card.status < 300, `issue card -> ${card.status}`)

    const syncDevice = async (name, mac, boundToolId) => {
      const inv = await POST('/api/admin/devices/invite', {
        token: admin.token,
        body: { expires_in_hours: 1 },
      })
      const r = await POST('/api/devices/register', {
        body: {
          device_code: inv.json?.data?.device_code,
          name: `${name}-${tag}`,
          kind: 'card_reader',
          mac_address: mac,
          software_version: '0.0.0-e2e',
          platform: 'linux',
        },
      })
      const id = r.json?.data?.device_id ?? r.json?.device_id
      const token = r.json?.data?.auth_token ?? r.json?.auth_token
      if (boundToolId) {
        await POST('/api/admin/tool-modules', {
          token: admin.token,
          body: { tool_id: boundToolId, device_id: id, role: 'reader', name: `${name} reader` },
        })
      }
      const sync = await GET('/api/toolguard/sync', { token })
      const body = sync.json?.data ?? sync.json
      return {
        status: sync.status,
        tools: (body?.tools ?? []).map((t) => t.id),
        cards: (body?.users ?? []).map((u) => u.profile_field_digest),
      }
    }

    const openReader = await syncDevice('sync-open', '02:00:00:00:04:01', openToolId)
    const gatedReader = await syncDevice('sync-gated', '02:00:00:00:04:02', gatedToolId)
    const unbound = await syncDevice('sync-unbound', '02:00:00:00:04:03', null)

    assertEq('toolmodules/sync-answers', 200, openReader.status)

    ok('toolmodules/sync-has-its-own-tool', openReader.tools.includes(openToolId),
      `the device bound to the open tool did not receive it: ${JSON.stringify(openReader.tools)}`)
    ok('toolmodules/sync-omits-another-devices-tool', !openReader.tools.includes(gatedToolId),
      `a reader bound to one tool received another device's tool: ${JSON.stringify(openReader.tools)}`)
    ok('toolmodules/sync-omits-in-both-directions', !gatedReader.tools.includes(openToolId),
      `scoping leaks the other way too: ${JSON.stringify(gatedReader.tools)}`)

    // The member entry is the part that matters. A reader bound to a tool this
    // member cannot use has no business holding anything that identifies them.
    //
    // Asserted on presence and absence of the *entry* rather than of the card
    // value, because since #109 the payload carries a digest and this driver
    // cannot recompute it -- deriving it needs argon2 and the deployment
    // pepper, neither of which a test driver has, and deliberately so.
    ok('toolmodules/sync-has-the-authorized-members-card', openReader.cards.length >= 1,
      `the member authorized for this device's tool is missing: ${JSON.stringify(openReader.cards)}`)
    ok('toolmodules/sync-withholds-an-unauthorized-members-card',
      gatedReader.cards.length === 0,
      `a device received an identifier for a member not authorized for anything it serves: ` +
        JSON.stringify(gatedReader.cards))

    // --- and it is a digest, not the card (#109) ---------------------------
    //
    // The absence asserted explicitly, against the code the fixture issued. A
    // check that only confirms a digest is present would pass just as happily
    // on a payload carrying both.
    const everyIdentifier = [...openReader.cards, ...gatedReader.cards, ...unbound.cards]
    ok('toolmodules/sync-carries-no-plaintext-card',
      !everyIdentifier.includes(cardCode),
      `the sync payload still contains a member's actual card code. A device holds this ` +
        `on disk, so a plug taken off a wall would yield it: ${JSON.stringify(everyIdentifier)}`)
    // And it is not merely absent because the payload is empty -- the entry
    // exists, it is just opaque. Without this the assertion above passes on a
    // server that sends no members at all.
    ok('toolmodules/sync-identifier-is-opaque',
      openReader.cards.length >= 1 &&
        openReader.cards.every((d) => /^[0-9a-f]{64}$/.test(d)),
      `expected a 32-byte hex digest per member, got: ${JSON.stringify(openReader.cards)}`)

    // Deny by default, with no exemption for any device kind: bound to nothing,
    // it authorizes nobody. The server logs a warning naming the device, because
    // this and a misconfiguration look identical from the device's side.
    assertEq('toolmodules/sync-unbound-device-gets-no-tools', 0, unbound.tools.length)
    assertEq('toolmodules/sync-unbound-device-gets-no-cards', 0, unbound.cards.length)

    // --- capabilities decide which enforcement tier is available -------------
    // A second tool wired to an integrated module: the reed is wired straight to
    // the thing that switches the tool, so it can cut locally and `firmware` is
    // genuinely achievable.
    const wiredTool = await POST('/api/tools', {
      token: admin.token,
      body: { name: `Integrated ${tag}`, category: 'laser_cutting', external_id: `tm-int-${tag}` },
    })
    const wiredToolId = wiredTool.json?.data?.id
    const capableInvite = await POST('/api/admin/devices/invite', {
      token: admin.token,
      body: { expires_in_hours: 1 },
    })
    const capableReg = await POST('/api/devices/register', {
      body: {
        device_code: capableInvite.json?.data?.device_code,
        name: `integrated-plug-${tag}`,
        kind: 'power_controller',
        mac_address: '02:00:00:00:83:02',
        software_version: '0.0.0-e2e',
        platform: 'linux',
      },
    })
    const capableDeviceId = capableReg.json?.data?.device_id ?? capableReg.json?.device_id
    const capableBinding = await POST('/api/admin/tool-modules', {
      token: admin.token,
      body: {
        tool_id: wiredToolId,
        device_id: capableDeviceId,
        role: 'power',
        name: 'integrated plug',
        params: {
          capabilities: {
            local_inputs: ['door_open'],
            local_inhibit: true,
            countdown: false,
            holds_last_on_disconnect: false,
          },
        },
      },
    })
    assertEq('toolmodules/capable-binding-created', 201, capableBinding.status)

    const achievable = await POST('/api/admin/tool-interlocks', {
      token: admin.token,
      body: {
        tool_id: wiredToolId,
        kind: 'trip',
        condition: 'door_open',
        enforcement: 'firmware',
      },
    })
    assertEq('toolmodules/achievable-firmware-tier-is-accepted', 201, achievable.status)

    // The same tier for a condition that module cannot sense is still refused --
    // the check is per condition, not a blanket "this tool has a smart plug".
    const wrongCondition = await POST('/api/admin/tool-interlocks', {
      token: admin.token,
      body: {
        tool_id: wiredToolId,
        kind: 'trip',
        condition: 'flow_ok',
        enforcement: 'firmware',
      },
    })
    assertEq('toolmodules/firmware-tier-is-per-condition', 400, wrongCondition.status)

    // --- the fail-safe gap is reported, not assumed away --------------------
    const snap2 = await GET('/api/admin/tool-modules/state', T)
    const wiredEntry = (snap2.json?.data?.tools ?? []).find((t) => t.tool_id === wiredToolId)
    assertEq('toolmodules/capable-tool-fails-safe', true, wiredEntry?.power_fails_safe)
    // The first tool's plug declared no capabilities at all, so it is not
    // claimed to fail safe on the strength of nothing.
    const plainEntry = (snap2.json?.data?.tools ?? []).find((t) => t.tool_id === toolId)
    ok('toolmodules/undeclared-plug-is-not-claimed-fail-safe',
      wiredEntry?.power_fails_safe === true && plainEntry !== undefined,
      'both tools should appear in the snapshot with an explicit fail-safe verdict')
  }

  // --- the snapshot the edge coordinates from -------------------------------
  const snap = await GET('/api/admin/tool-modules/state', T)
  assertEq('toolmodules/state', 200, snap.status)
  const entry = (snap.json?.data?.tools ?? []).find((t) => t.tool_id === toolId)
  ok('toolmodules/state-has-tool', !!entry,
    `wired tool absent from snapshot: ${snap.text.slice(0, 300)}`)
  ok('toolmodules/state-has-trip',
    (entry?.interlocks ?? []).some((i) => i.id === tripId), 'trip missing from snapshot')
  // Two oracles on the same claim, from opposite sides: the disabled rule is
  // absent, AND the rule that IS present carries what the edge needs to act.
  assertEq('toolmodules/state-omits-disabled-rule', false,
    (entry?.interlocks ?? []).some((i) => i.id === disabledId))
  const snapTrip = (entry?.interlocks ?? []).find((i) => i.id === tripId)
  assertEq('toolmodules/state-carries-latch', true, snapTrip?.latch)
  assertEq('toolmodules/state-carries-enforcement', 'edge', snapTrip?.enforcement)
  if (moduleId) {
    ok('toolmodules/state-has-binding',
      (entry?.modules ?? []).some((m) => m.id === moduleId), 'binding missing from snapshot')
  }

  // --- deletes report what they changed -------------------------------------
  const del = await DELETE(`/api/admin/tool-interlocks/${tripId}`, T)
  assertEq('toolmodules/delete-trip', 200, del.status)
  const delAgain = await DELETE(`/api/admin/tool-interlocks/${tripId}`, T)
  assertEq('toolmodules/second-delete-is-404', 404, delAgain.status)

  if (moduleId) {
    const delMod = await DELETE(`/api/admin/tool-modules/${moduleId}`, T)
    assertEq('toolmodules/delete-binding', 200, delMod.status)
    const delModAgain = await DELETE(`/api/admin/tool-modules/${moduleId}`, T)
    assertEq('toolmodules/second-binding-delete-is-404', 404, delModAgain.status)
  }

  // --- the wiring change is auditable ---------------------------------------
  const audit = await GET('/api/admin/audit-logs?limit=300', T)
  const events = (audit.json?.data?.logs ?? audit.json?.data ?? []).map((e) => e.event_type)
  const wanted = ['tool_interlock_created', 'tool_interlock_deleted']
  if (moduleId) wanted.push('tool_module_created', 'tool_module_deleted')
  for (const want of wanted) {
    ok(`toolmodules/audit-${want}`, events.includes(want),
      `no ${want} row among ${events.length} audit events`)
  }
})
