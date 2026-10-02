// Tier: the alert feed (#87), against the live stack.
//
// An alert is an audit event classified Notice or above, seen through the
// `alerts.view` permission and acknowledged through `alerts.acknowledge`. The
// claims below are about a real grant reaching a real user and a real row
// crossing the severity line, which no source check can see:
//
//   * a `failed_login_attempt` (Notice) appears in the feed for a holder of
//     alerts.view, and the feed is 403 for an account without it -- the
//     permission half the offline route matrix cannot assert;
//   * the same event is ABSENT at min_severity=critical, and absent from an
//     unrelated category -- the classification filters, not merely decorates;
//   * acknowledging moves it out of the unacknowledged view, the summary count
//     moves with it, a second acknowledgement is a no-op, and an audit row
//     records who did it;
//   * an Info-level event (a plain login) is never an alert: 404 on acknowledge.

import {
  assertEq,
  main,
  ok,
  GET,
  POST,
  PATCH,
  DELETE,
  RUN_TAG,
  account,
  adminAccount,
  login,
} from './lib.mjs'

await main(async () => {
  // Admin inherits staff, which holds both alerts.* grants by seed.
  const admin = await adminAccount('alerts_admin')
  const member = await account('alerts_member')

  // ---- raise a Notice-level event anyone can trigger ------------------------
  const bad = await login(member.username, 'definitely-not-the-password')
  assertEq('alerts/fixture-failed-login', 401, bad.status)

  // ---- permission, not role -------------------------------------------------
  const refused = await GET('/api/alerts', { token: member.token })
  assertEq('alerts/feed-is-403-without-alerts.view', 403, refused.status, refused.text.slice(0, 200))
  const refusedSummary = await GET('/api/alerts/summary', { token: member.token })
  assertEq('alerts/summary-is-403-without-alerts.view', 403, refusedSummary.status)
  const classification = await GET('/api/alerts/classification', { token: member.token })
  assertEq('alerts/classification-is-for-any-signed-in-user', 200, classification.status)
  const events = classification.json?.data?.events ?? {}
  assertEq('alerts/classification-files-failed-login-as-notice', 'notice', events.failed_login_attempt?.severity)
  assertEq('alerts/classification-files-unauthorized-power-as-critical', 'critical', events.unauthorized_power_detected?.severity)

  // ---- the feed ---------------------------------------------------------------
  const feed = await GET('/api/alerts?acknowledged=false&per_page=200', { token: admin.token })
  assertEq('alerts/feed-ok-with-alerts.view', 200, feed.status, feed.text.slice(0, 200))
  const mine = (feed.json?.data?.items ?? []).find(
    (a) => a.event_type === 'failed_login_attempt' && a.event_data?.username_or_email === member.username
  ) ?? (feed.json?.data?.items ?? []).find((a) => a.event_type === 'failed_login_attempt')
  ok('alerts/failed-login-appears-as-an-alert', !!mine, `types seen: ${JSON.stringify([...new Set((feed.json?.data?.items ?? []).map((a) => a.event_type))])}`)
  assertEq('alerts/alert-carries-its-severity', 'notice', mine?.severity)
  assertEq('alerts/alert-carries-its-category', 'user', mine?.category)
  ok('alerts/feed-reports-a-real-total', (feed.json?.data?.total ?? 0) >= 1, JSON.stringify(feed.json?.data?.total))

  // The classification filters. Precondition above: the row is in the default
  // view. At a higher floor and in another category it must not be.
  const critical = await GET('/api/alerts?min_severity=critical&per_page=200', { token: admin.token })
  ok(
    'alerts/notice-event-absent-at-critical-floor',
    !(critical.json?.data?.items ?? []).some((a) => a.id === mine?.id),
    'a notice-level alert was listed at min_severity=critical'
  )
  const doors = await GET('/api/alerts?category=door&per_page=200', { token: admin.token })
  ok(
    'alerts/user-event-absent-from-door-category',
    !(doors.json?.data?.items ?? []).some((a) => a.id === mine?.id),
    'a user-category alert was listed under category=door'
  )
  const badFilter = await GET('/api/alerts?min_severity=loud', { token: admin.token })
  assertEq('alerts/unknown-severity-is-400', 400, badFilter.status)

  // ---- acknowledgement ----------------------------------------------------------
  const before = await GET('/api/alerts/summary', { token: admin.token })
  assertEq('alerts/summary-ok', 200, before.status)
  const noticeBefore = before.json?.data?.unacknowledged?.notice ?? 0
  ok('alerts/summary-counts-the-unacknowledged-notice', noticeBefore >= 1, JSON.stringify(before.json?.data))

  const ack = await POST(`/api/alerts/${mine?.id}/acknowledge`, { token: admin.token, body: { note: 'looked; expected' } })
  assertEq('alerts/acknowledge-ok', 200, ack.status, ack.text.slice(0, 200))
  ok('alerts/acknowledge-records-who-and-note', ack.json?.data?.acknowledgement?.user_id === admin.user.id && ack.json?.data?.acknowledgement?.note === 'looked; expected', JSON.stringify(ack.json?.data?.acknowledgement))

  const after = await GET('/api/alerts/summary', { token: admin.token })
  assertEq('alerts/summary-drops-by-one', noticeBefore - 1, after.json?.data?.unacknowledged?.notice, JSON.stringify(after.json?.data))
  const unacked = await GET('/api/alerts?acknowledged=false&per_page=200', { token: admin.token })
  ok('alerts/acknowledged-leaves-the-unacknowledged-view', !(unacked.json?.data?.items ?? []).some((a) => a.id === mine?.id), 'still listed as unacknowledged')
  const acked = await GET('/api/alerts?acknowledged=true&per_page=200', { token: admin.token })
  ok('alerts/acknowledged-appears-in-the-acknowledged-view', (acked.json?.data?.items ?? []).some((a) => a.id === mine?.id), 'not listed as acknowledged')

  const again = await POST(`/api/alerts/${mine?.id}/acknowledge`, { token: admin.token, body: { note: 'second' } })
  assertEq('alerts/second-acknowledge-is-a-no-op', 200, again.status)
  assertEq('alerts/second-acknowledge-keeps-the-first-note', 'looked; expected', again.json?.data?.acknowledgement?.note)
  const afterAgain = await GET('/api/alerts/summary', { token: admin.token })
  assertEq('alerts/second-acknowledge-moves-nothing', after.json?.data?.unacknowledged?.notice, afterAgain.json?.data?.unacknowledged?.notice)

  const memberAck = await POST(`/api/alerts/${mine?.id}/acknowledge`, { token: member.token, body: {} })
  assertEq('alerts/acknowledge-is-403-without-alerts.acknowledge', 403, memberAck.status)

  // The acknowledgement is itself audited, attributed to the acknowledger.
  const audit = await GET('/api/admin/audit-logs?event_type=alert_acknowledged&per_page=100', { token: admin.token })
  const row = (audit.json?.data ?? []).find((e) => e.event_data?.alert_id === mine?.id)
  ok('alerts/acknowledgement-is-audited', row?.actor_id === admin.user.id && row?.event_data?.note === 'looked; expected', JSON.stringify(row))

  // ---- notifications by class (#87 slice 2) ----------------------------------------
  // Two webhooks to unreachable loopback ports (a delivery row is written for
  // every attempt, failed or not, so an unreachable sink still proves routing):
  //   A subscribes by class: everything at notice or above, any category;
  //   B subscribes to one unrelated event type only.
  // Raise a Notice event; A gets a delivery row for it and B gets none --
  // absence asserted after A's presence, so "nothing arrived" cannot be
  // "nothing was dispatched".
  const mk = async (name, body) => {
    const r = await POST('/api/admin/webhooks', { token: admin.token, body: { name, url: `http://127.0.0.1:9/${name}`, ...body } })
    assertEq(`alerts/webhook-created/${name}`, 201, r.status, r.text.slice(0, 200))
    return r.json?.data
  }
  const hookA = await mk(`class-${RUN_TAG}`, { class_subscriptions: [{ category: null, min_severity: 'notice' }] })
  const hookB = await mk(`event-${RUN_TAG}`, { event_types: ['door_created'] })
  assertEq('alerts/webhook-echoes-class-subscription', 'notice', hookA?.class_subscriptions?.[0]?.min_severity, JSON.stringify(hookA?.class_subscriptions))
  const badSub = await POST('/api/admin/webhooks', { token: admin.token, body: { name: `bad-${RUN_TAG}`, url: 'http://127.0.0.1:9/bad', class_subscriptions: [{ min_severity: 'loud' }] } })
  assertEq('alerts/unknown-class-severity-is-400', 400, badSub.status)

  const second = await login(member.username, 'still-not-the-password')
  assertEq('alerts/fixture-second-failed-login', 401, second.status)

  const deliveriesFor = async (id) => {
    const r = await GET(`/api/admin/webhooks/deliveries?webhook_id=${id}&limit=50`, { token: admin.token })
    return (r.json?.data ?? []).filter((d) => d.event_type === 'failed_login_attempt')
  }
  let rowsA = []
  for (let i = 0; i < 40 && rowsA.length === 0; i++) {
    await new Promise((r) => setTimeout(r, 250))
    rowsA = await deliveriesFor(hookA.id)
  }
  ok('alerts/class-subscribed-webhook-is-dispatched-a-notice-event', rowsA.length >= 1, 'no delivery row for the class-subscribed webhook within 10s')
  // Time allowed to pass after the precondition, then the absence.
  await new Promise((r) => setTimeout(r, 2000))
  const rowsB = await deliveriesFor(hookB.id)
  assertEq('alerts/event-subscribed-webhook-is-not-dispatched-another-type', 0, rowsB.length, `B received ${rowsB.length} failed_login_attempt deliveries`)

  // A floor above the event: no delivery. Same hook, raised to critical.
  const raise = await PATCH(`/api/admin/webhooks/${hookA.id}`, { token: admin.token, body: { class_subscriptions: [{ category: null, min_severity: 'critical' }] } })
  assertEq('alerts/class-subscription-updated', 200, raise.status, raise.text.slice(0, 200))
  // Let the dispatcher's retries for the earlier event settle (three
  // attempts, 1s + 2s backoff) before taking the baseline, or a late third
  // attempt of the OLD event reads as a delivery of the new one.
  await new Promise((r) => setTimeout(r, 4000))
  const countBefore = (await deliveriesFor(hookA.id)).length
  const third = await login(member.username, 'nor-this-one')
  assertEq('alerts/fixture-third-failed-login', 401, third.status)
  await new Promise((r) => setTimeout(r, 2500))
  assertEq('alerts/notice-event-does-not-reach-a-critical-floor', countBefore, (await deliveriesFor(hookA.id)).length, 'a notice event was delivered to a webhook whose floor is critical')

  // ---- the heartbeat (#87 slice 3) ----------------------------------------------
  // A heartbeat is itself an alert (Notice, category alerts) carrying what
  // happened since the last one. Run one now: it appears in the feed, counts
  // the alerts this stage raised, and a second one straight after counts
  // nothing new -- so the window really is "since the last heartbeat", and a
  // heartbeat never counts the previous heartbeat as an alert.
  const hbRefused = await POST('/api/admin/alerts/heartbeat', { token: member.token })
  assertEq('alerts/heartbeat-now-is-admin-only', 403, hbRefused.status)
  const hb1 = await POST('/api/admin/alerts/heartbeat', { token: admin.token })
  assertEq('alerts/heartbeat-now-ok', 200, hb1.status, hb1.text.slice(0, 200))
  ok('alerts/heartbeat-reports-ok', hb1.json?.data?.ok === true, JSON.stringify(hb1.json?.data))
  ok('alerts/heartbeat-counted-the-alerts-raised', (hb1.json?.data?.alerts_raised ?? 0) >= 3, `alerts_raised=${hb1.json?.data?.alerts_raised} (three failed logins at least)`)
  ok('alerts/heartbeat-counted-failed-deliveries', (hb1.json?.data?.deliveries_failed ?? 0) >= 1, `deliveries_failed=${hb1.json?.data?.deliveries_failed} (the unreachable webhook)`)
  const hbFeed = await GET('/api/alerts?category=alerts&per_page=50', { token: admin.token })
  const beat = (hbFeed.json?.data?.items ?? []).find((a) => a.event_type === 'alert_heartbeat')
  ok('alerts/heartbeat-is-itself-an-alert', !!beat && beat.severity === 'notice', JSON.stringify(beat))
  assertEq('alerts/heartbeat-event-carries-the-counts', hb1.json?.data?.alerts_raised, beat?.event_data?.alerts_raised)
  const hb2 = await POST('/api/admin/alerts/heartbeat', { token: admin.token })
  assertEq('alerts/second-heartbeat-counts-nothing-new', 0, hb2.json?.data?.alerts_raised, `second heartbeat alerts_raised=${hb2.json?.data?.alerts_raised}; the first heartbeat must not count as an alert`)

  // The webhooks go last: the heartbeat above counted their failed deliveries
  // (deleting a webhook cascades its delivery rows), and the dispatcher's
  // retries (1s + 2s backoff) must have settled so none is in flight.
  await new Promise((r) => setTimeout(r, 4000))
  for (const h of [hookA, hookB]) {
    const gone = await DELETE(`/api/admin/webhooks/${h.id}`, { token: admin.token })
    assertEq(`alerts/webhook-deleted/${h.name}`, 200, gone.status, gone.text.slice(0, 120))
  }

  // ---- an Info-level row is not an alert ------------------------------------------
  const logins = await GET('/api/admin/audit-logs?event_type=user_login&per_page=5', { token: admin.token })
  const loginRow = (logins.json?.data ?? [])[0]
  ok('alerts/fixture-has-a-login-row', !!loginRow, 'no user_login audit rows at all')
  const notAlert = await POST(`/api/alerts/${loginRow?.id}/acknowledge`, { token: admin.token, body: {} })
  assertEq('alerts/info-level-row-is-404-to-acknowledge', 404, notAlert.status, notAlert.text.slice(0, 200))
})
