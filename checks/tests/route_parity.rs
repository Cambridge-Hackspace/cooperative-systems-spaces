//! Every path a client asks for must be a route the server registers.
//!
//! Clients name server routes as string literals, in three languages, with
//! nothing connecting the two ends. When they drift the failure is a 404 that
//! looks like a deployment problem, or — because this server has a
//! static-file fallback — a 200 carrying `index.html`, which is worse: the
//! CLI's `info` command was parsing that HTML as JSON and printing
//! `Raw response: <!doctype html>`.
//!
//! This is text-level rather than compiler-level on purpose: it needs no
//! database and no `AppState`, so it runs on the FreeBSD workstation where
//! `css-server` cannot be built at all. What it cannot do is resolve a path
//! built by `format!` from pieces, so it also asserts it found a plausible
//! number of routes — a scraper that quietly finds fewer is indistinguishable
//! from a codebase that got smaller.

use css_checks::repo_root;
use std::collections::BTreeSet;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// The server side
// ---------------------------------------------------------------------------

/// One `Router::new()` builder's body: every `.route(path, ...)` it registers
/// and every `.nest(prefix, module::builder())` it delegates to.
///
/// Scans the body as text rather than line by line. Several builders write
/// `.route(` and its path on separate lines (`api/doors.rs:44` and a dozen
/// others), and a line-oriented scan silently found nothing for those — which
/// is how the whole admin router came back empty.
struct Builder {
    routes: Vec<String>,
    nests: Vec<(String, String)>,
}

/// The body of `fn <name>(`, ending at the next top-level item.
fn body_of<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let start = src.find(&format!("fn {name}("))?;
    let body = &src[start..];
    let end = body
        .match_indices('\n')
        .find(|(i, _)| {
            let line = body[i + 1..].split('\n').next().unwrap_or("");
            [
                "fn ",
                "pub fn ",
                "async fn ",
                "pub async fn ",
                "impl ",
                "struct ",
            ]
            .iter()
            .any(|kw| line.starts_with(kw))
        })
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    Some(&body[..end])
}

/// The first string literal at or after `from`.
fn next_literal(text: &str, from: usize) -> Option<String> {
    let rest = text.get(from..)?;
    let open = rest.find('"')?;
    let after = rest.get(open + 1..)?;
    let close = after.find('"')?;
    Some(after[..close].to_string())
}

fn parse_builder(src: &str, name: &str) -> Builder {
    let mut out = Builder {
        routes: Vec::new(),
        nests: Vec::new(),
    };
    let Some(body) = body_of(src, name) else {
        return out;
    };

    // Strip line comments so documentation cannot satisfy the scan.
    let body: String = body
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    for (i, _) in body.match_indices(".route(") {
        if let Some(path) = next_literal(&body, i) {
            out.routes.push(path);
        }
    }

    for (i, _) in body.match_indices(".nest(") {
        let Some(prefix) = next_literal(&body, i) else {
            continue;
        };
        // `.nest("/mfa", crate::api::mfa::mfa_routes())` -> ("mfa", "mfa_routes")
        let after = &body[i..];
        let Some(comma) = after.find(',') else {
            continue;
        };
        // trim_start first: the text is `, auth::auth_routes())`, and
        // take_while on the leading space yields the empty string -- which
        // produced an empty route set for the entire server.
        let target: String = after[comma + 1..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
            .collect();
        let parts: Vec<&str> = target
            .trim()
            .split("::")
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 2 {
            let builder = parts[parts.len() - 1].to_string();
            let module = parts[parts.len() - 2].to_string();
            out.nests.push((prefix, format!("{module}::{builder}")));
        }
    }
    out
}

/// Every route the server serves, with `{param}` segments normalized to `{p}`.
///
/// Walks the nesting recursively: `api_routes()` nests module routers,
/// `admin_routes()` nests a second layer, and `auth_routes()` nests the MFA
/// router inside itself. An earlier version only looked one level down from
/// `mod.rs` and `admin.rs`, and therefore reported every `/api/auth/mfa/*`
/// route as nonexistent.
fn server_routes() -> BTreeSet<String> {
    let api_dir = repo_root().join("server/src/api");
    let mut sources = std::collections::BTreeMap::new();
    for entry in WalkDir::new(&api_dir).into_iter().filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|e| e == "rs") {
            let stem = entry
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            sources.insert(stem, std::fs::read_to_string(entry.path()).unwrap());
        }
    }

    let mut out = BTreeSet::new();
    // (prefix, module, builder)
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
        let parsed = parse_builder(src, &builder);

        for path in parsed.routes {
            let full = if path == "/" {
                prefix.clone()
            } else {
                format!("{prefix}{path}")
            };
            out.insert(normalize(&full));
        }
        for (sub_prefix, target) in parsed.nests {
            if let Some((m, b)) = target.split_once("::") {
                queue.push((
                    format!("{prefix}{sub_prefix}"),
                    m.to_string(),
                    b.to_string(),
                ));
            }
        }
    }

    // Merged at the router root rather than nested under /api
    // (server/src/main.rs:307-308).
    out.insert("/status".to_string());
    out
}

