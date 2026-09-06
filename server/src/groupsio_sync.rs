//! Groups.io mailing-list sync.
//!
//! Two mechanisms keep the Groups.io group in step with platform membership:
//!
//! * **Event push** -- a second consumer of the audit-event fan-out (alongside
//!   the webhook dispatcher). A member becoming intended-subscribed
//!   (verified/activated/opted-in) is added; becoming un-intended
//!   (deactivated/deleted/opted-out) is removed; an email change moves the
//!   address. Low latency, but best-effort: a missed or failed event is swept
//!   up by...
//! * **Reconciliation** -- a periodic full diff of the intended roster against
//!   the group. It adds the missing, removes strangers (the platform owns the
//!   whole list, minus a `protected_addresses` allowlist), and detects members
//!   who left via a Groups.io email link (they surface in past-members) to
//!   record a local opt-out so they are never re-added.
//!
//! The diff (`reconcile_plan`) and the "who belongs" predicate
//! (`intended_subscribed`) are pure and unit-tested; they are also the oracle
//! the `contracts/groupsio_sync.json` vectors pin. Everything case-insensitive:
//! addresses are not normalized at storage, so a mixed-case address must not
//! churn add/remove each cycle.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::config::ConfigManager;
use crate::database::DatabaseManager;
use crate::groupsio::GroupsioClient;
use crate::models::{AuditEventType, AuditLog, NewAuditLog, NewGroupsioSyncRun};

/// Normalize an address for comparison: trimmed and lowercased.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Whether a member should be on the mailing list. Opt-out is the default-off
/// gate; verification is the "don't push an unconfirmed address" gate.
pub fn intended_subscribed(is_active: bool, email_verified: bool, opted_out: bool) -> bool {
    is_active && email_verified && !opted_out
}

/// What a reconciliation pass should do to make Groups.io mirror intent.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReconcilePlan {
    /// Intended members not on the group and not known to have left: add them.
    pub to_add: Vec<String>,
    /// On the group, not intended, not protected: remove them (own the list).
    pub to_remove: Vec<String>,
    /// Intended, absent from the group, and present in past-members: they left
    /// outside the platform (an email-link unsubscribe). Record an opt-out and
    /// do not re-add.
    pub to_opt_out: Vec<String>,
}

/// Compute the reconciliation plan. All inputs are compared case-insensitively.
///
/// `to_add` deliberately excludes anyone in `past`: re-adding a member Groups.io
/// shows as departed would fight an email-link unsubscribe. Those land in
/// `to_opt_out` instead. `to_remove` excludes `protected` so "own the list"
/// cannot evict the group's own owner/moderators.
pub fn reconcile_plan(
    intended: &[String],
    current: &[String],
    past: &[String],
    protected: &[String],
) -> ReconcilePlan {
    let intended: BTreeSet<String> = intended.iter().map(|e| normalize_email(e)).collect();
    let current: BTreeSet<String> = current.iter().map(|e| normalize_email(e)).collect();
    let past: BTreeSet<String> = past.iter().map(|e| normalize_email(e)).collect();
    let protected: BTreeSet<String> = protected.iter().map(|e| normalize_email(e)).collect();

    let to_add: Vec<String> = intended
        .iter()
        .filter(|e| !current.contains(*e) && !past.contains(*e))
        .cloned()
        .collect();

    let to_remove: Vec<String> = current
        .iter()
        .filter(|e| !intended.contains(*e) && !protected.contains(*e))
        .cloned()
        .collect();

    let to_opt_out: Vec<String> = intended
        .iter()
        .filter(|e| past.contains(*e) && !current.contains(*e))
        .cloned()
        .collect();

    ReconcilePlan {
        to_add,
        to_remove,
        to_opt_out,
    }
}

/// The result of one reconciliation pass, for the admin status view and the run
/// log.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ReconcileOutcome {
    pub added: usize,
    pub removed: usize,
    pub opted_out: usize,
    pub ok: bool,
    pub error: Option<String>,
}

