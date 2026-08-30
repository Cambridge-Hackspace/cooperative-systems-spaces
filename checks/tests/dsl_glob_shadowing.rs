//! A `use ...::dsl::*` must not shadow a function's own parameters.
//!
//! `server/src/database.rs` had this:
//!
//! ```ignore
//! pub fn remove_tool_trainer_by_user_tool(&self, user_id: Uuid, tool_id: Uuid) -> ... {
//!     use crate::schema::tool_trainers::dsl::*;
//!     diesel::update(
//!         tool_trainers
//!             .filter(user_id.eq(user_id))     // column = column
//!             .filter(tool_id.eq(tool_id))     // column = column
//!             .filter(is_active.eq(true)),
//!     )
//!     .set((is_active.eq(false), ...))
//! ```
//!
//! The glob brings every *column* of the table into scope, so inside the body
//! `user_id` is the column, not the parameter. Both filters compile as
//! `column = column`, which is true for every row — and the update therefore
//! deactivated **every trainer assignment in the table** on a call that reads as
//! "unassign this one trainer from this one tool".
//!
//! It typechecks. It is not a warning about the filter. rustc's only complaint
//! is "unused variable: `user_id`", four lines above, which reads like a tidying
//! job rather than a query that does the opposite of what it says. The only
//! reason it never destroyed anybody's data is that nothing called it.
//!
//! The sibling two functions above does the same job correctly, using
//! `user_id_param` and `tool_id_param` — so the convention already existed and
//! the duplicate simply did not follow it. That is exactly the kind of thing a
//! check is for and a code review is not.
//!
//! This check is source-as-data and needs no database, no server crate, and no
//! compilation of anything. It runs in milliseconds.

use css_checks::{read, repo_root};
use std::collections::{BTreeMap, BTreeSet};

/// Column names per table, from `diesel::table!` blocks in `server/src/schema.rs`.
fn columns_by_table() -> BTreeMap<String, BTreeSet<String>> {
    let src = read("server/src/schema.rs");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut current: Option<String> = None;
    let mut depth = 0usize;

    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("");
        let trimmed = code.trim();

        if current.is_none() {
            // `    table_name (id) {`  — the table's own opening line.
            if let Some(open) = trimmed.find(" (") {
                if trimmed.ends_with('{') && !trimmed.starts_with("diesel::") {
                    let name = trimmed[..open].trim();
                    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        current = Some(name.to_string());
                        depth = 1;
                        continue;
                    }
                }
            }
            continue;
        }

        depth += trimmed.matches('{').count();
        depth = depth.saturating_sub(trimmed.matches('}').count());
        if depth == 0 {
            current = None;
            continue;
        }

        // `        column_name -> Type,`
        if let Some(arrow) = trimmed.find("->") {
            let name = trimmed[..arrow].trim().trim_start_matches('#').trim();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                if let Some(table) = &current {
                    out.entry(table.clone())
                        .or_default()
                        .insert(name.to_string());
                }
            }
        }
    }
    out
}

