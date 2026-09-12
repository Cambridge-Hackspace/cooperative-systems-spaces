//! Role-based access control model + resolver (#65).
//!
//! The data model (roles, permissions, the role x permission matrix, role
//! inheritance, and user<->role assignments) lives in the `..._add_rbac`
//! migration; this resolver computes a user's *effective* permissions and tier
//! level from it. As of Phase 2 it is the enforcement path: the auth extractors
//! and the in-handler gates resolve through [`RoleGraph::has_permission_for_role_name`]
//! (keyed on `users.role`), and [`crate::database::DatabaseManager`] caches the
//! graph. Phase 3 will switch the key from the single `users.role` to the
//! multi-role `user_roles` assignment.
//!
//! The seed migration mirrors the legacy 5-role ladder, so `effective_permissions`
//! reproduces the old `can_access_*` tiers exactly -- proven by the pure tests
//! below and the source oracle `checks/tests/rbac_seed_reproduces_the_ladder.rs`,
//! so switching enforcement onto it changed no behaviour.

use std::collections::{HashMap, HashSet};

use diesel::pg::PgConnection;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::{role_inheritance, role_permissions, roles, user_roles};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = roles)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub is_system: bool,
    pub level: i16,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// The role graph loaded into memory: each role's own permissions, its direct
/// inheritance edges, and its tier level. Effective permissions are the
/// transitive closure over inheritance; effective level is the max across a
/// user's roles (and inherited roles). Cheap to hold; rebuild on any RBAC write.
#[derive(Debug, Clone, Default)]
pub struct RoleGraph {
    own_perms: HashMap<Uuid, HashSet<String>>,
    inherits: HashMap<Uuid, Vec<Uuid>>,
    level: HashMap<Uuid, i16>,
    by_name: HashMap<String, Uuid>,
}

impl RoleGraph {
    /// Build from raw rows (pure; unit-tested).
    pub fn from_rows(
        role_rows: &[(Uuid, String, i16)], // (id, name, level)
        perm_rows: &[(Uuid, String)],      // (role_id, permission_key)
        edge_rows: &[(Uuid, Uuid)],        // (role_id, inherits_role_id)
    ) -> Self {
        let mut g = RoleGraph::default();
        for (id, name, level) in role_rows {
            g.level.insert(*id, *level);
            g.by_name.insert(name.clone(), *id);
            g.own_perms.entry(*id).or_default();
            g.inherits.entry(*id).or_default();
        }
        for (rid, key) in perm_rows {
            g.own_perms.entry(*rid).or_default().insert(key.clone());
        }
        for (rid, parent) in edge_rows {
            g.inherits.entry(*rid).or_default().push(*parent);
        }
        g
    }

    pub fn load(conn: &mut PgConnection) -> QueryResult<Self> {
        let role_rows: Vec<(Uuid, String, i16)> = roles::table
            .select((roles::id, roles::name, roles::level))
            .load(conn)?;
        let perm_rows: Vec<(Uuid, String)> = role_permissions::table
            .select((role_permissions::role_id, role_permissions::permission_key))
            .load(conn)?;
        let edge_rows: Vec<(Uuid, Uuid)> = role_inheritance::table
            .select((
                role_inheritance::role_id,
                role_inheritance::inherits_role_id,
            ))
            .load(conn)?;
        Ok(Self::from_rows(&role_rows, &perm_rows, &edge_rows))
    }

    pub fn role_id(&self, name: &str) -> Option<Uuid> {
        self.by_name.get(name).copied()
    }

    /// Effective permissions for a set of roles named by their `roles.name`.
    /// Names that do not resolve are skipped (an unseeded role grants nothing).
    pub fn effective_permissions_by_names(&self, names: &[&str]) -> HashSet<String> {
        let ids: Vec<Uuid> = names
            .iter()
            .filter_map(|n| self.by_name.get(*n).copied())
            .collect();
        self.effective_permissions(&ids)
    }

    /// Does the role named `role_name` (via inheritance) hold `key`? This is the
    /// enforcement entry point in Phase 2, where a user carries a single role
    /// name (`users.role`); Phase 3 switches to the multi-role `user_roles`
    /// assignment. An unknown role name holds nothing, so it denies.
    pub fn has_permission_for_role_name(&self, role_name: &str, key: &str) -> bool {
        match self.by_name.get(role_name) {
            Some(id) => self.has_permission(&[*id], key),
            None => false,
        }
    }

    /// Every role reachable from `roles` via inheritance (inclusive). Cycle-safe.
    fn closure(&self, roles: &[Uuid]) -> HashSet<Uuid> {
        let mut seen = HashSet::new();
        let mut stack = roles.to_vec();
        while let Some(r) = stack.pop() {
            if !seen.insert(r) {
                continue;
            }
            if let Some(parents) = self.inherits.get(&r) {
                stack.extend(parents.iter().copied());
            }
        }
        seen
    }

    /// Union of own permissions across the inheritance closure of the given roles.
    pub fn effective_permissions(&self, roles: &[Uuid]) -> HashSet<String> {
        let mut out = HashSet::new();
        for r in self.closure(roles) {
            if let Some(p) = self.own_perms.get(&r) {
                out.extend(p.iter().cloned());
            }
        }
        out
    }

