// Pages: the wiki pipeline, end to end (#81).
//
// This tier exists because #81 had none. The navigation builder silently
// discarded every folder that had no same-named page at the repository root --
// sixty of the live wiki's sixty-seven pages were fetched, rendered and served
// while being unreachable from the sidebar, and the refresh button reported
// "67 pages loaded" the whole time. Nothing in the battery could have noticed,
// because the stack config named no repository at all.
//
// The fixture (`make_wiki_fixture` in e2e/stack.sh) is built to that shape on
// purpose: TOOLS/ has no TOOLS.md, which is the exact condition under which the
// old builder dropped its contents. A fixture with one would pass on the code
// that shipped the bug.
//
//   INDEX.md                    -> index
//   TOOLS/LATHE.md              -> tools/lathe
//   TOOLS/LASERS/INDEX.md       -> tools/lasers/index
//   TOOLS/LASERS/MUSE.md        -> tools/lasers/muse
//
// RUN IN TWO PHASES, `initial` and `replaced`, because one of them needs the
// repository to change under the service and the other needs it not to.
//
// The second phase is #157. `make_wiki_fixture 2` wipes the fixture and runs
// `git init` again, so the service's checkout faces a history UNRELATED to the
// one it holds -- a force-push, a squash, a re-created repository. The service
// used to recover from a failed pull with `reset --hard HEAD`, which resets to
// the LOCAL commit and therefore left the branches exactly as divergent as it
// found them; the retry ran the identical pull and failed identically, after
// which the wiki served zero pages and logged an error on every refresh. The
// comment this replaced said a refresh "would assert only that nothing broke",
// and it was right about a static fixture -- which is why the fixture no longer
// is one.
//
// Generation 2 keeps the SAME PAGE COUNT on purpose. A count is what a tier
// reaches for first, and a count would have passed over this defect in both
// directions, so every assertion below names slugs.
//
// What this still does NOT cover: the site repository's refresh, which runs the
// same code against a different configuration key, and the periodic auto-update,
// which is the same call behind a timer this suite does not wait out.

import { GET, POST, ok, assertEq, record, main, adminAccount } from './lib.mjs'

/** Every slug the navigation exposes, at any depth. */
function reachable(nav) {
  const out = []
  const walk = (nodes) => {
    for (const n of nodes ?? []) {
      if (n.slug) out.push(n.slug)
      walk(n.children)
    }
  }
  walk(nav)
  return out.sort()
}

/** Find a node by title among `nodes`. */
function byTitle(nodes, title) {
  return (nodes ?? []).find((n) => n.title === title)
}

/** Every slug the wiki serves, sorted. */
async function servedSlugs() {
  const listed = await GET('/api/pages/wiki')
  const body = listed.json?.data ?? listed.json
  const pages = Array.isArray(body) ? body : (body?.pages ?? [])
  return pages.map((p) => p.slug).sort()
}

/** Every slug the navigation reaches, sorted. */
async function navigableSlugs() {
  const navRes = await GET('/api/pages/navigation')
  const nav = (navRes.json?.data ?? navRes.json)?.wiki_nav ?? []
  return { nav, slugs: reachable(nav) }
}