/// `{user_id}` / `${userId}` / `:id` all become `{p}`.
fn normalize(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '$' if chars.peek() == Some(&'{') => {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
                out.push_str("{p}");
            }
            '{' => {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
                out.push_str("{p}");
            }
            _ => out.push(c),
        }
    }
    out.trim_end_matches('/').to_string()
}

// ---------------------------------------------------------------------------
// The client side
// ---------------------------------------------------------------------------

/// How a client's literals become server paths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// Rust clients build absolute paths, so the literal is the path.
    Absolute,
    /// The frontend's axios instance carries `baseURL: '/api'`
    /// (frontend/src/utils/api.ts:42), so its literals omit that prefix
    /// entirely and `/profiles/x` on the wire is `/api/profiles/x`.
    AxiosBaseUrl,
}

fn client_paths(dir: &str, exts: &[&str], prefix: Prefix) -> BTreeSet<String> {
    let mut out = BTreeSet::new();

    for entry in WalkDir::new(repo_root().join(dir))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| exts.contains(&x.to_string_lossy().as_ref()))
        })
    {
        let src = std::fs::read_to_string(entry.path()).unwrap_or_default();

        for line in src.lines() {
            let code = line.split("//").next().unwrap_or("");

            // Only lines that actually issue a request. Without this, a
            // `<router-link to="/wiki">` or a `$route.path.startsWith('/page')`
            // is indistinguishable from an API call: both are `/`-leading
            // literals in the same files.
            //
            // Known limitation, stated rather than hidden: api.ts builds two
            // paths into a `const` and passes the variable, so those two are not
            // seen here. The count assertion below is what catches this scan
            // silently finding less than it used to.
            if prefix == Prefix::AxiosBaseUrl
                && !["apiClient.", "Api.", "axios.", "fetch("]
                    .iter()
                    .any(|m| code.contains(m))
            {
                continue;
            }

            for lit in literals(code) {
                // The axios instance's own baseURL, not a path.
                if lit == "/api" {
                    continue;
                }
                let candidate = match prefix {
                    Prefix::Absolute if lit.starts_with("/api/") => lit.clone(),
                    // Already absolute. Three components bypass `apiClient`
                    // and call axios/fetch directly, so they carry the prefix
                    // themselves -- see ToolEditModal.vue, HomeView.vue and
                    // RegisterView.vue. Prefixing again would produce
                    // `/api/api/...` and report a route that does not exist.
                    Prefix::AxiosBaseUrl if lit.starts_with("/api/") => lit.clone(),
                    Prefix::AxiosBaseUrl if lit.starts_with('/') && !lit.starts_with("//") => {
                        format!("/api{lit}")
                    }
                    _ => continue,
                };
                // Only the path is routed; drop any query string.
                let path = candidate
                    .split('?')
                    .next()
                    .unwrap_or(&candidate)
                    .to_string();
                out.insert(normalize(&path));
            }
        }
    }
    out
}

