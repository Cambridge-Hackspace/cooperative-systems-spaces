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
// What this does NOT cover: the refresh endpoints, which need an admin and a
// repository that changes after bring-up. The fixture is static, so a refresh
// here would assert only that nothing broke -- which `devseed` and the unit
// tests already establish more cheaply.

import { GET, ok, assertEq, record, main } from './lib.mjs'

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

main(async () => {
  // --- the pages themselves ------------------------------------------------
  const listed = await GET('/api/pages/wiki')
  const body = listed.json?.data ?? listed.json
  const pages = Array.isArray(body) ? body : (body?.pages ?? [])
  const slugs = pages.map((p) => p.slug).sort()

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
  const navRes = await GET('/api/pages/navigation')
  const nav = (navRes.json?.data ?? navRes.json)?.wiki_nav ?? []
  const navigable = reachable(nav)

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
    'the refresh endpoints: the fixture is static after bring-up, so a refresh ' +
      'here would assert only that nothing broke'
  )
})
