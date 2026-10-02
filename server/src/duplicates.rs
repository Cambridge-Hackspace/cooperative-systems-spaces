//! Possible duplicate member records (#38, #118).
//!
//! The roster was loaded from two sources -- ToolPass and Stripe -- that knew
//! the same people by different addresses, so one person can be two accounts.
//! The merge (#118) folds them together; this finds the pairs worth looking at.
//! It is a REPORT, not a decision: every pair carries the reasons it was
//! proposed, and an administrator reads them before the merge preview, which
//! has its own warnings. A heuristic that is wrong costs a glance; one that is
//! missing costs a member two logins forever, so the rules lean towards
//! proposing.
//!
//! Pure over a loaded roster so the rules are unit-testable without a
//! database; the one lookup that needs the card cipher (does a profile's card
//! value belong to someone else's card row?) is passed in as a closure.

use std::collections::BTreeMap;

use serde::Serialize;
use uuid::Uuid;

use crate::models::User;
use crate::user_merge::MergeParty;

/// Why two accounts might be one person. Stable strings: the UI shows them and
/// the e2e driver asserts on them.
pub mod reason {
    /// Full names equal after trimming, case-folding and collapsing spaces.
    pub const SAME_NAME: &str = "same_name";
    /// Addresses equal after folding: lowercase, dots removed from the local
    /// part, a `+tag` suffix dropped. `e.klacza@x` and `eklacza@x` is the
    /// case this exists for.
    pub const EMAIL_ALIAS: &str = "email_alias";
    /// One account's profile names a card value that resolves to a card row
    /// held by the other account.
    pub const SHARED_CARD: &str = "shared_card";
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DuplicateCandidate {
    /// The older account first; the merge dialog offers both directions.
    pub a: MergeParty,
    pub b: MergeParty,
    pub reasons: Vec<String>,
}

/// Name as compared: trimmed, lowercased, inner whitespace collapsed.
pub fn fold_name(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Address as compared. Dots in the local part and a `+suffix` are ignored for
/// every domain, not only Gmail's: the question is "could a person have typed
/// both of these for one mailbox", and the cost of a wrong yes is one glance.
pub fn fold_email(email: &str) -> String {
    let lower = email.trim().to_lowercase();
    let Some((local, domain)) = lower.rsplit_once('@') else {
        return lower;
    };
    let local = local.split('+').next().unwrap_or("").replace('.', "");
    format!("{local}@{domain}")
}

/// Find candidate pairs over the roster.
///
/// `emails_of` gives every address an account holds (primary and secondary);
/// `card_owner` resolves a profile card value to the account whose card row
/// it is, if any. `profile_field` is the configured profile key that holds a
/// legacy card value.
pub fn candidates<E, C>(
    users: &[User],
    profile_field: &str,
    emails_of: E,
    card_owner: C,
) -> Vec<DuplicateCandidate>
where
    E: Fn(Uuid) -> Vec<String>,
    C: Fn(&str) -> Option<Uuid>,
{
    let by_id: BTreeMap<Uuid, &User> = users.iter().map(|u| (u.id, u)).collect();
    // (a, b) with a older than b -> reasons, so each pair is reported once.
    let mut pairs: BTreeMap<(Uuid, Uuid), Vec<String>> = BTreeMap::new();
    let mut propose = |x: &User, y: &User, why: &str| {
        if x.id == y.id {
            return;
        }
        let (a, b) = if (x.created_at, x.id) <= (y.created_at, y.id) {
            (x.id, y.id)
        } else {
            (y.id, x.id)
        };
        let reasons = pairs.entry((a, b)).or_default();
        if !reasons.iter().any(|r| r == why) {
            reasons.push(why.to_string());
        }
    };

    // Same name.
    let mut by_name: BTreeMap<String, Vec<&User>> = BTreeMap::new();
    for u in users {
        let folded = fold_name(&u.full_name);
        if !folded.is_empty() {
            by_name.entry(folded).or_default().push(u);
        }
    }
    for group in by_name.values() {
        for (i, x) in group.iter().enumerate() {
            for y in &group[i + 1..] {
                propose(x, y, reason::SAME_NAME);
            }
        }
    }

    // Email alias, across every address each account holds.
    let mut by_email: BTreeMap<String, Vec<&User>> = BTreeMap::new();
    for u in users {
        for addr in emails_of(u.id) {
            by_email.entry(fold_email(&addr)).or_default().push(u);
        }
    }
    for group in by_email.values() {
        for (i, x) in group.iter().enumerate() {
            for y in &group[i + 1..] {
                propose(x, y, reason::EMAIL_ALIAS);
            }
        }
    }

    // A profile card value that is somebody else's card.
    for u in users {
        let values: Vec<String> = match u.profile.get(profile_field) {
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        for v in values {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            if let Some(owner) = card_owner(v) {
                if let Some(other) = by_id.get(&owner) {
                    propose(u, other, reason::SHARED_CARD);
                }
            }
        }
    }

    let mut out: Vec<DuplicateCandidate> = pairs
        .into_iter()
        .filter_map(|((a, b), reasons)| {
            Some(DuplicateCandidate {
                a: MergeParty::from(*by_id.get(&a)?),
                b: MergeParty::from(*by_id.get(&b)?),
                reasons,
            })
        })
        .collect();
    // Strongest evidence first: more reasons, then shared card, then by name.
    out.sort_by(|x, y| {
        y.reasons
            .len()
            .cmp(&x.reasons.len())
            .then_with(|| {
                y.reasons
                    .iter()
                    .any(|r| r == reason::SHARED_CARD)
                    .cmp(&x.reasons.iter().any(|r| r == reason::SHARED_CARD))
            })
            .then_with(|| x.a.full_name.cmp(&y.a.full_name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: u8, name: &str, email: &str, profile: serde_json::Value, day: u32) -> User {
        User {
            id: Uuid::from_u128(id as u128),
            username: format!("u{id}"),
            email: email.to_string(),
            password_hash: String::new(),
            full_name: name.to_string(),
            is_active: true,
            created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, day)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            updated_at: chrono::NaiveDate::from_ymd_opt(2026, 1, day)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            profile,
            meta: serde_json::json!({}),
            mfa_enrolled_at: None,
            email_verified_at: None,
            mailing_list_opt_out_at: None,
            membership_next_due_at: None,
            token_version: 0,
        }
    }

    fn primary_only(users: &[User]) -> impl Fn(Uuid) -> Vec<String> + '_ {
        move |id| {
            users
                .iter()
                .filter(|u| u.id == id)
                .map(|u| u.email.clone())
                .collect()
        }
    }

    #[test]
    fn folding_names_and_emails() {
        assert_eq!(fold_name("  Ada   LOVELACE "), "ada lovelace");
        assert_eq!(fold_email("E.Klacza+css@Gmail.com"), "eklacza@gmail.com");
        assert_eq!(fold_email("plain@x.org"), "plain@x.org");
        assert_eq!(fold_email("not-an-address"), "not-an-address");
    }

    #[test]
    fn same_name_and_alias_are_each_a_reason_and_the_pair_is_reported_once() {
        let users = vec![
            user(
                1,
                "Ed Klacza",
                "e.klacza@gmail.com",
                serde_json::json!({}),
                1,
            ),
            user(
                2,
                "ed klacza",
                "eklacza@gmail.com",
                serde_json::json!({}),
                2,
            ),
            user(3, "Someone Else", "else@x.org", serde_json::json!({}), 3),
        ];
        let out = candidates(&users, "card_id", primary_only(&users), |_| None);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].a.id, Uuid::from_u128(1), "older account first");
        assert_eq!(out[0].reasons, vec![reason::SAME_NAME, reason::EMAIL_ALIAS]);
    }

    #[test]
    fn a_profile_card_that_is_someone_elses_row_is_a_reason() {
        let users = vec![
            user(
                1,
                "A Person",
                "a@x.org",
                serde_json::json!({ "card_id": "CARD1" }),
                1,
            ),
            user(2, "Different Name", "b@x.org", serde_json::json!({}), 2),
        ];
        let owner = |code: &str| (code == "CARD1").then_some(Uuid::from_u128(2));
        let out = candidates(&users, "card_id", primary_only(&users), owner);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].reasons, vec![reason::SHARED_CARD]);
    }

