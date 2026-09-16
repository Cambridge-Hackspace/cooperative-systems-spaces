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
        // Continue only if a comma separates the next tuple. Otherwise the
        // VALUES list has ended -- what follows is the wrapping `)` and an
        // `AS alias(col, col)` column list, whose `(...)` must NOT be read as a
        // data row (it would look like a bogus grant/edge to a role named
        // after the alias column).
        let mut j = i;
        while j < n && body[j].is_whitespace() {
            j += 1;
        }
        if j < n && body[j] == ',' {
            i = j + 1;
            continue;
        }
        break;
    }
    rows
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// True if `needle` (lowercased table clause, e.g. "insert into roles") appears
/// in `lower` followed by a non-identifier char, so `roles` does not also match
/// `role_permissions` / `role_inheritance` / `user_roles`.
fn targets(lower: &str, needle: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let end = from + rel + needle.len();
        let boundary = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if boundary {
            return true;
        }
        from = from + rel + needle.len();
    }
    false
}

/// Byte index just past the `=` of the first whole-word `key = ` assignment in
/// `part` (case-insensitive), or None. Whole-word so `name` does not match
/// inside `username`/`updated_at` and `level` not inside anything else. Indices
/// are valid in the original `part` too: migrations are ASCII (the
/// `migrations_are_portable` oracle enforces it), so `to_lowercase` preserves
/// every byte offset.
fn assign_pos(part: &str, key: &str) -> Option<usize> {
    let lower = part.to_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(key) {
        let start = from + rel;
        let end = start + key.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            let mut j = end;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'=' {
                let mut k = j + 1;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                return Some(k);
            }
        }
        from = end;
    }
    None
}

/// The single-quoted string starting at byte `k` in `part` (with `''` escapes),
/// or None if `k` is not a quote.
fn quoted_at(part: &str, k: usize) -> Option<String> {
    let b = part.as_bytes();
    if k >= b.len() || b[k] != b'\'' {
        return None;
    }
    let mut i = k + 1;
    let mut out = String::new();
    while i < b.len() {
        if b[i] == b'\'' {
            if i + 1 < b.len() && b[i + 1] == b'\'' {
                out.push('\'');
                i += 2;
                continue;
            }
            return Some(out);
        }
        out.push(b[i] as char);
        i += 1;
    }
    None
}

/// The integer literal starting at byte `k` in `part`, or None.
fn int_at(part: &str, k: usize) -> Option<i64> {
    let b = part.as_bytes();
    let mut i = k;
    let mut s = String::new();
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        s.push(b[i] as char);
        i += 1;
    }
    while i < b.len() && b[i].is_ascii_digit() {
        s.push(b[i] as char);
        i += 1;
    }
    s.parse().ok()
}

fn capture_name(part: &str) -> Option<String> {
    assign_pos(part, "name").and_then(|k| quoted_at(part, k))
}

fn capture_level(part: &str) -> Option<i64> {
    assign_pos(part, "level").and_then(|k| int_at(part, k))
}

/// A tiny, ordered interpreter over the RBAC-relevant migration statements.
///
/// It replays INSERT/UPDATE/DELETE against `roles`, `role_permissions` and
/// `role_inheritance` in document order. A rename (`member` -> `active`), a
/// relevel, a new role (`historical`), a retired role (`unknown`), or a rebuilt
/// inheritance chain in a *later* migration is therefore reflected -- exactly
/// what a text scan of INSERTs alone would miss now that #77 changes the
/// taxonomy by `UPDATE`, not by re-seeding. Each role carries a stable synthetic
/// id, so a grant inserted under an old name follows the role through a rename.
///
/// This is still independent of `server/src/rbac.rs`: the resolver reads the
/// final rows from Postgres; this reconstructs those same rows from the
/// migration *text*. Two encodings of one ladder, asserted to agree.
#[derive(Default)]
struct Ladder {
    next: u64,
    id_of: HashMap<String, u64>,
    name_of: HashMap<u64, String>,
    level: HashMap<u64, i64>,
    own: HashMap<u64, HashSet<String>>,
    edges: HashMap<u64, HashSet<u64>>,
}

impl Ladder {
    fn insert_role(&mut self, name: &str, level: i64) {
        let id = self.next;
        self.next += 1;
        self.id_of.insert(name.to_string(), id);
        self.name_of.insert(id, name.to_string());
        self.level.insert(id, level);
        self.own.entry(id).or_default();
    }

