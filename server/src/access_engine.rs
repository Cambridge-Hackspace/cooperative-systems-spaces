//! The one access decision, for every resource (#101).
//!
//! Doors and tools are both access-controlled resources; this is the single
//! decision function that subsumes the two evaluators they use today --
//! `DoorService::evaluate` (`doors.rs`) and `user_is_authorized_for_tool`
//! (`database.rs`). It is pure and db-free: every input is materialized by the
//! caller, so `contracts/door_rules.json` can drive it without a database,
//! exactly as it already drives `expand_rules_at`.
//!
//! It reuses the door rule primitives (`schedule_state_at`, `rule_fires`,
//! `DoorRuleKind`/`DoorRuleEffect`) rather than reimplementing them, so it agrees
//! with `expand_rules_at`/`evaluate` by construction rather than by luck.
//!
//! Slice 2a lands it ADDITIVELY: nothing calls it in production yet. The old
//! evaluators still run; this is unit- and vector-tested in isolation first, then
//! wired in -- retiring them -- in slice 2b.

use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::doors::{rule_fires, schedule_state_at};
use crate::models::{AccessRule, DoorRuleEffect, DoorRuleKind, Schedule};
use crate::rbac::RoleGraph;

/// What a principal is trying to do to a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Use a tool (energize it).
    Use,
    /// Unlock a door in person (the QR check-in path).
    Unlock,
    /// Unlock a door on someone's behalf from elsewhere. Composes with the
    /// resource's rules; the permission gate (`door.unlock.remote`) is applied by
    /// the caller, not here (slice 3/4).
    UnlockRemote,
}

impl Action {
    /// Actions that identify a person, and so require an active principal. Every
    /// action does today; the method exists to make the exception explicit if an
    /// anonymous action is ever added.
    fn needs_identity(self) -> bool {
        match self {
            Action::Use | Action::Unlock | Action::UnlockRemote => true,
        }
    }
}

/// Why a resource refused. Carries enough to reproduce today's messages without
/// dictating them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    /// Locked out / unsafe -- refused to everyone (safety; a hard override).
    LockedOut,
    /// Unavailable for this action (a door disabled, a tool in an unusable
    /// status); carries the caller's reason string.
    Unavailable(String),
    /// The principal is inactive.
    Inactive,
    /// An explicit deny rule matched the principal.
    Rule,
    /// No allow rule matched and the resource is not open by default.
    NoMatch,
    /// A training-controlled tool the principal has not satisfied (no completed
    /// steps and no waiver).
    Training,
    /// A metered tool the principal cannot be billed on.
    Billing,
}

/// The decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

impl Decision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// The principal, materialized. `level` is the RBAC effective level (0 if none);
/// `card_digests` are the wire-digests that identify this principal, matching what
/// a `kind=card` rule stores at rest.
#[derive(Debug, Clone)]
pub struct Principal {
    pub user_id: Option<Uuid>,
    pub level: i16,
    pub card_digests: BTreeSet<String>,
    pub is_active: bool,
}

/// What "no matching rule" means for a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultEffect {
    /// No matching rule -> allowed. A tool: open to all, subject to training.
    /// (`user_is_authorized_for_tool`'s "not access controlled -> true".)
    Open,
    /// No matching rule -> denied. A door. (`evaluate`'s "No matching access rule".)
    Restricted,
}

/// A resource's policy, materialized by the caller.
#[derive(Debug, Clone)]
pub struct ResourcePolicy {
    /// Locked out / unsafe -> refuse everyone. Generalizes `tool_is_locked_out`.
    pub locked_out: bool,
    /// `Some(reason)` if unavailable for the action (a disabled door, a tool
    /// status that cannot be used). `None` = available.
    pub unavailable: Option<String>,
    /// What "no matching rule" means here.
    pub default_effect: DefaultEffect,
    /// A resource-level schedule (tools). `None` = always open. Gates the
    /// default-open grant, fail-closed, exactly as a rule schedule does. Doors
    /// carry `None` here -- their schedules live on the rules.
    pub schedule_id: Option<Uuid>,
    /// For `Action::Use`: whether the training dimension is satisfied (all steps
    /// complete, OR a waiver, OR the tool is not training-controlled). `None` for
    /// actions where training does not apply (doors).
    pub training_ok: Option<bool>,
    /// For a metered tool under `Action::Use`: `Some(false)` refuses on billing.
    /// `None` = not metered / not applicable.
    pub metered_ok: Option<bool>,
}