/// Rust files under `server/src` that could contain a dsl glob.
fn server_sources() -> Vec<std::path::PathBuf> {
    let root = repo_root().join("server/src");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// `(file, line, fn name, parameter names)` for every function in a file.
///
/// Deliberately line-based rather than a parse. The thing being looked for is a
/// glob import and a parameter list, both of which are unambiguous at the line
/// level, and a `syn` dependency here would make the cheapest tier in the
/// repository stop being cheap. `both_sides_were_actually_parsed` is what keeps
/// the scanner honest.
struct Function {
    file: String,
    line: usize,
    name: String,
    params: Vec<String>,
    body: String,
}

fn functions_in(path: &std::path::Path) -> Vec<Function> {
    let Ok(src) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let rel = path
        .strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("pub(crate) fn "))
        {
            continue;
        }
        let Some(paren) = trimmed.find('(') else {
            continue;
        };
        let name = trimmed[..paren]
            .rsplit(' ')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }

        // The signature, to the balanced closing paren, and then the body to the
        // next line at the same indentation that closes it.
        let indent = line.len() - trimmed.len();
        let mut sig = String::new();
        let mut depth = 0i32;
        let mut j = i;
        while j < lines.len() {
            for c in lines[j].chars() {
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                }
            }
            sig.push_str(lines[j]);
            sig.push('\n');
            if depth <= 0 && j > i || (depth == 0 && j == i) {
                break;
            }
            j += 1;
        }

        let mut body = String::new();
        let mut k = j;
        while k < lines.len() {
            body.push_str(lines[k]);
            body.push('\n');
            let l = lines[k];
            if k > j && l.len() > indent && !l[..indent].trim().is_empty() {
                break;
            }
            if k > j && l.trim() == "}" && (l.len() - l.trim_start().len()) == indent {
                break;
            }
            k += 1;
        }

        // Parameter names: `name: Type` at the top level of the signature.
        let open = sig.find('(').map(|x| x + 1).unwrap_or(0);
        let close = sig.rfind(')').unwrap_or(sig.len());
        let inside = &sig[open.min(close)..close];
        let mut params = Vec::new();
        let mut angle = 0i32;
        let mut paren_depth = 0i32;
        let mut current = String::new();
        for c in inside.chars() {
            match c {
                '<' => angle += 1,
                '>' => angle -= 1,
                '(' | '[' => paren_depth += 1,
                ')' | ']' => paren_depth -= 1,
                ',' if angle == 0 && paren_depth == 0 => {
                    params.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            }
            current.push(c);
        }
        params.push(current);

        let params: Vec<String> = params
            .iter()
            .filter_map(|p| {
                let p = p.trim();
                if p.is_empty() || p.starts_with('&') || p == "self" || p.starts_with("mut self") {
                    return None;
                }
                let name = p
                    .split(':')
                    .next()?
                    .trim()
                    .trim_start_matches("mut ")
                    .trim();
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return None;
                }
                Some(name.to_string())
            })
            .collect();

        out.push(Function {
            file: rel.clone(),
            line: i + 1,
            name,
            params,
            body,
        });
    }
    out
}

#[test]
fn both_sides_were_actually_parsed() {
    let tables = columns_by_table();
    assert!(
        tables.len() >= 20,
        "parsed only {} tables from server/src/schema.rs; the scan is broken",
        tables.len()
    );
    assert!(
        tables
            .get("tool_trainers")
            .is_some_and(|c| c.contains("user_id") && c.contains("tool_id")),
        "tool_trainers' columns did not parse; this check's own example would not be caught"
    );

    let total: usize = server_sources().iter().map(|p| functions_in(p).len()).sum();
    assert!(
        total >= 200,
        "found only {total} functions under server/src; the scan is broken"
    );
}

#[test]
fn no_dsl_glob_shadows_a_parameter() {
    let tables = columns_by_table();
    let mut offenders = Vec::new();

    for path in server_sources() {
        for f in functions_in(&path) {
            for line in f.body.lines() {
                let code = line.split("//").next().unwrap_or("").trim();
                // `use crate::schema::<table>::dsl::*;`
                let Some(rest) = code.strip_prefix("use crate::schema::") else {
                    continue;
                };
                let Some(table) = rest.strip_suffix("::dsl::*;") else {
                    continue;
                };
                let Some(columns) = tables.get(table) else {
                    continue;
                };

                for param in &f.params {
                    if columns.contains(param) {
                        offenders.push(format!(
                            "{}:{} fn {}: parameter `{}` is shadowed by the `{}` column \
                             brought in by `use crate::schema::{}::dsl::*`",
                            f.file, f.line, f.name, param, param, table
                        ));
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a dsl glob shadows a function parameter:\n{}\n\n\
         Inside the body the name is the *column*, not the argument, so \
         `.filter(user_id.eq(user_id))` compiles as `user_id = user_id` -- true \
         for every row. It typechecks; rustc's only complaint is \"unused \
         variable\", several lines away. A query written that way in an UPDATE \
         touches the whole table.\n\n\
         The convention already in use is a `_param` suffix, as in \
         `DatabaseManager::remove_tool_trainer`.",
        offenders.join("\n")
    );
}