/// The mailing-list sync service: event consumer plus reconciliation.
#[derive(Clone)]
pub struct GroupsIoService {
    db: Arc<DatabaseManager>,
    config: Arc<ConfigManager>,
    client: GroupsioClient,
}

impl GroupsIoService {
    /// Build the service and spawn its event consumer, returning the sender to
    /// register on the [`DatabaseManager`] (mirrors `WebhookDispatcher::start`).
    pub fn start(
        db: Arc<DatabaseManager>,
        config: Arc<ConfigManager>,
        client: GroupsioClient,
    ) -> (Arc<Self>, mpsc::UnboundedSender<AuditLog>) {
        let svc = Arc::new(Self { db, config, client });
        let (tx, mut rx) = mpsc::unbounded_channel::<AuditLog>();

        let consumer = svc.clone();
        tokio::spawn(async move {
            // Sequential on purpose: events for one member (subscribe then
            // unsubscribe) must apply in order. Volume is low.
            while let Some(event) = rx.recv().await {
                consumer.handle_event(event).await;
            }
            debug!("Groups.io event channel closed; consumer stopping");
        });

        (svc, tx)
    }

    /// Whether the module is currently enabled (checked live).
    fn enabled(&self) -> bool {
        self.config.get_config().groupsio.enabled
    }

    /// Translate one audit event into a Groups.io add/remove. A no-op unless the
    /// module is enabled and the event is one we act on.
    async fn handle_event(&self, event: AuditLog) {
        if !self.enabled() {
            return;
        }
        let Some(kind) = crate::models::AuditEventType::all()
            .iter()
            .find(|k| k.as_str() == event.event_type.as_str())
            .cloned()
        else {
            return;
        };

        match kind {
            // Became (or may have become) intended.
            AuditEventType::MailingListSubscribe
            | AuditEventType::EmailVerified
            | AuditEventType::UserActivation => {
                if let Some(uid) = event.user_id {
                    self.add_if_intended(uid).await;
                }
            }
            // Platform-initiated removal: the member may still be on the list.
            AuditEventType::MailingListUnsubscribe | AuditEventType::UserDeactivation => {
                if let Some(uid) = event.user_id {
                    if let Some(email) = self.email_of(uid) {
                        self.remove(&[email]).await;
                    }
                }
            }
            // The account is gone; its address is only in the event payload.
            AuditEventType::UserDeletion => {
                if let Some(email) = event
                    .event_data
                    .get("deleted_email")
                    .and_then(|v| v.as_str())
                {
                    self.remove(&[email.to_string()]).await;
                }
            }
            // Move the address: drop the old, add the new if still intended.
            AuditEventType::UserEmailChange => {
                if let Some(old) = event.event_data.get("old_email").and_then(|v| v.as_str()) {
                    self.remove(&[old.to_string()]).await;
                }
                if let Some(uid) = event.user_id {
                    self.add_if_intended(uid).await;
                }
            }
            _ => {}
        }
    }

    /// Current email for a user, if they still exist.
    fn email_of(&self, uid: uuid::Uuid) -> Option<String> {
        match self.db.find_user_by_id(uid) {
            Ok(Some(u)) => Some(u.email),
            Ok(None) => None,
            Err(e) => {
                warn!("Groups.io sync: failed to load user {uid}: {e}");
                None
            }
        }
    }

    /// Add a user to the group if they are currently intended-subscribed.
    async fn add_if_intended(&self, uid: uuid::Uuid) {
        let user = match self.db.find_user_by_id(uid) {
            Ok(Some(u)) => u,
            Ok(None) => return,
            Err(e) => {
                warn!("Groups.io sync: failed to load user {uid}: {e}");
                return;
            }
        };
        let intended = intended_subscribed(
            user.is_active,
            user.email_verified_at.is_some(),
            user.mailing_list_opt_out_at.is_some(),
        );
        if !intended {
            return;
        }
        if let Err(e) = self.client.direct_add(&[user.email.clone()]).await {
            warn!("Groups.io sync: failed to add {}: {e}", user.email);
        } else {
            self.record(AuditEventType::MailingListSyncAdd, Some(uid), &[user.email]);
        }
    }

