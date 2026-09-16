// The navigation tree as the server sends it (`css_lib::nav::NavNode`).
//
// A node with an empty `slug` is a folder that has no page of its own -- see
// the Rust module for why those exist. Nothing in this file may assume a slug
// is present; that assumption is what #81 was.

export interface NavItem {
  title: string
  slug: string
  path: string
  children: NavItem[]
}

/**
 * A stable identity for a node, used for `v-for` keys and for tracking which
 * branches are open.
 *
 * Group nodes have no slug, so slug alone is not an identity: two sibling
 * groups would collide on `''` and Vue would reuse one's DOM for the other.
 * The repository path is unique per node and survives a title change.
 */
export function keyOf(item: NavItem): string {
  return item.slug || item.path || item.title
}

/** Every key on the path from the root down to `slug`, that node included. */
export function ancestorKeys(items: NavItem[], slug: string | undefined): string[] {
  if (!slug) return []
  const found: string[] = []
  const walk = (nodes: NavItem[], trail: string[]): boolean => {
    for (const node of nodes) {
      const here = [...trail, keyOf(node)]
      if (node.slug === slug || walk(node.children ?? [], here)) {
        if (found.length === 0) found.push(...here)
        return true
      }
    }
    return false
  }
  walk(items, [])
  return found
}