/// The one decision.
///
/// Precedence, each step preserving a current behavior:
/// 1. lockout (hard) 2. availability 3. principal viability 4. deny rules
/// 5. deny-precedence 6. positive grant / default-open (schedule-gated,
/// fail-closed) 7. training + metered 8. interlocks (slice 3/4; no-op) 9. allow.
#[allow(clippy::too_many_arguments)]
pub fn may(
    principal: &Principal,
    policy: &ResourcePolicy,
    rules: &[AccessRule],
    schedules: &[Schedule],
    tz: chrono_tz::Tz,
    now: DateTime<Utc>,
    graph: &RoleGraph,
    action: Action,
) -> Decision {
    // 1. Locked out / unsafe -> nobody, regardless of any allow.
    if policy.locked_out {
        return Decision::Deny(DenyReason::LockedOut);
    }
    // 2. Availability for the action.
    if let Some(reason) = &policy.unavailable {
        return Decision::Deny(DenyReason::Unavailable(reason.clone()));
    }
    // 3. Principal viability.
    if action.needs_identity() && !principal.is_active {
        return Decision::Deny(DenyReason::Inactive);
    }

    // 4. Rules. Identical shape to DoorService::evaluate: effect parsed first, the
    //    schedule gate fail-closed per effect, unknown kind/effect skipped.
    let user_id_str = principal.user_id.map(|u| u.to_string());
    let mut matched_allow = false;
    let mut matched_deny = false;
    for rule in rules {
        let effect = match DoorRuleEffect::parse(&rule.effect) {
            Some(e) => e,
            None => continue,
        };
        if !rule_fires(
            effect,
            schedule_state_at(rule.schedule_id, schedules, tz, now),
        ) {
            continue;
        }
        let kind = match DoorRuleKind::parse(&rule.kind) {
            Some(k) => k,
            None => continue,
        };
        let matched = match kind {
            DoorRuleKind::Card => principal.card_digests.contains(&rule.value),
            DoorRuleKind::User => user_id_str.as_deref() == Some(rule.value.as_str()),
            DoorRuleKind::Role => match graph.level_of_name(&rule.value) {
                Some(required) => principal.level >= required,
                None => false,
            },
            // Open access is a door-level held-unlock latch, never a per-principal
            // grant -- matches `evaluate` (OpenAccess => false). The latch is
            // compiled separately by `open_access_hold_until_at`.
            DoorRuleKind::OpenAccess => false,
        };
        if !matched {
            continue;
        }
        match effect {
            DoorRuleEffect::Allow => matched_allow = true,
            DoorRuleEffect::Deny => matched_deny = true,
        }
    }

    // 5. Deny-precedence: an explicit deny beats any positive grant.
    if matched_deny {
        return Decision::Deny(DenyReason::Rule);
    }

    // 6. Positive grant, or the default-open fallback. `default_effect` reconciles
    //    the two subtypes: a door is Restricted (no match -> deny), a tool is Open
    //    (no match -> allowed, subject to training). The default-open grant is
    //    gated by the resource schedule, fail-closed, which is where an online
    //    tool grant begins honoring the tool schedule (the deliberate #101 change).
    let default_open = matches!(policy.default_effect, DefaultEffect::Open)
        && rule_fires(
            DoorRuleEffect::Allow,
            schedule_state_at(policy.schedule_id, schedules, tz, now),
        );
    if !matched_allow && !default_open {
        return Decision::Deny(DenyReason::NoMatch);
    }

    // 7. Parallel dimensions (tool use). Training and metering are separate from
    //    the rule/role dimension: a training-gated tool must satisfy both.
    if policy.training_ok == Some(false) {
        return Decision::Deny(DenyReason::Training);
    }
    if policy.metered_ok == Some(false) {
        return Decision::Deny(DenyReason::Billing);
    }

    // 8. Interlocks / presence: slice 3/4 (no-op here).
    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> chrono_tz::Tz {
        "UTC".parse().unwrap()
    }

    /// A role graph with the seeded ladder, matching `door_vectors.rs::seed_graph`.
    fn graph() -> RoleGraph {
        let rows: Vec<(Uuid, String, i16)> = [
            ("guest", 1),
            ("historical", 2),
            ("active", 3),
            ("staff", 4),
            ("admin", 5),
        ]
        .iter()
        .enumerate()
        .map(|(i, (n, l))| (Uuid::from_u128(i as u128 + 1), n.to_string(), *l))
        .collect();
        RoleGraph::from_rows(&rows, &[], &[])
    }

    fn rule(kind: &str, value: &str, effect: &str) -> AccessRule {
        AccessRule {
            id: Uuid::new_v4(),
            resource_id: Uuid::nil(),
            kind: kind.into(),
            value: value.into(),
            effect: effect.into(),
            created_at: Utc::now(),
            schedule_id: None,
        }
    }

    fn principal(level: i16) -> Principal {
        Principal {
            user_id: Some(Uuid::from_u128(100)),
            level,
            card_digests: BTreeSet::new(),
            is_active: true,
        }
    }

    /// A door policy: restricted, nothing else in play.
    fn door_policy() -> ResourcePolicy {
        ResourcePolicy {
            locked_out: false,
            unavailable: None,
            default_effect: DefaultEffect::Restricted,
            schedule_id: None,
            training_ok: None,
            metered_ok: None,
        }
    }

    /// A tool policy: open by default, training satisfied.
    fn tool_policy() -> ResourcePolicy {
        ResourcePolicy {
            locked_out: false,
            unavailable: None,
            default_effect: DefaultEffect::Open,
            schedule_id: None,
            training_ok: Some(true),
            metered_ok: None,
        }
    }

    fn decide(p: &Principal, pol: &ResourcePolicy, rules: &[AccessRule], a: Action) -> Decision {
        may(p, pol, rules, &[], tz(), Utc::now(), &graph(), a)
    }

    // --- precedence, one assertion per step, each mutation-checkable -----------

    #[test]
    fn lockout_refuses_even_a_fully_allowed_principal() {
        let mut pol = tool_policy();
        pol.locked_out = true;
        // An admin who would otherwise pass is still refused.
        assert_eq!(
            decide(&principal(5), &pol, &[], Action::Use),
            Decision::Deny(DenyReason::LockedOut)
        );
    }

    #[test]
    fn unavailable_refuses() {
        let mut pol = door_policy();
        pol.unavailable = Some("Door is disabled".into());
        assert_eq!(
            decide(
                &principal(5),
                &pol,
                &[rule("role", "guest", "allow")],
                Action::Unlock
            ),
            Decision::Deny(DenyReason::Unavailable("Door is disabled".into()))
        );
    }

    #[test]
    fn inactive_principal_refused() {
        let mut p = principal(5);
        p.is_active = false;
        assert_eq!(
            decide(&p, &tool_policy(), &[], Action::Use),
            Decision::Deny(DenyReason::Inactive)
        );
    }

    #[test]
    fn deny_rule_beats_an_allow_rule() {
        let rules = [
            rule("role", "guest", "allow"),
            rule("user", &Uuid::from_u128(100).to_string(), "deny"),
        ];
        assert_eq!(
            decide(&principal(5), &door_policy(), &rules, Action::Unlock),
            Decision::Deny(DenyReason::Rule)
        );
    }

    #[test]
    fn restricted_with_no_matching_rule_denies() {
        assert_eq!(
            decide(&principal(5), &door_policy(), &[], Action::Unlock),
            Decision::Deny(DenyReason::NoMatch)
        );
    }

    #[test]
    fn a_role_allow_rule_grants_at_or_above_its_level() {
        let rules = [rule("role", "active", "allow")]; // level 3
        assert!(decide(&principal(3), &door_policy(), &rules, Action::Unlock).is_allow());
        assert!(decide(&principal(5), &door_policy(), &rules, Action::Unlock).is_allow());
        assert_eq!(
            decide(&principal(2), &door_policy(), &rules, Action::Unlock),
            Decision::Deny(DenyReason::NoMatch)
        );
    }

    #[test]
    fn a_card_rule_matches_a_held_digest() {
        let mut p = principal(1);
        p.card_digests.insert("digest-abc".into());
        let rules = [rule("card", "digest-abc", "allow")];
        assert!(decide(&p, &door_policy(), &rules, Action::Unlock).is_allow());
    }

    #[test]
    fn open_tool_grants_when_training_satisfied() {
        assert!(decide(&principal(1), &tool_policy(), &[], Action::Use).is_allow());
    }

    #[test]
    fn open_tool_refuses_when_training_unmet() {
        let mut pol = tool_policy();
        pol.training_ok = Some(false);
        assert_eq!(
            decide(&principal(1), &pol, &[], Action::Use),
            Decision::Deny(DenyReason::Training)
        );
    }

    #[test]
    fn metered_tool_refuses_on_billing() {
        let mut pol = tool_policy();
        pol.metered_ok = Some(false);
        assert_eq!(
            decide(&principal(1), &pol, &[], Action::Use),
            Decision::Deny(DenyReason::Billing)
        );
    }

    // --- the deliberate #101 change: online tool grants honor the schedule -----

    #[test]
    fn open_tool_with_a_closed_schedule_is_refused() {
        // A schedule that is never active (empty intervals) -> Inactive -> the
        // default-open grant is dropped, so the tool refuses. With no schedule it
        // grants; this is the asymmetry #101 closes.
        let sched_id = Uuid::from_u128(500);
        let schedules = [Schedule {
            id: sched_id,
            name: "closed".into(),
            description: None,
            intervals: serde_json::json!([]),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_public: false,
        }];
        let mut pol = tool_policy();
        pol.schedule_id = Some(sched_id);
        assert_eq!(
            may(
                &principal(1),
                &pol,
                &[],
                &schedules,
                tz(),
                Utc::now(),
                &graph(),
                Action::Use
            ),
            Decision::Deny(DenyReason::NoMatch)
        );
        // Same tool, no schedule -> granted.
        pol.schedule_id = None;
        assert!(may(
            &principal(1),
            &pol,
            &[],
            &[],
            tz(),
            Utc::now(),
            &graph(),
            Action::Use
        )
        .is_allow());
    }

    #[test]
    fn a_missing_schedule_fails_closed_for_default_open() {
        // schedule_id references a schedule not in the snapshot -> Unresolvable ->
        // the default-open grant is dropped (fail-closed), matching the door rule
        // fail-closed rule.
        let mut pol = tool_policy();
        pol.schedule_id = Some(Uuid::from_u128(999));
        assert_eq!(
            may(
                &principal(1),
                &pol,
                &[],
                &[],
                tz(),
                Utc::now(),
                &graph(),
                Action::Use
            ),
            Decision::Deny(DenyReason::NoMatch)
        );
    }
}
