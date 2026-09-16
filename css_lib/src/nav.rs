//! Navigation tree construction for the wiki and site page sets (#81).
//!
//! This lives here, away from the server, because it is a pure function of a
//! list of pages and because the server crate cannot be compiled on every
//! machine that works on this repository -- logic that cannot be run is logic
//! that does not get tested, and the defect this module exists to fix shipped
//! precisely because the old version had no tests at all.
//!
//! # The defect
//!
//! The previous implementation collected nested pages into a map keyed by their
//! first slug segment, then attached them by walking the *root-level pages* and
//! looking each one's slug up in that map. A folder with no same-named page at
//! the root was therefore never looked up, and every page inside it vanished
//! from the navigation while remaining perfectly reachable by URL. On the live
//! wiki that was 60 of 67 pages: four folders, none with a matching root page.
//!
//! The admin's refresh button was working the whole time. It reported "67 pages
//! loaded" and the sidebar showed seven -- which is why the bug was reported as
//! a broken refresh.
//!
//! # The invariant
//!
//! Every page given to [`build_navigation`] appears exactly once in the tree it
//! returns. That is the property the old code broke, it is what
//! `every_page_is_reachable` asserts, and it is the reason folders are
//! synthesised rather than skipped.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The parts of a page the navigation needs. Deliberately not the server's
/// `Page`: rendered HTML and modification times have no bearing on the tree,
/// and depending on them would drag the whole page pipeline in here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavSource {
    pub title: String,
    /// Slash-separated, lowercase, no extension -- e.g. `tools/lasers/muse`.
    pub slug: String,
    /// Path relative to the repository root, original case -- e.g.
    /// `TOOLS/LASERS/MUSE.md`. Used to give synthesised folders the name they
    /// actually have on disk.
    pub path: String,
}

/// One entry in the navigation tree.
///
/// `slug` is empty for a folder that has no page of its own. Such a node is a
/// grouping header, not a link, and a renderer that turns it into an anchor
/// produces a 404 -- so the emptiness is load-bearing, not incidental.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavNode {
    pub title: String,
    pub slug: String,
    pub path: String,
    pub children: Vec<NavNode>,
}

impl NavNode {
    /// True when this node groups pages but is not itself a page.
    pub fn is_group(&self) -> bool {
        self.slug.is_empty()
    }
}

/// A folder under construction. Kept separate from [`NavNode`] because a folder
/// may adopt a page later than it is created -- `tools/lasers` can be built as a
/// bare folder while walking `tools/lasers/muse`, and only adopt `TOOLS/LASERS.md`
/// when that page's turn comes.
#[derive(Default)]
struct Folder {
    /// The page occupying this exact path, if any.
    page: Option<NavSource>,
    children: BTreeMap<String, Folder>,
}

impl Folder {
    fn descend(&mut self, segment: &str) -> &mut Folder {
        self.children.entry(segment.to_string()).or_default()
    }
}

/// Build the navigation tree.
///
/// Folders are synthesised wherever pages are nested, whether or not a page
/// exists at the folder's own path, and nesting is followed to any depth. A
/// folder that *does* have a page -- either at its exact slug (`tools`) or as an
/// index inside it (`tools/index`) -- adopts that page's title and link rather
/// than listing it twice.
pub fn build_navigation(pages: &[NavSource]) -> Vec<NavNode> {
    let mut root = Folder::default();

    for page in pages {
        let segments: Vec<&str> = page.slug.split('/').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            continue;
        }
        let mut cursor = &mut root;
        for segment in &segments {
            cursor = cursor.descend(segment);
        }
        cursor.page = Some(page.clone());
    }

    let mut items: Vec<NavNode> = root
        .children
        .iter()
        .map(|(segment, child)| node_for(segment, child, &[segment]))
        .collect();
    items.sort_by(|a, b| a.title.cmp(&b.title));
    items
}

/// One node, recursing into its own children.
fn node_for(segment: &str, folder: &Folder, prefix: &[&str]) -> NavNode {
    let index = folder.children.get("index").and_then(|c| c.page.as_ref());
    let landing = folder.page.as_ref().or(index);

    let mut children = Vec::new();
    for (child_segment, child) in &folder.children {
        if child_segment == "index" && folder.page.is_none() && index.is_some() {
            continue;
        }
        let mut sub_prefix: Vec<&str> = prefix.to_vec();
        sub_prefix.push(child_segment);
        children.push(node_for(child_segment, child, &sub_prefix));
    }
    children.sort_by(|a, b| a.title.cmp(&b.title));

    NavNode {
        title: landing
            .map(|p| p.title.clone())
            .unwrap_or_else(|| humanize(segment)),
        slug: landing.map(|p| p.slug.clone()).unwrap_or_default(),
        path: landing
            .map(|p| p.path.clone())
            .unwrap_or_else(|| folder_path(folder, prefix.len())),
        children,
    }
}

