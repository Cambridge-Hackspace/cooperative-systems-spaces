// The stage timeline (stack.sh timeline_start): every MQTT message a stage
// saw, stamped as it arrived, read back so a driver can judge a window against
// the stage's own events rather than against the shell's clock.
//
// Pure: text in, rows out. No filesystem, so the self-test can feed it the
// timelines a broken system -- or a slow, healthy one -- would record.

/**
 * One row per line: `<seconds.nanos> TAB <retained> TAB <topic> TAB <payload>`.
 * `line` is the 1-based line number, which is what a mark counts in; `t` is
 * seconds; `json` is the payload parsed, or null.
 */
export function parseTimeline(text) {
  const rows = []
  const lines = (text ?? '').split('\n')
  // A trailing newline leaves one empty string, which is not a line.
  if (lines.length && lines[lines.length - 1] === '') lines.pop()
  lines.forEach((raw, idx) => {
    const parts = raw.split('\t')
    if (parts.length < 4) return
    const t = Number(parts[0])
    if (!Number.isFinite(t)) return
    const payload = parts.slice(3).join('\t')
    let json = null
    try {
      json = JSON.parse(payload)
    } catch {
      json = null
    }
    rows.push({ line: idx + 1, t, retained: parts[1] === '1', topic: parts[2], payload, json })
  })
  return rows
}

/** `<name> TAB <lines so far>` per line, as stack.sh timeline_mark writes. */
export function parseMarks(text) {
  const marks = new Map()
  for (const raw of (text ?? '').split('\n')) {
    const [name, n] = raw.split('\t')
    if (name && n !== undefined && Number.isFinite(Number(n))) marks.set(name, Number(n))
  }
  return marks
}

/**
 * The first row past line `after` on `topic` whose parsed payload satisfies
 * `pred`, or null. How a driver finds the stage's own message from the mark
 * taken just before sending it.
 */
export function firstAfter(rows, after, topic, pred = () => true) {
  return (
    rows.find((r) => r.line > after && r.topic === topic && r.json !== null && pred(r.json)) ?? null
  )
}

/** As firstAfter, for a payload that is not JSON: matched exactly. */
export function firstRawAfter(rows, after, topic, payload) {
  return rows.find((r) => r.line > after && r.topic === topic && r.payload === payload) ?? null
}

/** Parsed payloads on `topic` with `from < t <= to`, matching `pred`, in order. */
export function during(rows, topic, from, to, pred = () => true) {
  return rows
    .filter((r) => r.topic === topic && r.t > from && r.t <= to && r.json !== null && pred(r.json))
    .map((r) => r.json)
}

/** Parsed payloads on `topic` between two line numbers (exclusive), matching `pred`. */
export function between(rows, topic, fromLine, toLine, pred = () => true) {
  return rows
    .filter(
      (r) =>
        r.topic === topic && r.line > fromLine && r.line < toLine && r.json !== null && pred(r.json)
    )
    .map((r) => r.json)
}