    pub fn has_permission(&self, roles: &[Uuid], key: &str) -> bool {
        self.effective_permissions(roles).contains(key)
    }

    /// Max tier level across the closure (0 when the user has no roles).
    pub fn effective_level(&self, roles: &[Uuid]) -> i16 {
        self.closure(roles)
            .iter()
            .filter_map(|r| self.level.get(r))
            .copied()
            .max()
            .unwrap_or(0)
    }

    /// True if the inheritance edges contain a cycle. Used to reject a bad edge
    /// before it is written (Phase 3); the seed is acyclic.
    pub fn has_cycle(&self) -> bool {
        // white/grey/black DFS
        #[derive(Clone, Copy, PartialEq)]
        enum M {
            White,
            Grey,
            Black,
        }
        let mut mark: HashMap<Uuid, M> = self.inherits.keys().map(|k| (*k, M::White)).collect();
        fn dfs(n: Uuid, inh: &HashMap<Uuid, Vec<Uuid>>, mark: &mut HashMap<Uuid, M>) -> bool {
            mark.insert(n, M::Grey);
            if let Some(ps) = inh.get(&n) {
                for &p in ps {
                    match mark.get(&p).copied().unwrap_or(M::White) {
                        M::Grey => return true,
                        M::White => {
                            if dfs(p, inh, mark) {
                                return true;
                            }
                        }
                        M::Black => {}
                    }
                }
            }
            mark.insert(n, M::Black);
            false
        }
        let nodes: Vec<Uuid> = self.inherits.keys().copied().collect();
        for n in nodes {
            if mark.get(&n).copied().unwrap_or(M::White) == M::White
                && dfs(n, &self.inherits, &mut mark)
            {
                return true;
            }
        }
        false
    }
}

/// The role ids assigned to a user.
pub fn roles_for_user(conn: &mut PgConnection, uid: Uuid) -> QueryResult<Vec<Uuid>> {
    user_roles::table
        .filter(user_roles::user_id.eq(uid))
        .select(user_roles::role_id)
        .load(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    // A graph shaped like the seed: admin(4)->staff(3)->member(2)->newbie(1)->unknown(0),
    // with member.access/staff.access/admin.access owned by member/staff/admin.
    fn ladder() -> (RoleGraph, [Uuid; 5]) {
        let (unknown, newbie, member, staff, admin) = (uid(0), uid(1), uid(2), uid(3), uid(4));
        let roles = [
            (unknown, "unknown".into(), 0i16),
            (newbie, "newbie".into(), 1),
            (member, "member".into(), 2),
            (staff, "staff".into(), 3),
            (admin, "admin".into(), 4),
        ];
        let perms = [
            (member, "member.access".to_string()),
            (staff, "staff.access".to_string()),
            (admin, "admin.access".to_string()),
        ];
        let edges = [
            (admin, staff),
            (staff, member),
            (member, newbie),
            (newbie, unknown),
        ];
        (
            RoleGraph::from_rows(&roles, &perms, &edges),
            [unknown, newbie, member, staff, admin],
        )
    }

    #[test]
    fn seed_reproduces_the_legacy_can_access_tiers() {
        let (g, [unknown, newbie, member, staff, admin]) = ladder();
        // can_access_admin = admin only
        assert!(g.has_permission(&[admin], "admin.access"));
        for r in [unknown, newbie, member, staff] {
            assert!(
                !g.has_permission(&[r], "admin.access"),
                "only admin has admin.access"
            );
        }
        // can_access_staff = staff|admin
        for r in [staff, admin] {
            assert!(g.has_permission(&[r], "staff.access"));
        }
        for r in [unknown, newbie, member] {
            assert!(!g.has_permission(&[r], "staff.access"));
        }
        // can_access_member = member|staff|admin
        for r in [member, staff, admin] {
            assert!(
                g.has_permission(&[r], "member.access"),
                "member+ has member.access"
            );
        }
        for r in [unknown, newbie] {
            assert!(!g.has_permission(&[r], "member.access"));
        }
    }

    #[test]
    fn multiple_roles_union_their_permissions() {
        let (g, [_, newbie, member, staff, _]) = ladder();
        // A user who is both newbie and staff has staff+member (via staff), not admin.
        let eff = g.effective_permissions(&[newbie, staff]);
        assert!(eff.contains("staff.access") && eff.contains("member.access"));
        assert!(!eff.contains("admin.access"));
        // effective level = max across roles (staff=3).
        assert_eq!(g.effective_level(&[newbie, staff]), 3);
        assert_eq!(g.effective_level(&[member]), 2);
        assert_eq!(g.effective_level(&[]), 0);
    }

    #[test]
    fn the_seed_graph_is_acyclic_and_a_cycle_is_detected() {
        let (g, _) = ladder();
        assert!(!g.has_cycle(), "the seeded ladder must be acyclic");
        // introduce a back-edge unknown->admin to make a cycle.
        let (unknown, admin) = (uid(0), uid(4));
        let mut bad = g.clone();
        bad.inherits.entry(unknown).or_default().push(admin);
        assert!(bad.has_cycle(), "a back-edge must be detected as a cycle");
    }
}