/// The on-disk path of a page-less folder, recovered from any page beneath it.
///
/// Slugs are lowercased, so the folder's real name (`TOOLS`) only survives in
/// the relative paths of its contents. Taking the first `depth` segments of one
/// of those gives the folder back its actual name rather than a guess.
fn folder_path(folder: &Folder, depth: usize) -> String {
    fn first_page(folder: &Folder) -> Option<&NavSource> {
        if let Some(p) = folder.page.as_ref() {
            return Some(p);
        }
        folder.children.values().find_map(first_page)
    }

    first_page(folder)
        .map(|p| p.path.split('/').take(depth).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// A readable title for a folder with no page to borrow one from.
fn humanize(segment: &str) -> String {
    segment
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(slug: &str, title: &str, path: &str) -> NavSource {
        NavSource {
            title: title.to_string(),
            slug: slug.to_string(),
            path: path.to_string(),
        }
    }

    /// Collect every slug the tree exposes, at any depth.
    fn reachable(nodes: &[NavNode]) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(nodes: &[NavNode], out: &mut Vec<String>) {
            for n in nodes {
                if !n.slug.is_empty() {
                    out.push(n.slug.clone());
                }
                walk(&n.children, out);
            }
        }
        walk(nodes, &mut out);
        out.sort();
        out
    }

    /// The whole point. A page that is loaded and served but absent from the
    /// navigation is a page nobody can find, which is what #81 was.
    #[test]
    fn every_page_is_reachable() {
        let pages = vec![
            page("index", "Index", "INDEX.md"),
            page("events", "Events", "EVENTS.md"),
            page("tools/lasers/muse", "Muse", "TOOLS/LASERS/MUSE.md"),
            page("tools/3d-printing", "3D Printing", "TOOLS/3D_PRINTING.md"),
            page(
                "policies/code-of-conduct",
                "Code of Conduct",
                "POLICIES/COC.md",
            ),
            page("materials/wood", "Wood", "MATERIALS/WOOD.md"),
        ];
        let mut expected: Vec<String> = pages.iter().map(|p| p.slug.clone()).collect();
        expected.sort();

        assert_eq!(expected, reachable(&build_navigation(&pages)));
    }

    /// The exact shape of the live wiki: folders, none of which has a
    /// same-named page at the root. The old implementation returned the root
    /// pages and dropped all the rest.
    #[test]
    fn a_folder_with_no_root_page_still_appears() {
        let pages = vec![
            page("index", "Index", "INDEX.md"),
            page("tools/lathe", "Lathe", "TOOLS/LATHE.md"),
            page("tools/mill", "Mill", "TOOLS/MILL.md"),
        ];
        let nav = build_navigation(&pages);

        let tools = nav
            .iter()
            .find(|n| n.title == "Tools")
            .expect("the tools folder should be in the navigation");
        assert!(tools.is_group(), "a folder with no page is not a link");
        assert_eq!("TOOLS", tools.path, "the folder keeps its on-disk name");
        assert_eq!(
            vec!["Lathe", "Mill"],
            tools.children.iter().map(|c| &c.title).collect::<Vec<_>>()
        );
    }

    /// Nesting deeper than one level was flattened into the top folder before,
    /// so `tools/lasers/muse` sat beside `tools/lathe` as though the laser
    /// cutters were not a group of their own.
    #[test]
    fn nesting_is_followed_to_any_depth() {
        let pages = vec![
            page("tools/lathe", "Lathe", "TOOLS/LATHE.md"),
            page("tools/lasers/muse", "Muse", "TOOLS/LASERS/MUSE.md"),
            page("tools/lasers/k40", "K40", "TOOLS/LASERS/K40.md"),
        ];
        let nav = build_navigation(&pages);
        let tools = &nav[0];

        let lasers = tools
            .children
            .iter()
            .find(|c| c.title == "Lasers")
            .expect("lasers should be a folder inside tools, not flattened beside it");
        assert_eq!(2, lasers.children.len());
        assert!(tools.children.iter().all(|c| c.title != "Muse"));
    }

    /// A folder that does have a page at its own path links to it.
    #[test]
    fn a_folder_with_a_matching_page_links_to_it() {
        let pages = vec![
            page("tools", "Tools", "TOOLS.md"),
            page("tools/lathe", "Lathe", "TOOLS/LATHE.md"),
        ];
        let nav = build_navigation(&pages);

        assert_eq!(1, nav.len(), "the page and the folder are one entry");
        assert_eq!("tools", nav[0].slug);
        assert_eq!("Tools", nav[0].title);
        assert_eq!(1, nav[0].children.len());
    }

    /// An index inside a folder is the folder's landing page, listed once.
    #[test]
    fn a_folder_index_becomes_the_folder_link_and_is_not_also_a_child() {
        let pages = vec![
            page("materials/index", "Materials", "MATERIALS/INDEX.md"),
            page("materials/wood", "Wood", "MATERIALS/WOOD.md"),
        ];
        let nav = build_navigation(&pages);

        assert_eq!("materials/index", nav[0].slug);
        assert_eq!("Materials", nav[0].title);
        assert_eq!(
            vec!["Wood"],
            nav[0].children.iter().map(|c| &c.title).collect::<Vec<_>>(),
            "the index must not appear as its own child"
        );
    }

    /// Both orders of arrival produce the same tree. Pages come out of a
    /// HashMap, so the order is whatever the hasher felt like that morning; a
    /// builder that depended on seeing the folder page first would work in
    /// testing and shuffle in production.
    #[test]
    fn page_order_does_not_change_the_tree() {
        let a = page("tools", "Tools", "TOOLS.md");
        let b = page("tools/lathe", "Lathe", "TOOLS/LATHE.md");
        assert_eq!(
            build_navigation(&[a.clone(), b.clone()]),
            build_navigation(&[b, a])
        );
    }

    /// Sorted by *title*, which is not the same as sorted by slug: titles come
    /// from each page's H1 and routinely disagree with its filename. Every slug
    /// below is deliberately in the opposite order to its title, so an
    /// implementation that leant on the underlying map's key order -- which is
    /// sorted, and would otherwise make the sort look unnecessary -- fails here.
    #[test]
    fn root_and_children_are_sorted_by_title() {
        let pages = vec![
            page("aaa-workshops", "Zebra", "AAA.md"),
            page("zzz-events", "Apple", "ZZZ.md"),
            page("tools/aaa-wrench", "Wrench", "TOOLS/AAA.md"),
            page("tools/zzz-anvil", "Anvil", "TOOLS/ZZZ.md"),
        ];
        let nav = build_navigation(&pages);
        assert_eq!(
            vec!["Apple", "Tools", "Zebra"],
            nav.iter().map(|n| &n.title).collect::<Vec<_>>()
        );
        let tools = nav.iter().find(|n| n.title == "Tools").unwrap();
        assert_eq!(
            vec!["Anvil", "Wrench"],
            tools.children.iter().map(|c| &c.title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_pages_is_an_empty_navigation_not_a_panic() {
        assert!(build_navigation(&[]).is_empty());
    }

    #[test]
    fn humanize_splits_on_both_separators() {
        assert_eq!("Code Of Conduct", humanize("code_of_conduct"));
        assert_eq!("3d Printing", humanize("3d-printing"));
        assert_eq!("Tools", humanize("tools"));
    }

    /// Self-test of the `every_page_is_reachable` oracle: fed the tree the
    /// broken implementation produced -- root pages only, orphan folders
    /// discarded -- it has to complain. An invariant test that passes on the
    /// original defect proves nothing.
    #[test]
    fn the_reachability_oracle_rejects_the_old_broken_tree() {
        let pages = vec![
            page("index", "Index", "INDEX.md"),
            page("tools/lathe", "Lathe", "TOOLS/LATHE.md"),
        ];
        // What the old build_navigation_static returned for this input.
        let broken = vec![NavNode {
            title: "Index".into(),
            slug: "index".into(),
            path: "INDEX.md".into(),
            children: vec![],
        }];
        let expected: Vec<String> = {
            let mut s: Vec<String> = pages.iter().map(|p| p.slug.clone()).collect();
            s.sort();
            s
        };
        assert_ne!(
            expected,
            reachable(&broken),
            "the oracle must fail on the tree that shipped the bug"
        );
    }
}