    fn update_role(&mut self, target: &str, new_name: Option<&str>, new_level: Option<i64>) {
        let Some(&id) = self.id_of.get(target) else {
            return;
        };
        if let Some(l) = new_level {
            self.level.insert(id, l);
        }
        if let Some(nn) = new_name {
            if nn != target {
                self.id_of.remove(target);
                self.id_of.insert(nn.to_string(), id);
                self.name_of.insert(id, nn.to_string());
            }
        }
    }

    fn delete_role(&mut self, name: &str) {
        let Some(id) = self.id_of.remove(name) else {
            return;
        };
        self.name_of.remove(&id);
        self.level.remove(&id);
        self.own.remove(&id);
        self.edges.remove(&id);
        for parents in self.edges.values_mut() {
            parents.remove(&id);
        }
    }

    fn grant(&mut self, role: &str, perm: &str) {
        let id = *self.id_of.get(role).unwrap_or_else(|| {
            panic!("grant to unknown role {role:?} -- migration order or a rename is off")
        });
        self.own.entry(id).or_default().insert(perm.to_string());
    }

    fn add_edge(&mut self, child: &str, parent: &str) {
        let c = *self
            .id_of
            .get(child)
            .unwrap_or_else(|| panic!("inheritance child {child:?} not seeded before use"));
        let p = *self
            .id_of
            .get(parent)
            .unwrap_or_else(|| panic!("inheritance parent {parent:?} not seeded before use"));
        self.edges.entry(c).or_default().insert(p);
    }
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

/// Replay the migration corpus and return the FINAL ladder as name-keyed
/// (levels, own-permissions, inheritance-edges) maps.
fn parse_seed(
    sql: &str,
) -> (
    HashMap<String, i64>,
    HashMap<String, HashSet<String>>,
    HashMap<String, Vec<String>>,
) {
    let mut l = Ladder::default();

    for raw in sql.split(';') {
        let lower = raw.to_lowercase();
        if targets(&lower, "insert into roles") {
            for row in value_rows(raw) {
                assert_eq!(row.len(), 4, "roles seed tuple shape changed: {row:?}");
                let level: i64 = row[3].trim().parse().unwrap_or_else(|_| {
                    panic!("role {}'s level is not an integer: {:?}", row[0], row[3])
                });
                l.insert_role(row[0].trim(), level);
            }
        } else if targets(&lower, "insert into role_permissions") {
            for row in value_rows(raw) {
                assert_eq!(
                    row.len(),
                    2,
                    "role_permissions tuple shape changed: {row:?}"
                );
                l.grant(row[0].trim(), row[1].trim());
            }
        } else if targets(&lower, "insert into role_inheritance") {
            for row in value_rows(raw) {
                assert_eq!(
                    row.len(),
                    2,
                    "role_inheritance tuple shape changed: {row:?}"
                );
                l.add_edge(row[0].trim(), row[1].trim());
            }
        } else if targets(&lower, "update roles") {
            let (set_part, where_part) = match lower.find(" where ") {
                Some(w) => (&raw[..w], &raw[w..]),
                None => (raw, ""),
            };
            if let Some(target) = capture_name(where_part) {
                let new_name = capture_name(set_part);
                let new_level = capture_level(set_part);
                l.update_role(&target, new_name.as_deref(), new_level);
            }
        } else if targets(&lower, "delete from role_inheritance") {
            // The taxonomy migration wipes the old chain before rebuilding it.
            l.edges.clear();
        } else if targets(&lower, "delete from roles") {
            if let Some(name) = capture_name(raw) {
                l.delete_role(&name);
            }
        }
    }

    let mut levels = HashMap::new();
    let mut own: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, &id) in &l.id_of {
        levels.insert(name.clone(), l.level[&id]);
        own.insert(name.clone(), l.own.get(&id).cloned().unwrap_or_default());
    }
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (child, parents) in &l.edges {
        let cname = l.name_of[child].clone();
        let e = edges.entry(cname).or_default();
        for p in parents {
            e.push(l.name_of[p].clone());
        }
    }

    (levels, own, edges)
}

