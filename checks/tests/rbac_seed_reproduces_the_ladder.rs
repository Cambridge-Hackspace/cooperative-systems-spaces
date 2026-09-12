//! The #65 RBAC seed must reproduce the legacy `can_access_*` ladder exactly.
//!
//! Phase 1 adds a data-driven RBAC model but changes no behaviour: the seed in
//! `..._add_rbac/up.sql` recreates the old five-rung ladder
//! (`unknown<newbie<member<staff<admin`) as roles + a permission catalog + an
//! inheritance chain, and the enforcement path is meant to keep granting exactly
//! what `can_access_admin/staff/member()` granted before. If someone edits the
//! seed -- drops an inheritance edge, mis-levels a role, moves a grant -- the
//! effective-permission ladder silently diverges from the code that still gates
//! routes. This oracle recomputes the effective permissions FROM the seed's own
//! text and asserts they match the legacy truth table.
//!
//! Per docs/testing-methodology.md ("test against the source, not a re-export"):
//! the transitive-closure logic is duplicated HERE, independently of
//! `server/src/rbac.rs`. If both agreed by construction the check would be
//! vacuous; the point is that the migration text and the resolver are two
//! independent encodings of the same ladder, and this asserts they agree.

use std::collections::{HashMap, HashSet};

use css_checks::repo_root;

const MIGRATION: &str = "server/migrations/2026-09-12-130000-0000_add_rbac/up.sql";

fn seed_sql() -> String {
    std::fs::read_to_string(repo_root().join(MIGRATION))
        .unwrap_or_else(|e| panic!("cannot read {MIGRATION}: {e}"))
}

/// Split a single SQL statement's `VALUES (...), (...)` list into rows of
/// fields. Respects single-quoted strings (with `''` escapes) so a comma inside
/// a description does not split a field. Unquoted tokens (TRUE, 0) are returned
/// verbatim. Everything before the first `VALUES` keyword is ignored, so this
/// works for both `INSERT ... VALUES ...` and `... JOIN (VALUES ...) AS ...`.
fn value_rows(stmt: &str) -> Vec<Vec<String>> {
    let upper = stmt.to_uppercase();
    let Some(kw) = upper.find("VALUES") else {
        return Vec::new();
    };
    let body: Vec<char> = stmt[kw + "VALUES".len()..].chars().collect();
    let mut rows = Vec::new();
    let mut i = 0;
    let n = body.len();
    while i < n {
        // Skip to the next '(' that opens a row.
        while i < n && body[i] != '(' {
            i += 1;
        }
        if i >= n {
            break;
        }
        i += 1; // consume '('
        let mut fields: Vec<String> = Vec::new();
        let mut field = String::new();
        let mut in_str = false;
        let mut depth = 0i32; // nested parens inside a row (none expected, but safe)
        while i < n {
            let c = body[i];
            if in_str {
                if c == '\'' {
                    if i + 1 < n && body[i + 1] == '\'' {
                        field.push('\'');
                        i += 2;
                        continue;
                    }
                    in_str = false;
                    i += 1;
                    continue;
                }
                field.push(c);
                i += 1;
                continue;
            }
            match c {
                '\'' => {
                    in_str = true;
                    i += 1;
                }
                '(' => {
                    depth += 1;
                    field.push(c);
                    i += 1;
                }
                ')' if depth > 0 => {
                    depth -= 1;
                    field.push(c);
                    i += 1;
                }
                ')' => {
                    fields.push(field.trim().to_string());
                    i += 1; // consume ')'
                    break;
                }
                ',' => {
                    fields.push(field.trim().to_string());
                    field.clear();
                    i += 1;
                }
                _ => {
                    field.push(c);
                    i += 1;
                }
            }
        }
        rows.push(fields);
    }
    rows
}

/// The statement beginning with `INSERT INTO <table>` (semicolon-terminated).
fn statement(sql: &str, insert_into: &str) -> String {
    let upper = sql.to_uppercase();
    let needle = format!("INSERT INTO {}", insert_into.to_uppercase());
    let start = upper
        .find(&needle)
        .unwrap_or_else(|| panic!("no `INSERT INTO {insert_into}` in the seed"));
    let end = sql[start..]
        .find(';')
        .map(|e| start + e)
        .unwrap_or(sql.len());
    sql[start..end].to_string()
}

/// Transitive closure of `roles` over the inheritance edges (inclusive),
/// re-implemented here independently of `server/src/rbac.rs`.
fn closure(roots: &[&str], edges: &HashMap<String, Vec<String>>) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = roots.iter().map(|s| s.to_string()).collect();
    while let Some(r) = stack.pop() {
        if !seen.insert(r.clone()) {
            continue;
        }
        if let Some(ps) = edges.get(&r) {
            stack.extend(ps.iter().cloned());
        }
    }
    seen
}

