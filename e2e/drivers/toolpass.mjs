// toolpass -- the staged-SQLite fixtures the ToolPass loader (#38) reads.
//
// This driver writes no HTTP request and asserts nothing about the database.
// It builds the two extracts the `toolpass` stage loads -- the first scrape and
// the cutover scrape -- and then proves the thing every assertion in that stage
// rests on: that the second extract really does carry rows the first did not.
//
// Without that, "the cutover load imported the new sessions" is 0 == 0 and
// passes against a loader that imports nothing at all, which is exactly the
// defect the stage exists to catch. So the generations are built from one
// definition (the cutover is the base plus a named set of additions, by
// construction) and then *read back out of the two files* and compared. The
// read-back is the point: a manifest derived from the literals below would agree
// with itself whatever the writes did.
//
// The fixture is deliberately small. Volume is not what the loader gets wrong;
// what it gets wrong is which rows it considers already-present, so the
// interesting rows are the ones in the cutover extract that belong to a member
// the first extract already created.
//
// What this driver does not prove: anything about Postgres. The loader runs in
// the stage, against a scratch database, and every claim about what arrived is
// asserted there.

import { rmSync, writeFileSync } from 'node:fs'
import { DatabaseSync } from 'node:sqlite'

import { main, ok, record } from './lib.mjs'

const DIR = process.env.CSS_STACK_DIR ?? '/stack'

// The transform's output schema, as `read_staged` in
// server/src/bin/toolpass_load.rs selects it. Column *order* here is
// irrelevant -- the loader names every column -- but the names and tables are
// not: a typo makes the loader fail with a SQLite error about a missing table,
// which reads as a broken fixture rather than a broken loader, so keep them in
// step with that function.
const SCHEMA = `
CREATE TABLE stg_users (tp_id TEXT, full_name TEXT, email TEXT, username TEXT, role TEXT);
CREATE TABLE stg_tools (tp_id TEXT, name TEXT, description TEXT, external_id TEXT, category TEXT, status TEXT);
CREATE TABLE stg_cards (user_tp_id TEXT, code TEXT, status TEXT);
CREATE TABLE stg_tool_tiers (tool_tp_id TEXT, name TEXT, rate_per_min TEXT);
CREATE TABLE stg_tier_assignments (user_tp_id TEXT, tool_tp_id TEXT, tier_name TEXT);
CREATE TABLE stg_waivers (user_tp_id TEXT, tool_tp_id TEXT, reason TEXT, waived_at TEXT);
CREATE TABLE stg_ledger (user_tp_id TEXT, amount TEXT, currency TEXT, entry_type TEXT, occurred_at TEXT, description TEXT, ext_ref TEXT);
CREATE TABLE stg_sessions (source_ref TEXT, user_tp_id TEXT, tool_tp_id TEXT, started_at TEXT, reported_seconds TEXT);
`

// Which column identifies a row, per table. Used for the superset comparison
// and for the stage's named-row assertions, so the two agree on what "the same
// row" means.
const TABLES = {
  stg_users: 'tp_id',
  stg_tools: 'tp_id',
  stg_cards: 'code',
  stg_tool_tiers: 'name',
  stg_tier_assignments: 'user_tp_id',
  stg_waivers: 'user_tp_id',
  stg_ledger: 'ext_ref',
  stg_sessions: 'source_ref',
}

// --- the first scrape -------------------------------------------------------
// Two members on one tool: one with a card, a tier assignment, a legacy waiver
// and two ledger entries, one with neither. `member` and `newbie` are the legacy
// ToolPass role names on purpose -- role_of maps them onto the RBAC taxonomy,
// and a fixture using only the new names would never exercise that.
const BASE = {
  stg_users: [
    ['tp-1', 'Ada Toolpass', 'ada.toolpass@e2e.invalid', 'ada_toolpass', 'member'],
    ['tp-2', 'Bruno Toolpass', 'bruno.toolpass@e2e.invalid', 'bruno_toolpass', 'newbie'],
  ],
  stg_tools: [['tpt-1', 'Migrated Bandsaw', 'Imported from ToolPass', 'TP-BANDSAW', 'saw', 'idle']],
  stg_cards: [['tp-1', 'TPCARD0001', 'active']],
  // "Default Rate" is load-bearing: the loader reads the tier of that name as
  // the tool's own default rate.
  stg_tool_tiers: [['tpt-1', 'Default Rate', '0.0500']],
  stg_tier_assignments: [['tp-1', 'tpt-1', 'Default Rate']],
  stg_waivers: [['tp-1', 'tpt-1', 'ToolPass legacy grant', '2026-03-04T15:00:00Z']],
  stg_ledger: [
    [
      'tp-1',
      '-12.50',
      'usd',
      'tool_usage',
      '2026-08-01T14:00:00Z',
      'Bandsaw, August',
      'toolpass:txn:tp-1:1',
    ],
    ['tp-1', '20.00', 'usd', 'cash_payment', '2026-08-02T10:00:00Z', 'Top-up', 'toolpass:txn:tp-1:2'],
  ],
  stg_sessions: [
    ['tps-1', 'tp-1', 'tpt-1', '2026-08-01T13:30:00Z', '1500'],
    ['tps-2', 'tp-2', 'tpt-1', '2026-08-03T18:05:00Z', '600'],
  ],
}