/// Double-quoted, single-quoted and backtick literals on one line.
fn literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' && c != '\'' && c != '`' {
            continue;
        }
        let quote = c;
        let mut lit = String::new();
        for c in chars.by_ref() {
            if c == quote {
                break;
            }
            lit.push(c);
        }
        out.push(lit);
    }
    out
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Client paths with no server route, each with the reason it is tolerated.
///
/// Every entry is a request that will 404 — or, because this server has a
/// static-file fallback, may return `index.html` with a 200. Adding one is a
/// decision to ship a client call that cannot work.
const UNRESOLVED: &[(&str, &str)] = &[
    (
        "/api/toolpass/v1/add-user",
        "toolpass-client ships an add-user subcommand; no /api/toolpass router \
         exists anywhere in this workspace and no add-user endpoint exists under \
         /api/toolguard either. The feature was never built server-side.",
    ),
    ("/api/toolpass/v1/remove-user", "As above, for remove-user."),
    (
        "/api",
        "cli health probes GET /api/ purely for reachability and treats 404 as \
         success -- see the comment at cli/src/commands/health.rs:55. It is not \
         calling a route; it is asking whether anything answers.",
    ),
    // ---- Real defects, recorded rather than guessed at. --------------------
    //
    // Each of these is a UI action that cannot work: the request 404s, and
    // because api.ts wraps every call in `.catch`, the user sees a generic
    // "Failed to ..." rather than anything pointing at a missing route.
    //
    // They are listed instead of repointed because in each case the server has
    // a route that is *plausibly* the intended target but takes a different
    // shape, and matching a payload by guesswork would replace a visible
    // failure with a silent one.
    (
        "/api/training/prerequisites",
        "trainingApi.addTrainingPrerequisite POSTs here (utils/api.ts:670). The \
         server has POST /api/training/steps/{step_id}/prerequisites \
         (api/training.rs:130) -- the step id belongs in the path, not the body. \
         Note the sibling DELETE /api/training/prerequisites/{id} at :679 does \
         resolve, so only the create path is wrong.",
    ),
    (
        "/api/trainers/users",
        "userApi.getUsersForTraining GETs here (utils/api.ts:195) and already \
         has an explicit 404 fallback, so this one was known: somebody hit it, \
         worked around it, and left the call in place. No such route exists; \
         the roster comes from /api/admin/roster.",
    ),
];

#[test]
fn the_scrapers_found_something_to_compare() {
    // Both halves guard against a vacuous pass. If either scraper breaks, the
    // parity assertions below compare empty sets and report a clean tree.
    let server = server_routes();
    assert!(
        server.len() > 80,
        "only found {} server routes; the router scraper is broken (there are ~130)",
        server.len()
    );
    for expected in [
        "/api/auth/login",
        "/api/users/{p}",
        "/api/admin/roster",
        "/api/toolguard/tool-on",
    ] {
        assert!(
            server.contains(expected),
            "server scraper missed {expected}"
        );
    }

    let ts = client_paths("frontend/src", &["ts", "vue"], Prefix::AxiosBaseUrl);
    assert!(ts.len() > 50, "only found {} frontend paths", ts.len());

    let rs = client_paths("cli/src", &["rs"], Prefix::Absolute);
    assert!(rs.len() >= 5, "only found {} cli paths", rs.len());
}

#[test]
fn every_cli_path_resolves_to_a_server_route() {
    let server = server_routes();
    let unresolved: Vec<String> = client_paths("cli/src", &["rs"], Prefix::Absolute)
        .into_iter()
        .filter(|p| !server.contains(p))
        .filter(|p| !UNRESOLVED.iter().any(|(known, _)| known == p))
        .collect();

    assert!(
        unresolved.is_empty(),
        "these CLI paths match no server route: {unresolved:#?}\n\
         A path with no route 404s, or returns index.html through the static-file \
         fallback, which is worse because it looks like a success."
    );
}

#[test]
fn every_frontend_path_resolves_to_a_server_route() {
    let server = server_routes();
    let unresolved: Vec<String> =
        client_paths("frontend/src", &["ts", "vue"], Prefix::AxiosBaseUrl)
            .into_iter()
            .filter(|p| !server.contains(p))
            .filter(|p| !UNRESOLVED.iter().any(|(known, _)| known == p))
            .collect();

    assert!(
        unresolved.is_empty(),
        "these frontend paths match no server route: {unresolved:#?}"
    );
}

#[test]
fn the_unresolved_list_has_no_stale_entries() {
    // An entry that now resolves is a fix nobody removed the note for, and it
    // would go on excusing a future regression at the same path.
    let server = server_routes();
    for (path, _reason) in UNRESOLVED {
        assert!(
            !server.contains(&normalize(path)),
            "{path} is on the unresolved list but the server now serves it; \
             remove the entry"
        );
    }
}
