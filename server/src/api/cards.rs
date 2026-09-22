//! Admin management of member access cards (#33).
//!
//! Cards are first-class and lifecycle-aware: a member may hold several, each
//! `active` / `disabled` / `released`. Only `active` cards open tools/doors;
//! disabling or releasing revokes a single credential without touching the
//! member. Every action here is audited.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use css_lib::card_crypto::CardCipher;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::ApiError;
use super::responses::ApiResponse;
use crate::auth::AdminUser;
use crate::models::{AuditEventType, CardStatus, NewAuditLog, UserCard};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct IssueCardRequest {
    pub code: String,
}

/// What the admin UI receives for a card. Deliberately not [`UserCard`]: the
/// plaintext column is gone (#108) and the sealed columns are `#[serde(skip)]`,
/// so a card returned raw would carry no code at all. Instead the server
/// decrypts the sealed value and returns only a masked *hint* of it -- enough
/// to tell a member's cards apart, never the full credential, which does not
/// leave the server.
#[derive(Debug, Clone, Serialize)]
pub struct UserCardResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    /// Masked; see [`code_hint`]. Never the full code.
    pub code_hint: String,
    pub status: CardStatus,
    pub last_used_at: Option<DateTime<Utc>>,
    pub issued_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A masked hint of a card code: an ellipsis and up to the last four characters
/// of the plaintext. Enough to distinguish a member's cards in the admin UI
/// without returning the credential; the full code is decrypted on the server
/// for this and never leaves it. A short code reveals only its tail, never more.
fn code_hint(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let start = chars.len().saturating_sub(4);
    let tail: String = chars[start..].iter().collect();
    format!("…{tail}")
}

impl UserCardResponse {
    /// Decrypt the sealed code and build the masked response. Errors if the card
    /// is not sealed (impossible after #108) or the ciphertext will not open,
    /// both of which are real faults rather than something to paper over.
    fn from_card(card: UserCard, cipher: &CardCipher) -> Result<Self, ApiError> {
        let (ct, nonce) = match (card.code_encrypted.as_ref(), card.code_nonce.as_ref()) {
            (Some(ct), Some(nonce)) => (ct, nonce),
            _ => {
                return Err(ApiError::InternalServerError(
                    "card has no sealed code to decrypt".to_string(),
                ))
            }
        };
        let code = cipher.open(ct, nonce).map_err(|_| {
            ApiError::InternalServerError("could not decrypt card code".to_string())
        })?;
        Ok(Self {
            id: card.id,
            user_id: card.user_id,
            code_hint: code_hint(&code),
            status: card.status,
            last_used_at: card.last_used_at,
            issued_at: card.issued_at,
            disabled_at: card.disabled_at,
            released_at: card.released_at,
            disabled_reason: card.disabled_reason,
            created_at: card.created_at,
            updated_at: card.updated_at,
        })
    }
}

/// Map a card to its masked response, or fail loudly if no cipher is present.
/// Startup refuses to run without keys (#108), so `None` is unreachable in a
/// real deployment -- but a 500 beats returning a card with a blank hint.
fn card_response(
    card: UserCard,
    cipher: Option<&CardCipher>,
) -> Result<UserCardResponse, ApiError> {
    let cipher = cipher.ok_or_else(|| {
        ApiError::InternalServerError("card encryption keys are not configured".to_string())
    })?;
    UserCardResponse::from_card(card, cipher)
}

#[derive(Debug, Deserialize)]
pub struct DisableCardRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/user/{user_id}", get(list_user_cards).post(issue_card))
        .route("/{card_id}/disable", post(disable_card))
        .route("/{card_id}/release", post(release_card))
}

/// List every card belonging to a member (any status), newest first.
async fn list_user_cards(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<UserCardResponse>>>, ApiError> {
    let cipher = state.card_cipher.as_deref();
    let out = state
        .db
        .list_user_cards(user_id)?
        .into_iter()
        .map(|card| card_response(card, cipher))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ApiResponse::success(out)))
}

/// Issue a new active card to a member. A code already held by a live
/// (active/disabled) card is rejected with 409 via the unique index.
async fn issue_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(user_id): Path<Uuid>,
    Json(req): Json<IssueCardRequest>,
) -> Result<Json<ApiResponse<UserCardResponse>>, ApiError> {
    let code = req.code.trim();
    if code.is_empty() {
        return Err(ApiError::BadRequest(
            "Card code must not be empty".to_string(),
        ));
    }
    let card = state
        .db
        .create_card(user_id, code, state.card_cipher.as_deref())?;
    audit(&state, AuditEventType::CardIssued, &card, admin.0.id, None);
    Ok(Json(ApiResponse::success(card_response(
        card,
        state.card_cipher.as_deref(),
    )?)))
}

/// Disable a card: revoke access, keep it bound to the member (lost / suspected
/// compromised / member on hold).
async fn disable_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(card_id): Path<Uuid>,
    Json(req): Json<DisableCardRequest>,
) -> Result<Json<ApiResponse<UserCardResponse>>, ApiError> {
    let card = state.db.disable_card(card_id, req.reason.as_deref())?;
    audit(
        &state,
        AuditEventType::CardDisabled,
        &card,
        admin.0.id,
        req.reason.clone(),
    );
    Ok(Json(ApiResponse::success(card_response(
        card,
        state.card_cipher.as_deref(),
    )?)))
}

/// Release a card back to the pool: revoke access; the code may be reissued to
/// a different member later.
async fn release_card(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(card_id): Path<Uuid>,
) -> Result<Json<ApiResponse<UserCardResponse>>, ApiError> {
    let card = state.db.release_card(card_id)?;
    audit(
        &state,
        AuditEventType::CardReleased,
        &card,
        admin.0.id,
        None,
    );
    Ok(Json(ApiResponse::success(card_response(
        card,
        state.card_cipher.as_deref(),
    )?)))
}

/// Best-effort audit write for a card lifecycle action.
fn audit(
    state: &AppState,
    event: AuditEventType,
    card: &UserCard,
    actor: Uuid,
    reason: Option<String>,
) {
    let audit_log = NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: Some(card.user_id),
        actor_id: Some(actor),
        event_data: serde_json::json!({
            "card_id": card.id,
            "card_status": card.status,
            "reason": reason,
        }),
        ip_address: None,
        user_agent: None,
    };
    let _ = state.db.create_audit_log(&audit_log);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_hint_reveals_only_the_last_four_characters() {
        // The mask exists so an admin can tell a member's cards apart without
        // the response carrying the credential. If it ever showed the head,
        // this would fail -- which is the whole point of masking on read.
        assert_eq!(code_hint("CARDS-ACTIVE"), "…TIVE");
        assert_eq!(code_hint("2A-9E-7B-92"), "…B-92");
        let long = code_hint("0123456789ABCDEF");
        assert_eq!(long, "…CDEF");
        assert!(
            !long.contains("0123"),
            "the head of the code leaked into the hint"
        );
    }

    #[test]
    fn code_hint_handles_short_and_empty_codes_without_panicking() {
        // saturating_sub keeps a code shorter than four characters from
        // slicing out of bounds; it reveals only its own tail, never padding.
        assert_eq!(code_hint("AB"), "…AB");
        assert_eq!(code_hint(""), "…");
    }
}
