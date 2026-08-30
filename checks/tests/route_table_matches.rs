//! The contract tier's hand-maintained route table must equal the router.
//!
//! `server/tests/common/mod.rs` states independently what the API surface ought
//! to be. This derives the same set from `server/src/api/**` and compares them.
//!
//! The direction matters and is the whole design: this check may **report**
//! drift and may never **absorb** it. The expectation — which guard each route
//! carries, and therefore what it must answer — stays hand-written, because a
//! table generated at test time would agree with the router no matter what the
//! router said. Deleting a route by accident would remove it from both sides at
//! once and the suite would stay green.
//!
//! So: add a route without adding a row, and this fails. Remove one without
//! removing its row, and this fails. Change a handler's extractor without
//! changing its guard, and this fails.

use css_checks::repo_root;
use std::collections::BTreeSet;

/// `(METHOD, path, guard)` for every route the router registers.
fn derived() -> BTreeSet<(String, String, String)> {
    let api = repo_root().join("server/src/api");
    let mut sources = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(&api).expect("server/src/api must exist") {
        let path = entry.expect("readable dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            sources.insert(stem, std::fs::read_to_string(&path).unwrap());
        }
    }

    let mut out = BTreeSet::new();
    let mut queue = vec![(
        "/api".to_string(),
        "mod".to_string(),
        "api_routes".to_string(),
    )];
    let mut seen = BTreeSet::new();

    while let Some((prefix, module, builder)) = queue.pop() {
        if !seen.insert((prefix.clone(), module.clone(), builder.clone())) {
            continue;
        }
        let Some(src) = sources.get(&module) else {
            continue;
        };
        let Some(body) = builder_body(src, &builder) else {
            continue;
        };

        for (path, handlers) in routes_in(&body) {
            let full = if path == "/" {
                prefix.clone()
            } else {
                format!("{prefix}{path}")
            };
            for (method, handler) in handlers {
                out.insert((method, concrete(&full), guard_of(src, &handler)));
            }
        }
        for (sub, module, builder) in nests_in(&body) {
            queue.push((format!("{prefix}{sub}"), module, builder));
        }
    }
    out
}

/// The body of `fn <name>(`, to the next column-0 item.
fn builder_body(src: &str, name: &str) -> Option<String> {
    let at = src.find(&format!("fn {name}("))?;
    let mut out = String::new();
    for (i, line) in src[at..].lines().enumerate() {
        if i > 0
            && [
                "fn ",
                "pub fn ",
                "async fn ",
                "pub async fn ",
                "impl ",
                "struct ",
            ]
            .iter()
            .any(|kw| line.starts_with(kw))
        {
            break;
        }
        // Strip line comments so prose cannot satisfy the scan.
        out.push_str(line.split("//").next().unwrap_or(""));
        out.push('\n');
    }
    Some(out)
}

/// `.route("path", get(h).post(h2))` -> ("path", [("GET","h"),("POST","h2")])
fn routes_in(body: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    for (i, _) in body.match_indices(".route(") {
        let rest = &body[i..];
        let Some(path) = first_literal(rest) else {
            continue;
        };
        // Everything up to the next `.route(` or `.nest(` is this route's
        // method list. Both are searched from index 1 so the `.route(` we are
        // standing on does not terminate its own chunk.
        let next_route = rest[1..].find(".route(").map(|x| x + 1);
        let next_nest = rest[1..].find(".nest(").map(|x| x + 1);
        let end = match (next_route, next_nest) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => rest.len(),
        };
        let chunk = &rest[..end.min(rest.len())];

        let mut methods = Vec::new();
        for verb in ["get", "post", "put", "patch", "delete"] {
            let needle = format!("{verb}(");
            let mut from = 0;
            while let Some(p) = chunk[from..].find(&needle) {
                let abs = from + p;
                // `axum::routing::delete(h)` and `delete(h)` both count; a
                // substring like `budget(` must not.
                let before = chunk[..abs].chars().last().unwrap_or(' ');
                if before.is_alphanumeric() || before == '_' {
                    from = abs + needle.len();
                    continue;
                }
                let after = &chunk[abs + needle.len()..];
                let ident: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    methods.push((verb.to_uppercase(), ident));
                }
                from = abs + needle.len();
            }
        }
        if !methods.is_empty() {
            out.push((path, methods));
        }
    }
    out
}