// --- what the cutover scrape adds -------------------------------------------
// One new member, and -- the rows that matter -- a ledger entry and a usage
// session belonging to `tp-1`, a member the first load already created. Those
// two are what the loader used to discard: the ledger phase skipped a member who
// already had migrated entries, and the sessions phase skipped itself entirely
// once any migrated session existed.
const CUTOVER_ADDS = {
  stg_users: [['tp-3', 'Cleo Toolpass', 'cleo.toolpass@e2e.invalid', 'cleo_toolpass', 'active']],
  stg_cards: [['tp-3', 'TPCARD0003', 'active']],
  stg_ledger: [
    [
      'tp-1',
      '-3.25',
      'usd',
      'tool_usage',
      '2026-09-20T16:40:00Z',
      'Bandsaw, September',
      'toolpass:txn:tp-1:3',
    ],
    ['tp-3', '15.00', 'usd', 'cash_payment', '2026-09-22T09:15:00Z', 'Top-up', 'toolpass:txn:tp-3:1'],
    // Repeated, for the reason given against stg_sessions below.
    ['tp-3', '15.00', 'usd', 'cash_payment', '2026-09-22T09:15:00Z', 'Top-up', 'toolpass:txn:tp-3:1'],
  ],
  stg_sessions: [
    ['tps-3', 'tp-1', 'tpt-1', '2026-09-20T16:33:00Z', '390'],
    ['tps-4', 'tp-3', 'tpt-1', '2026-09-25T11:02:00Z', '720'],
    // `tps-4` twice, deliberately. The transform that produces these extracts
    // runs outside this repository and makes no uniqueness promise, and the
    // loader batches 2000 rows per statement -- so a repeated identifier inside
    // one batch either loads once or aborts a cutover halfway through. Which of
    // those happens is a property of the conflict clause, and this is the only
    // place it is stated. The repeat is of a row that is *new* in this extract,
    // so it exercises the insert path rather than the already-present one.
    ['tps-4', 'tp-3', 'tpt-1', '2026-09-25T11:02:00Z', '720'],
  ],
}

/** The rows of one generation: 1 is the first scrape, 2 is the cutover. */
function rowsFor(generation) {
  const out = {}
  for (const table of Object.keys(TABLES)) {
    const base = BASE[table] ?? []
    const adds = generation === 2 ? (CUTOVER_ADDS[table] ?? []) : []
    out[table] = [...base, ...adds]
  }
  return out
}

function write(path, rows) {
  rmSync(path, { force: true })
  const db = new DatabaseSync(path)
  try {
    db.exec(SCHEMA)
    for (const [table, values] of Object.entries(rows)) {
      if (values.length === 0) continue
      const width = values[0].length
      const stmt = db.prepare(
        `INSERT INTO ${table} VALUES (${Array(width).fill('?').join(',')})`,
      )
      for (const row of values) stmt.run(...row)
    }
  } finally {
    db.close()
  }
}

/**
 * Read a written fixture back: raw row counts and identifier sets.
 *
 * `rows` and `ids.size` differ wherever an extract repeats an identifier, and
 * the difference is load-bearing -- what the database must end up holding is one
 * row per identifier, not one per staged row.
 */