fn effective_perms(
    role: &str,
    own: &HashMap<String, HashSet<String>>,
    edges: &HashMap<String, Vec<String>>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for r in closure(&[role], edges) {
        if let Some(p) = own.get(&r) {
            out.extend(p.iter().cloned());
        }
    }
    out
}

/// Parse the seed into (levels, own-permissions, inheritance-edges).
fn parse_seed(
    sql: &str,
) -> (
    HashMap<String, i64>,
    HashMap<String, HashSet<String>>,
    HashMap<String, Vec<String>>,
) {
    // roles: (name, description, is_system, level)
    let mut levels = HashMap::new();
    for row in value_rows(&statement(sql, "roles")) {
        assert_eq!(row.len(), 4, "roles seed tuple shape changed: {row:?}");
        let level: i64 = row[3]
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("role {}'s level is not an integer: {:?}", row[0], row[3]));
        levels.insert(row[0].clone(), level);
    }

    // role_permissions: JOIN (VALUES ('member','member.access'), ...)
    let mut own: HashMap<String, HashSet<String>> = HashMap::new();
    for row in value_rows(&statement(sql, "role_permissions")) {
        assert_eq!(
            row.len(),
            2,
            "role_permissions seed tuple shape changed: {row:?}"
        );
        own.entry(row[0].clone())
            .or_default()
            .insert(row[1].clone());
    }

    // role_inheritance: JOIN (VALUES ('admin','staff'), ...) child->parent
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for row in value_rows(&statement(sql, "role_inheritance")) {
        assert_eq!(
            row.len(),
            2,
            "role_inheritance seed tuple shape changed: {row:?}"
        );
        edges
            .entry(row[0].clone())
            .or_default()
            .push(row[1].clone());
    }

    (levels, own, edges)
}

#[test]
fn seed_reproduces_the_legacy_ladder() {
    let sql = seed_sql();
    let (levels, own, edges) = parse_seed(&sql);

    // Anti-vacuity: the five legacy roles with their ladder levels.
    let expected_levels: HashMap<&str, i64> = [
        ("unknown", 0),
        ("newbie", 1),
        ("member", 2),
        ("staff", 3),
        ("admin", 4),
    ]
    .into_iter()
    .collect();
    for (name, lvl) in &expected_levels {
        assert_eq!(
            levels.get(*name).copied(),
            Some(*lvl),
            "role {name} must be seeded at level {lvl} (got {:?})",
            levels.get(*name)
        );
    }
    assert_eq!(
        levels.len(),
        expected_levels.len(),
        "unexpected extra/missing seeded roles: {:?}",
        levels.keys().collect::<Vec<_>>()
    );

    // The legacy truth table: which roles can reach each *.access permission,
    // mirroring can_access_member/staff/admin (each tier and everything above).
    let truth: &[(&str, &[&str])] = &[
        ("admin.access", &["admin"]),
        ("staff.access", &["staff", "admin"]),
        ("member.access", &["member", "staff", "admin"]),
    ];
    let roles = ["unknown", "newbie", "member", "staff", "admin"];
    for (perm, allowed) in truth {
        let allowed: HashSet<&str> = allowed.iter().copied().collect();
        for role in roles {
            let has = effective_perms(role, &own, &edges).contains(*perm);
            let should = allowed.contains(role);
            assert_eq!(
                has,
                should,
                "effective permissions diverged from the legacy ladder: \
                 role `{role}` {} have `{perm}` but the seed says it {}",
                if should { "should" } else { "should NOT" },
                if has { "does" } else { "does not" },
            );
        }
    }
}

/// Self-test the oracle: a seed with a BROKEN ladder (drop admin->staff) must
/// make the truth-table assertion fail. If this "broken" input still passed,
/// the check above would prove nothing. Modelled on the real defect the oracle
/// exists to catch: a missing inheritance edge.
#[test]
fn the_oracle_catches_a_broken_ladder() {
    let sql = seed_sql();
    let (_levels, own, mut edges) = parse_seed(&sql);

    // Remove the admin->staff edge: admin now loses staff.access + member.access.
    if let Some(parents) = edges.get_mut("admin") {
        parents.retain(|p| p != "staff");
    }

    // With the edge gone, admin must NO LONGER reach staff.access -- prove the
    // recomputation actually reflects the (broken) edges rather than a constant.
    let admin_perms = effective_perms("admin", &own, &edges);
    assert!(
        admin_perms.contains("admin.access"),
        "admin keeps its own admin.access grant"
    );
    assert!(
        !admin_perms.contains("staff.access"),
        "self-test is inert: dropping admin->staff should strip inherited staff.access"
    );
    assert!(
        !admin_perms.contains("member.access"),
        "self-test is inert: dropping admin->staff should strip inherited member.access"
    );
}
