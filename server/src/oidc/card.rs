//! A card tap at a reader signs the member in to one of the space's own apps
//! (#241): POP, the print tower, first.
//!
//! On a yes for a resource with a `sign_in_app`, the scan reply carries a
//! [`LocalSignIn`]: a one-time code the app redeems at `token` with
//! `grant_type=urn:css:card-sign-in` and its own credentials, or, when the
//! resource's second factor applies to the member, a ticket the app sends
//! back with the member's TOTP code to get one. The code is not an
//! authorization code: that grant requires PKCE, which a card has no browser
//! to hold, and stays as it is.
//!
//! What keeps it safe, each in one place below:
//! - only an app marked as the space's own, with a secret, and not disabled,
//!   is ever given one ([`app_for`]);
//! - a code is good once, for that app, for as long as an authorization code
//!   (`[oidc] code_ttl_secs`, sixty seconds by default); a ticket for two
//!   minutes and three tries; a member for ten tries an hour across tickets;
//! - a member the resource requires a second factor of, who has no TOTP, is
//!   refused in words, never let through without it.

use chrono::{Duration, Utc};

use crate::api::errors::ApiError;
use crate::models::{CardSignIn, NewCardSignIn, OauthClient, Resource, User};
use crate::oidc::flow;
use crate::AppState;

pub use css_lib::wire::LocalSignIn;

/// How long a ticket is good for.
pub const TICKET_SECS: i64 = 120;
/// Tries at one ticket.
pub const TICKET_TRIES: i32 = 3;
/// Tries at second factors one member may spend in an hour, across tickets.
pub const MEMBER_TRIES_PER_HOUR: i64 = 10;
/// The grant a card sign-in's code is redeemed with.
pub const GRANT: &str = "urn:css:card-sign-in";

/// How long a card sign-in's code is good for: as long as an authorization
/// code.
fn code_secs(state: &AppState) -> i64 {
    state.config_manager.get_config().oidc.code_ttl_secs
}

/// Who a resource's second factor applies to: `off`, `admins`, `everyone`.
/// Anything else applies to everyone: a setting POP cannot read fails closed.
pub fn applies(setting: &str, is_admin: bool) -> bool {
    match setting {
        "off" => false,
        "admins" => is_admin,
        _ => true,
    }
}

/// The app a resource signs in to, if it may be given card sign-ins: the
/// space's own (skips consent, as the member is not asked at a reader),
/// confidential (it redeems with a secret), and not disabled.
pub fn app_for(client: Option<OauthClient>) -> Option<OauthClient> {
    client.filter(|c| c.skip_consent && c.confidential && !c.disabled)
}

/// The sign-in a yes on `resource` by `user` carries, if the resource signs
/// in to an app. Called only after the resource has said yes.
pub fn begin(
    state: &AppState,
    resource: &Resource,
    user: &User,
) -> Result<Option<LocalSignIn>, ApiError> {
    let Some(app_id) = resource.sign_in_app else {
        return Ok(None);
    };
    let Some(app) = app_for(state.db.oauth_get_client(app_id)?) else {
        tracing::warn!(
            "resource {} signs in to an app that is gone, disabled, has no secret, or is not the space's own: no sign-in",
            resource.id
        );
        return Ok(Some(LocalSignIn::Refused {
            reason: "This reader can't sign anyone in just now: ask an administrator".into(),
        }));
    };
    let (roles, _) = state.db.user_roles_and_permissions(user.id)?;
    let is_admin = roles.iter().any(|r| r == "admin");
    let now = Utc::now();
    if applies(&resource.second_factor, is_admin) {
        let has_totp = state
            .db
            .list_user_totp(user.id)?
            .iter()
            .any(|t| t.confirmed_at.is_some());
        if !has_totp {
            return Ok(Some(LocalSignIn::Refused {
                reason: "Signing in here needs an authenticator app: set one up in CS:S, under your profile's security"
                    .into(),
            }));
        }
        let ticket = flow::new_secret();
        state.db.card_sign_in_start(&NewCardSignIn {
            resource_id: resource.id,
            client_id: app.id,
            user_id: user.id,
            ticket_hash: Some(flow::hash_secret(&ticket)),
            code_hash: None,
            amr: "hwk".into(),
            expires_at: now + Duration::seconds(TICKET_SECS),
        })?;
        return Ok(Some(LocalSignIn::Totp { ticket }));
    }
    let code = flow::new_secret();
    state.db.card_sign_in_start(&NewCardSignIn {
        resource_id: resource.id,
        client_id: app.id,
        user_id: user.id,
        ticket_hash: None,
        code_hash: Some(flow::hash_secret(&code)),
        amr: "hwk".into(),
        expires_at: now + Duration::seconds(code_secs(state)),
    })?;
    Ok(Some(LocalSignIn::Code { code }))
}

/// Why a second factor was not taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The ticket is unknown, another app's, spent, expired or out of tries.
    Ticket,
    /// The member has spent their tries for the hour.
    TooMany,
    /// The code was wrong (the try is spent): whose it was, for the record.
    WrongCode(uuid::Uuid),
}

/// The second factor for `ticket`, given by `app`: a code to redeem, or why
/// not. A try is counted before the TOTP is checked.
pub fn second_factor(
    state: &AppState,
    app: &OauthClient,
    ticket: &str,
    totp: &str,
) -> Result<Result<String, Refused>, ApiError> {
    let Some(row): Option<CardSignIn> =
        state
            .db
            .card_sign_in_try(&flow::hash_secret(ticket), app.id, TICKET_TRIES)?
    else {
        return Ok(Err(Refused::Ticket));
    };
    let since = Utc::now() - Duration::hours(1);
    if state.db.card_sign_in_tries_since(row.user_id, since)? > MEMBER_TRIES_PER_HOUR {
        return Ok(Err(Refused::TooMany));
    }
    let user = state
        .db
        .find_user_by_id(row.user_id)?
        .filter(|u| u.is_active)
        .ok_or(ApiError::Unauthorized("member not active".into()))?;
    if crate::api::mfa::verify_totp_path(state, &user, Some(totp)).is_err() {
        return Ok(Err(Refused::WrongCode(user.id)));
    }
    let code = flow::new_secret();
    state.db.card_sign_in_issue(
        row.id,
        &flow::hash_secret(&code),
        "hwk otp",
        Utc::now() + Duration::seconds(code_secs(state)),
    )?;
    Ok(Ok(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_factor_applies_as_set_and_fails_closed() {
        assert!(!applies("off", true));
        assert!(!applies("off", false));
        assert!(applies("admins", true));
        assert!(!applies("admins", false));
        assert!(applies("everyone", false));
        assert!(applies("everyone", true));
        // A value nobody set: everyone, never no one.
        assert!(applies("", false));
        assert!(applies("Admins", false));
    }
}