#[test]
fn seed_reproduces_the_tier_ladder() {
    let sql = seed_sql();
    let (levels, own, edges) = parse_seed(&sql);

    // Anti-vacuity: the five tier roles with their #77 ladder levels. `guest`
    // (1) is the signed-in baseline that replaced `newbie`/`unknown`;
    // `historical` (2) is a lapsed member; `active` (3) holds member.access.
    let expected_levels: HashMap<&str, i64> = [
        ("guest", 1),
        ("historical", 2),
        ("active", 3),
        ("staff", 4),
        ("admin", 5),
    ]
    .into_iter()
    .collect();
    for (name, lvl) in &expected_levels {
        assert_eq!(
            levels.get(*name).copied(),
            Some(*lvl),
            "role {name} must end at level {lvl} (got {:?})",
            levels.get(*name)
        );
    }
    assert_eq!(
        levels.len(),
        expected_levels.len(),
        "unexpected extra/missing seeded roles after all migrations: {:?}",
        levels.keys().collect::<Vec<_>>()
    );

    // The tier truth table: which roles reach each *.access permission. The
    // permission KEYS are unchanged from the legacy ladder (only the role names
    // moved); member.access is now held by `active` and everything above it.
    let truth: &[(&str, &[&str])] = &[
        ("admin.access", &["admin"]),
        ("staff.access", &["staff", "admin"]),
        ("member.access", &["active", "staff", "admin"]),
    ];
    let roles = ["guest", "historical", "active", "staff", "admin"];
    for (perm, allowed) in truth {
        let allowed: HashSet<&str> = allowed.iter().copied().collect();
        for role in roles {
            let has = effective_perms(role, &own, &edges).contains(*perm);
            let should = allowed.contains(role);
            assert_eq!(
                has,
                should,
                "effective permissions diverged from the tier ladder: \
                 role `{role}` {} have `{perm}` but the seed says it {}",
                if should { "should" } else { "should NOT" },
                if has { "does" } else { "does not" },
            );
        }
    }
}

/// Phase 2 migrated the in-handler `can_access_staff()` overrides (users,
/// profiles, trainers, training) onto granular permissions. Those gates admit
/// staff and admin and nobody else, so the granular grants must reproduce
/// exactly that: held by `staff` (and `admin` via inheritance), denied to
/// `active`, `historical`, and `guest`. If a future migration widened a grant --
/// say, handed `users.manage` to `active` -- a route that still reads like a
/// staff gate would quietly admit members, and this catches it.
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
    let roles = ["guest", "historical", "active", "staff", "admin"];

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
/// make the truth-table recomputation fail. If this "broken" input still
/// passed, the check above would prove nothing. Modelled on the real defect the
/// oracle exists to catch: a missing inheritance edge.
/// The development-instance seed must expect a role this ladder actually
/// defines.
///
/// `e2e/run.sh`'s `devseed` stage signs in as the admin it just created and
/// asserts the primary role that comes back. It asserted `"Admin"` until the
/// rbac_taxonomy migration renamed the roles to lowercase, at which point
/// `reaper test --profile devlive` stopped working -- and stayed broken,
/// because `devseed` is not in `STAGES_DEFAULT` and the battery therefore never
/// runs it. The stage is reachable only through a profile a human invokes by
/// hand, so nothing in CI was ever going to notice.
///
/// This is the cheap half of noticing. It does not run the stage; it asserts
/// that the name the stage is waiting to see is a name the seed can produce,
/// which is the specific thing that went stale. Scoped to this one comparison
/// on purpose: role-shaped string literals elsewhere in `e2e/` are guard names
/// (`Guard::Member` kept its name through #77) and synthetic journey fixtures,
/// and a check that flagged those would be a check somebody switches off.
#[test]
fn the_devseed_stage_expects_a_role_the_seed_defines() {
    let run_sh = css_checks::read("e2e/run.sh");

    // The comparison devseed makes, as it is written in the script.
    let marker = "if [[ ${seen_role} == \"";
    let at = run_sh.find(marker).unwrap_or_else(|| {
        panic!(
            "could not find devseed's role comparison in e2e/run.sh. If it was \
             rewritten, this check has to be rewritten with it rather than left \
             passing on a string that is no longer there."
        )
    });
    let rest = &run_sh[at + marker.len()..];
    let expected = &rest[..rest.find('"').expect("unterminated role literal")];

    let (levels, _, _) = parse_seed(&seed_sql());
    assert!(
        levels.contains_key(expected),
        "e2e/run.sh's devseed stage waits for the primary role {expected:?}, \
         which the RBAC seed does not define. Defined roles: {:?}.\n\n\
         devlive is the instance firmware developers are told to bring up (see \
         FIRMWARE.md); a devseed that cannot pass its own admin check is a \
         development instance nobody can use.",
        {
            let mut names: Vec<&String> = levels.keys().collect();
            names.sort();
            names
        }
    );
}

#[test]
fn the_oracle_catches_a_broken_ladder() {
    let sql = seed_sql();
    let (_levels, own, mut edges) = parse_seed(&sql);

    // Remove the admin->staff edge: admin now loses staff.access + member.access.
    if let Some(parents) = edges.get_mut("admin") {
        parents.retain(|p| p != "staff");
    }

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