fn nests_in(body: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (i, _) in body.match_indices(".nest(") {
        let rest = &body[i..];
        let Some(prefix) = first_literal(rest) else {
            continue;
        };
        let Some(comma) = rest.find(',') else {
            continue;
        };
        let target: String = rest[comma + 1..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        let parts: Vec<&str> = target.split("::").filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            out.push((
                prefix,
                parts[parts.len() - 2].to_string(),
                parts[parts.len() - 1].to_string(),
            ));
        }
    }
    out
}

fn first_literal(s: &str) -> Option<String> {
    let open = s.find('"')?;
    let after = &s[open + 1..];
    let close = after.find('"')?;
    Some(after[..close].to_string())
}

/// The parameter list of `fn <name>(`, balanced.
fn signature(src: &str, name: &str) -> String {
    let Some(at) = src.find(&format!("fn {name}(")) else {
        return String::new();
    };
    let open = src[at..].find('(').map(|i| at + i).unwrap_or(at);
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return src[open..open + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// Must match `Guard` in server/tests/common/mod.rs.
fn guard_of(src: &str, handler: &str) -> String {
    let sig = signature(src, handler);
    for (extractor, guard) in [
        ("AdminUser", "Admin"),
        ("StaffUser", "Staff"),
        ("MemberUser", "Member"),
        ("AuthUser", "Auth"),
        ("DeviceAuth", "Device"),
    ] {
        // The extractor may be written bare or fully qualified --
        // `auth: DeviceAuth` and `auth: crate::auth::DeviceAuth` are the same
        // guard. Matching only the bare form silently classified
        // /api/devices/ws as Public, which is the opposite of what it is.
        if sig.contains(&format!(": {extractor}")) || sig.contains(&format!("::{extractor}")) {
            return guard.to_string();
        }
    }
    // Handlers that authenticate inside the body from a bare HeaderMap. Listed
    // by name rather than inferred, because taking a HeaderMap is not by itself
    // evidence of authenticating — `home_links::list_links_public` takes one to
    // derive a role best-effort and is genuinely public.
    const INLINE_AUTH: &[&str] = &["tool_on", "tool_off", "tool_log", "sync", "boot_reset"];
    if INLINE_AUTH.contains(&handler) && sig.contains("HeaderMap") {
        return "InlineAuth".to_string();
    }
    "Public".to_string()
}

/// Substitute path parameters exactly as the table does.
fn concrete(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut inner = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                inner.push(c);
            }
            if inner.starts_with('*') {
                out.push_str("some/slug");
            } else {
                out.push_str("00000000-0000-4000-8000-000000000001");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `(METHOD, path, guard)` as written in the hand-maintained table.
fn declared() -> BTreeSet<(String, String, String)> {
    let src = read_table();
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("R(") {
            continue;
        }
        let parts: Vec<String> = line
            .match_indices('"')
            .collect::<Vec<_>>()
            .chunks(2)
            .filter_map(|w| {
                let (a, _) = w.first()?;
                let (b, _) = w.get(1)?;
                Some(src_slice(line, *a + 1, *b))
            })
            .collect();
        let guard = line
            .split("Guard::")
            .nth(1)
            .and_then(|g| g.split(|c: char| !c.is_alphanumeric()).next())
            .unwrap_or("")
            .to_string();
        if parts.len() == 2 && !guard.is_empty() {
            out.insert((parts[0].clone(), parts[1].clone(), guard));
        }
    }
    out
}

fn src_slice(s: &str, a: usize, b: usize) -> String {
    s[a..b].to_string()
}

fn read_table() -> String {
    std::fs::read_to_string(repo_root().join("server/tests/common/mod.rs"))
        .expect("server/tests/common/mod.rs must exist")
}

#[test]
fn both_sides_found_something() {
    // Either scraper silently returning nothing would make the comparison below
    // pass over two empty sets.
    assert!(
        derived().len() > 150,
        "derived only {} routes from server/src/api; the scraper is broken",
        derived().len()
    );
    assert!(
        declared().len() > 150,
        "parsed only {} rows from the table; the parser is broken",
        declared().len()
    );
}

#[test]
fn the_hand_written_table_equals_the_router() {
    let derived = derived();
    let declared = declared();

    let missing: Vec<_> = derived.difference(&declared).collect();
    let extra: Vec<_> = declared.difference(&derived).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "server/tests/common/mod.rs has drifted from server/src/api.\n\n\
         In the router but not the table (add these rows):\n{missing:#?}\n\n\
         In the table but not the router (remove these rows):\n{extra:#?}\n\n\
         The table is the contract tier's independent statement of the API \
         surface; it is maintained by hand on purpose, and this check exists to \
         report drift rather than to paper over it."
    );
}