    #[test]
    fn an_account_is_never_paired_with_itself() {
        // Its own card resolving to itself, and its own name matching itself,
        // must not produce a pair.
        let users = vec![user(
            1,
            "Solo",
            "solo@x.org",
            serde_json::json!({ "card_id": "MINE" }),
            1,
        )];
        let out = candidates(&users, "card_id", primary_only(&users), |_| {
            Some(Uuid::from_u128(1))
        });
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn secondary_addresses_count() {
        let users = vec![
            user(1, "One", "one@x.org", serde_json::json!({}), 1),
            user(2, "Two", "two@x.org", serde_json::json!({}), 2),
        ];
        let emails = |id: Uuid| {
            if id == Uuid::from_u128(1) {
                vec!["one@x.org".to_string(), "t.w.o@x.org".to_string()]
            } else {
                vec!["two@x.org".to_string()]
            }
        };
        let out = candidates(&users, "card_id", emails, |_| None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].reasons, vec![reason::EMAIL_ALIAS]);
    }

    #[test]
    fn stronger_evidence_sorts_first() {
        let users = vec![
            user(1, "Pat", "pat@x.org", serde_json::json!({}), 1),
            user(2, "Pat", "pat2@x.org", serde_json::json!({}), 2),
            user(3, "Sam", "s.am@x.org", serde_json::json!({}), 3),
            user(4, "Sam", "sam@x.org", serde_json::json!({}), 4),
        ];
        let out = candidates(&users, "card_id", primary_only(&users), |_| None);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].a.full_name, "Sam", "two reasons beat one");
        assert_eq!(out[0].reasons.len(), 2);
    }
}
