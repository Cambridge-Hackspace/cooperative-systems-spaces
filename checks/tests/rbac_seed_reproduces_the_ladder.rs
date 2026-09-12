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

/// Every migration `up.sql`, concatenated. The RBAC seed is spread across more
/// than one migration -- the tier roles and inheritance land in the Phase 1
/// migration, and later migrations add granular permissions and grants -- so
/// the ladder the resolver actually sees is the union of them all. Parsing the
/// whole corpus (rather than one pinned file) keeps this oracle honest as the
/// catalog grows.
fn seed_sql() -> String {
    let root = repo_root().join("server/migrations");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "up.sql") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut sql = String::new();
    for f in files {
        sql.push_str(&std::fs::read_to_string(&f).unwrap_or_default());
        sql.push_str("\n;\n");
    }
    sql
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

/// Every statement beginning `INSERT INTO <table>` across the concatenated
/// migration corpus. There can be more than one per table -- e.g. grants are
/// seeded in one migration and extended in another -- and all of them count.
/// Splitting on `;` is safe: the RBAC seed's INSERTs contain no inner
/// semicolons. The table name is matched at a word boundary so `roles` does not
/// also catch `role_permissions`, `role_inheritance`, or `user_roles`.
fn statements(sql: &str, insert_into: &str) -> Vec<String> {
    let needle = format!("insert into {}", insert_into.to_lowercase());
    sql.split(';')
        .filter(|stmt| {
            let lower = stmt.to_lowercase();
            match lower.find(&needle) {
                Some(pos) => lower[pos + needle.len()..]
                    .chars()
                    .next()
                    .map(|c| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(true),
                None => false,
            }
        })
        .map(|s| s.to_string())
        .collect()
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
    for stmt in statements(sql, "roles") {
        for row in value_rows(&stmt) {
            assert_eq!(row.len(), 4, "roles seed tuple shape changed: {row:?}");
            let level: i64 = row[3].trim().parse().unwrap_or_else(|_| {
                panic!("role {}'s level is not an integer: {:?}", row[0], row[3])
            });
            levels.insert(row[0].clone(), level);
        }
    }

    // role_permissions: JOIN (VALUES ('member','member.access'), ...), possibly
    // across several migrations.
    let mut own: HashMap<String, HashSet<String>> = HashMap::new();
    for stmt in statements(sql, "role_permissions") {
        for row in value_rows(&stmt) {
            assert_eq!(
                row.len(),
                2,
                "role_permissions seed tuple shape changed: {row:?}"
            );
            own.entry(row[0].clone())
                .or_default()
                .insert(row[1].clone());
        }
    }

    // role_inheritance: JOIN (VALUES ('admin','staff'), ...) child->parent
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for stmt in statements(sql, "role_inheritance") {
        for row in value_rows(&stmt) {
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

/// Phase 2 migrated the in-handler `can_access_staff()` overrides (users,
/// profiles, trainers, training) onto granular permissions. Those gates
/// previously admitted staff and admin and nobody else, so the granular grants
/// must reproduce exactly that: held by `staff` (and `admin` via inheritance),
/// denied to `member`, `newbie`, and `unknown`. If a future migration widened a
/// grant -- say, handed `users.manage` to `member` -- a route that still reads
/// like a staff gate would quietly admit members, and this catches it.
#[test]
fn granular_gate_permissions_reproduce_the_staff_override() {
    let sql = seed_sql();
    let (_levels, own, edges) = parse_seed(&sql);

    // The permissions the migrated in-handler gates consult. Keep in lockstep
    // with the `role_has_permission(.., "..")` calls in server/src/api/*.rs.
    let staff_gates = [
        "users.manage",
        "profiles.manage",
        "training.certify",
        "trainers.manage",
    ];
    let roles = ["unknown", "newbie", "member", "staff", "admin"];

    for perm in staff_gates {
        // Anti-vacuity: the permission has to actually exist as a grant, or the
        // "denied to everyone below staff" checks would pass over a typo.
        assert!(
            own.values().any(|keys| keys.contains(perm)),
            "`{perm}` is granted to no role at all -- either the migration is \
             missing or the permission key was renamed without updating this \
             oracle and the handlers that read it."
        );
        for role in roles {
            let has = effective_perms(role, &own, &edges).contains(perm);
            let should = role == "staff" || role == "admin";
            assert_eq!(
                has,
                should,
                "granular gate `{perm}` is held by the wrong roles: `{role}` {} \
                 have it but the staff-override rule says it {}",
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