    /// Remove one or more addresses from the group.
    async fn remove(&self, emails: &[String]) {
        if let Err(e) = self.client.remove_members(emails).await {
            warn!("Groups.io sync: failed to remove {:?}: {e}", emails);
        } else {
            self.record(AuditEventType::MailingListSyncRemove, None, emails);
        }
    }

    /// Run one full reconciliation pass and record it. Safe to call from the
    /// ticker or the admin "reconcile now" endpoint.
    pub async fn reconcile_once(&self) -> ReconcileOutcome {
        let started_at = chrono::Utc::now();
        let outcome = self.reconcile_apply().await;
        let run = NewGroupsioSyncRun {
            started_at,
            finished_at: chrono::Utc::now(),
            added: outcome.added as i32,
            removed: outcome.removed as i32,
            opted_out: outcome.opted_out as i32,
            ok: outcome.ok,
            error: outcome.error.clone(),
        };
        if let Err(e) = self.db.record_groupsio_sync_run(&run) {
            warn!("Failed to record Groups.io sync run: {e}");
        }
        outcome
    }

    /// The reconciliation itself, without the run bookkeeping.
    async fn reconcile_apply(&self) -> ReconcileOutcome {
        let mut outcome = ReconcileOutcome::default();
        if !self.enabled() {
            outcome.error = Some("Groups.io integration is disabled".to_string());
            return outcome;
        }

        let intended = match self.db.list_mailing_list_intended() {
            Ok(v) => v,
            Err(e) => {
                outcome.error = Some(format!("load intended roster: {e}"));
                return outcome;
            }
        };
        let current = match self.client.get_members().await {
            Ok(v) => v.into_iter().map(|m| m.email).collect::<Vec<_>>(),
            Err(e) => {
                outcome.error = Some(format!("get_members: {e}"));
                return outcome;
            }
        };
        let past = match self.client.get_past_members().await {
            Ok(v) => v.into_iter().map(|m| m.email).collect::<Vec<_>>(),
            Err(e) => {
                outcome.error = Some(format!("get_past_members: {e}"));
                return outcome;
            }
        };
        let protected = self.config.get_config().groupsio.protected_addresses;

        let plan = reconcile_plan(&intended, &current, &past, &protected);

        if !plan.to_add.is_empty() {
            match self.client.direct_add(&plan.to_add).await {
                Ok(()) => {
                    outcome.added = plan.to_add.len();
                    self.record(AuditEventType::MailingListSyncAdd, None, &plan.to_add);
                }
                Err(e) => {
                    outcome.error = Some(format!("direct_add: {e}"));
                    return outcome;
                }
            }
        }

        if !plan.to_remove.is_empty() {
            match self.client.remove_members(&plan.to_remove).await {
                Ok(()) => {
                    outcome.removed = plan.to_remove.len();
                    self.record(AuditEventType::MailingListSyncRemove, None, &plan.to_remove);
                }
                Err(e) => {
                    outcome.error = Some(format!("remove_members: {e}"));
                    return outcome;
                }
            }
        }

        // Members who left via an email link: record the opt-out locally so they
        // are never re-added. No Groups.io call -- they are already gone -- and
        // no member-intent event, which would try to re-remove them; a
        // sync-remove audit row with a reason is the record.
        for email in &plan.to_opt_out {
            match self.db.find_user_by_email(email) {
                Ok(Some(u)) => {
                    if let Err(e) = self
                        .db
                        .set_mailing_list_opt_out(u.id, Some(chrono::Utc::now()))
                    {
                        warn!("Groups.io sync: failed to record opt-out for {email}: {e}");
                        continue;
                    }
                    outcome.opted_out += 1;
                    self.record_reason(
                        AuditEventType::MailingListSyncRemove,
                        Some(u.id),
                        email,
                        "external_unsubscribe",
                    );
                }
                Ok(None) => {}
                Err(e) => warn!("Groups.io sync: failed to look up {email}: {e}"),
            }
        }

        outcome.ok = true;
        outcome
    }