async function initialFixture() {
  // --- the pages themselves ------------------------------------------------
  const slugs = await servedSlugs()

  assertEq('pages/fixture-loaded', 4, slugs.length)
  ok(
    'pages/nested-page-is-served',
    slugs.includes('tools/lasers/muse'),
    `a page two folders deep is missing from the page list: ${JSON.stringify(slugs)}`
  )

  // --- and the navigation reaches all of them ------------------------------
  //
  // The invariant #81 broke, asserted end to end: a page that is served and not
  // navigable is a page nobody can find. Compared as sets, so over-inclusion is
  // caught as well as omission.
  const { nav, slugs: navigable } = await navigableSlugs()

  // Anti-vacuity first. Comparing two sets is satisfied by two *empty* sets,
  // and that is not hypothetical: the first run of this tier passed this
  // assertion while the stack served zero pages, because the server image had
  // no git and the clone had failed. An invariant that holds trivially when
  // nothing loaded is an invariant that would not have caught #81 either.
  ok(
    'pages/navigation-is-not-empty',
    navigable.length > 0,
    'the navigation exposes no pages at all, so the comparison below would ' +
      'pass on an empty wiki'
  )
  assertEq(
    'pages/every-page-is-reachable-from-the-navigation',
    JSON.stringify(slugs),
    JSON.stringify(navigable)
  )

  // --- the specific shape that was dropped ---------------------------------
  const tools = byTitle(nav, 'Tools')
  ok(
    'pages/folder-without-a-root-page-appears',
    !!tools,
    `no Tools entry in the navigation: ${JSON.stringify(nav.map((n) => n.title))}`
  )
  assertEq('pages/folder-without-a-page-is-not-a-link', '', tools?.slug ?? 'missing')

  const lasers = byTitle(tools?.children, 'Lasers')
  ok(
    'pages/nesting-goes-deeper-than-one-level',
    !!lasers,
    'Lasers should be a folder inside Tools, not flattened beside it'
  )

  // A folder holding an INDEX.md adopts it as its landing page rather than
  // listing it as a child called "Lasers" inside a folder called "Lasers".
  assertEq('pages/folder-index-becomes-the-folder-link', 'tools/lasers/index', lasers?.slug)
  assertEq(
    'pages/folder-index-is-not-also-a-child',
    JSON.stringify(['Muse']),
    JSON.stringify((lasers?.children ?? []).map((c) => c.title))
  )

  // --- and the page a reader would click actually renders ------------------
  const deep = await GET('/api/pages/wiki/tools/lasers/muse')
  assertEq('pages/nested-page-renders', 200, deep.status)
  ok(
    'pages/nested-page-has-its-content',
    (deep.text ?? '').includes('Muse'),
    'the rendered page does not contain its own heading'
  )

  record(
    'pages/what-this-does-not-cover',
    'skip',
    'the site repository (same code, a different config key) and the periodic ' +
      'auto-update (the same call behind a timer this suite does not wait out)'
  )
}

/**
 * #157: the upstream history has been replaced since bring-up, and a refresh
 * must land on the new one.
 *
 * Run after `make_wiki_fixture 2`, which wipes the fixture and inits a fresh
 * repository, so the service's checkout holds commits with no ancestor in
 * common with what it is now asked to fetch.
 */
async function replacedFixture() {
  const admin = await adminAccount('pages')

  // The refresh's own status is the first oracle, and on the defect it was a
  // 500 carrying `Git pull failed even after reset`. An administrator pressing
  // the button is exactly how this was found in production.
  const refreshed = await POST('/api/admin/pages/wiki/refresh', { token: admin.token, body: {} })
  assertEq('pages/refresh-after-a-replaced-history-succeeds', 200, refreshed.status)

  const reported = (refreshed.json?.data ?? refreshed.json)?.wiki_pages_count
  assertEq('pages/refresh-reports-the-new-page-count', 4, reported)

  const slugs = await servedSlugs()

  // Anti-vacuity, and this is the shape the defect actually took: a service
  // that cannot sync serves ZERO pages, and "the old page is gone" is satisfied
  // by an empty wiki.
  ok(
    'pages/pages-are-still-served-after-a-replacement',
    slugs.length > 0,
    'the wiki serves nothing at all, so the assertions below pass on an empty wiki'
  )

  // Both sides, because neither alone is enough: the page only the NEW history
  // has must be served, and the page only the OLD history had must not be.
  ok(
    'pages/the-replacement-history-is-served',
    slugs.includes('tools/mill'),
    `tools/mill is only in generation 2, and it is missing: ${JSON.stringify(slugs)}`
  )
  ok(
    'pages/the-replaced-history-is-gone',
    !slugs.includes('tools/lathe'),
    `tools/lathe is only in generation 1, and it is still served: ${JSON.stringify(slugs)}`
  )

  // Deliberately unchanged across the two generations -- recorded so that a
  // reader does not mistake it for the assertion that catches anything.
  assertEq('pages/the-count-is-unchanged-by-design', 4, slugs.length)

  // The rebuild happened too, not just the fetch: the navigation is derived
  // from the page set on publish, so a stale nav beside fresh pages would mean
  // the sync landed and the publish did not.
  const { slugs: navigable } = await navigableSlugs()
  assertEq(
    'pages/the-navigation-follows-the-replacement',
    JSON.stringify(slugs),
    JSON.stringify(navigable)
  )

  // And the removed page stops resolving, which is what a reader following an
  // old link gets.
  const gone = await GET('/api/pages/wiki/tools/lathe')
  assertEq('pages/a-page-the-replacement-dropped-is-not-found', 404, gone.status)
}

main(async () => {
  // `replaced` runs against generation 2 of the fixture; anything else is the
  // first phase, against generation 1 as bring-up built it.
  if (process.argv[2] === 'replaced') {
    await replacedFixture()
    return
  }
  await initialFixture()
})