function readBack(path) {
  const db = new DatabaseSync(path, { readOnly: true })
  try {
    const rows = {}
    const ids = {}
    for (const [table, key] of Object.entries(TABLES)) {
      rows[table] = db.prepare(`SELECT count(*) AS n FROM ${table}`).get().n
      ids[table] = new Set(
        db.prepare(`SELECT ${key} AS k FROM ${table}`).all().map((r) => String(r.k)),
      )
    }
    return { rows, ids }
  } finally {
    db.close()
  }
}

main(async () => {
  const paths = { 1: `${DIR}/toolpass-gen1.sqlite`, 2: `${DIR}/toolpass-gen2.sqlite` }

  write(paths[1], rowsFor(1))
  write(paths[2], rowsFor(2))

  const gen1 = readBack(paths[1])
  const gen2 = readBack(paths[2])

  // Non-empty, per table that has rows. A fixture that wrote nothing would make
  // every count assertion in the stage compare zero against zero.
  for (const table of Object.keys(TABLES)) {
    ok(
      `toolpass/fixture-has-${table.replace(/^stg_/, '')}`,
      gen1.rows[table] > 0,
      `${table} is empty in the first extract`,
    )
  }

  // The superset guard, which is what the whole stage leans on: every row of the
  // first extract is still in the cutover extract, and the cutover extract adds
  // rows to the two tables whose idempotency this tier is about.
  let missing = []
  for (const [table, key] of Object.entries(TABLES)) {
    for (const id of gen1.ids[table]) {
      if (!gen2.ids[table].has(id)) missing.push(`${table}.${key}=${id}`)
    }
  }
  ok(
    'toolpass/cutover-extract-keeps-every-earlier-row',
    missing.length === 0,
    `the cutover extract has dropped ${missing.length}: ${missing.slice(0, 5).join(', ')}`,
  )

  // Compared on distinct identifiers, not on row counts: a cutover extract that
  // merely repeated a row it already had would satisfy the row comparison and
  // add nothing for the loader to import.
  for (const table of ['stg_ledger', 'stg_sessions', 'stg_users', 'stg_cards']) {
    ok(
      `toolpass/cutover-extract-adds-${table.replace(/^stg_/, '')}`,
      gen2.ids[table].size > gen1.ids[table].size,
      `${table}: ${gen1.ids[table].size} distinct in the first extract and ` +
        `${gen2.ids[table].size} in the cutover, so "the new rows arrived" would assert nothing`,
    )
  }

  // And the repeated identifiers are really there, so the stage's
  // "a repeated staged row loads once" is a claim about the loader rather than
  // about a fixture that happens to be duplicate-free.
  for (const table of ['stg_ledger', 'stg_sessions']) {
    ok(
      `toolpass/cutover-extract-repeats-a-${table.replace(/^stg_/, '')}-row`,
      gen2.rows[table] > gen2.ids[table].size,
      `${table}: ${gen2.rows[table]} rows and ${gen2.ids[table].size} distinct identifiers, so ` +
        `nothing in the cutover extract is repeated`,
    )
  }

  // The specific rows the stage names. Asserted here as well, because the stage
  // looks for them in Postgres by these identifiers and a typo there would
  // present as the loader having dropped a row it was never given.
  for (const [table, id] of [
    ['stg_ledger', 'toolpass:txn:tp-1:3'],
    ['stg_sessions', 'tps-3'],
  ]) {
    ok(
      `toolpass/cutover-extract-carries-${id}`,
      gen2.ids[table].has(id) && !gen1.ids[table].has(id),
      `${id} must be absent from the first extract and present in the cutover one`,
    )
  }

  // The manifest the stage asserts the database against: read back out of the
  // files rather than restated from the literals above.
  //
  // `<table>` is the distinct-identifier count -- what the database must hold --
  // and `<table>_rows` the staged row count, which the stage uses to show that
  // the two differ before claiming a repeated row loaded once.
  const manifest = []
  for (const [generation, read] of [
    [1, gen1],
    [2, gen2],
  ]) {
    for (const [table, ids] of Object.entries(read.ids)) {
      const name = table.replace(/^stg_/, '')
      manifest.push(`gen${generation}.${name}=${ids.size}`)
      manifest.push(`gen${generation}.${name}_rows=${read.rows[table]}`)
    }
  }
  writeFileSync(`${DIR}/toolpass-manifest.env`, manifest.join('\n') + '\n')
  record('toolpass/manifest-written', 'ok', `${manifest.length} counts`)
})