    /// Write a sync audit row listing the affected addresses. Best-effort: an
    /// audit failure never fails the sync (mirrors every other audit write).
    fn record(&self, event: AuditEventType, user_id: Option<uuid::Uuid>, emails: &[String]) {
        let log = NewAuditLog {
            event_type: event.as_str().to_string(),
            user_id,
            actor_id: None,
            event_data: serde_json::json!({ "emails": emails }),
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&log) {
            error!(
                "Failed to write Groups.io sync audit {}: {e}",
                event.as_str()
            );
        }
    }

    /// As `record`, for a single address carrying a reason.
    fn record_reason(
        &self,
        event: AuditEventType,
        user_id: Option<uuid::Uuid>,
        email: &str,
        reason: &str,
    ) {
        let log = NewAuditLog {
            event_type: event.as_str().to_string(),
            user_id,
            actor_id: None,
            event_data: serde_json::json!({ "email": email, "reason": reason }),
            ip_address: None,
            user_agent: None,
        };
        if let Err(e) = self.db.create_audit_log(&log) {
            error!(
                "Failed to write Groups.io sync audit {}: {e}",
                event.as_str()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn intended_requires_active_verified_and_not_opted_out() {
        assert!(intended_subscribed(true, true, false));
        assert!(!intended_subscribed(false, true, false)); // inactive
        assert!(!intended_subscribed(true, false, false)); // unverified
        assert!(!intended_subscribed(true, true, true)); // opted out
    }

    #[test]
    fn reconcile_adds_the_missing() {
        let plan = reconcile_plan(&v(&["a@x.org", "b@x.org"]), &v(&["a@x.org"]), &[], &[]);
        assert_eq!(plan.to_add, v(&["b@x.org"]));
        assert!(plan.to_remove.is_empty());
        assert!(plan.to_opt_out.is_empty());
    }

    #[test]
    fn reconcile_removes_the_stranger() {
        let plan = reconcile_plan(
            &v(&["a@x.org"]),
            &v(&["a@x.org", "stranger@x.org"]),
            &[],
            &[],
        );
        assert_eq!(plan.to_remove, v(&["stranger@x.org"]));
        assert!(plan.to_add.is_empty());
    }

    #[test]
    fn reconcile_never_removes_a_protected_address() {
        // The group owner is on the list, is not a platform member, and must
        // survive "the platform owns the list".
        let plan = reconcile_plan(
            &v(&["a@x.org"]),
            &v(&["a@x.org", "owner@x.org"]),
            &[],
            &v(&["owner@x.org"]),
        );
        assert!(plan.to_remove.is_empty());
    }

    #[test]
    fn a_member_in_past_and_not_current_is_opted_out_not_re_added() {
        // Intended, gone from the group, present in past-members: they clicked
        // unsubscribe in an email. Record the opt-out; do NOT re-add.
        let plan = reconcile_plan(&v(&["gone@x.org"]), &[], &v(&["gone@x.org"]), &[]);
        assert_eq!(plan.to_opt_out, v(&["gone@x.org"]));
        assert!(
            plan.to_add.is_empty(),
            "must not fight an email-link opt-out"
        );
    }

    #[test]
    fn a_past_member_who_is_back_on_the_list_is_left_alone() {
        // Present in both past and current (they rejoined): current wins, no
        // opt-out, no add.
        let plan = reconcile_plan(
            &v(&["back@x.org"]),
            &v(&["back@x.org"]),
            &v(&["back@x.org"]),
            &[],
        );
        assert!(plan.to_opt_out.is_empty());
        assert!(plan.to_add.is_empty());
        assert!(plan.to_remove.is_empty());
    }

    #[test]
    fn matching_is_case_insensitive() {
        // Same address in different case must not churn add/remove.
        let plan = reconcile_plan(&v(&["Alice@X.org"]), &v(&["alice@x.org"]), &[], &[]);
        assert!(
            plan.to_add.is_empty(),
            "cased duplicate must not be re-added"
        );
        assert!(
            plan.to_remove.is_empty(),
            "cased duplicate must not be removed"
        );
    }
}
