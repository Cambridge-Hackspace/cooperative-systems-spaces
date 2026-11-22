//! The notification relay (#226): a registered app messages a member, and CS:S
//! delivers it however that member has chosen.
//!
//! For apps, with an app token from the client-credentials grant that carries
//! `notify:members` (#225):
//! - `PUT  /api/notify/kinds`              declare the kinds of message it sends
//! - `GET  /api/notify/kinds`
//! - `POST /api/notify/members/{sub}`      send one message to one member
//!
//! For members, with their CS:S session:
//! - `GET  /api/notify/preferences`        every app's kinds, and their choices
//! - `PUT  /api/notify/preferences/{app}`  change them
//!
//! **What an app learns.** Only that a message was accepted, and its id. Not
//! the address, not the channel, and not whether the member chose to get it:
//! the answer is byte-for-byte the same either way. CS:S keeps no copy of the
//! text, only that a message of that kind was sent and what became of it.

use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::responses::ApiResponse;
use crate::auth::AuthUser;
use crate::models::{
    AuditEventType, NewAuditLog, NewNotificationMessage, NotificationKind, OauthClient,
};
use crate::oidc::{self, relay};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/kinds", get(list_kinds).put(declare_kinds))
        .route("/members/{sub}", post(notify_member))
        .route("/preferences", get(my_preferences))
        .route("/preferences/{client}", put(set_preferences))
}

const SCOPE: &str = "notify:members";

fn disabled() -> ApiError {
    ApiError::NotFound("The OpenID Connect provider is not enabled".to_string())
}

fn audit(state: &AppState, event: AuditEventType, user: Option<Uuid>, data: Value) {
    let _ = state.db.create_audit_log(&NewAuditLog {
        event_type: event.as_str().to_string(),
        user_id: user,
        actor_id: None,
        event_data: data,
        ip_address: None,
        user_agent: None,
    });
}

/// The calling app, as an extractor so it runs before the body is parsed.
///
/// As a plain call inside the handler it ran after `Json`, so a request with
/// no credential and a body the handler could not read was answered 422, not
/// 401: it told an unauthenticated caller about the body's shape before saying
/// it was not allowed in. `contract_matrix.rs`'s
/// no_guarded_route_answers_anything_but_401_without_a_credential caught it.
pub struct RelayApp(pub OauthClient);

impl FromRequestParts<AppState> for RelayApp {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        crate::api::oidc::app_from_token(state, &parts.headers, SCOPE).map(RelayApp)
    }
}

// ---------------------------------------------------------------------------
// Kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct KindIn {
    kind: String,
    label: String,
}

#[derive(Debug, Deserialize)]
struct DeclareKinds {
    kinds: Vec<KindIn>,
}

#[derive(Debug, Serialize)]
struct KindOut {
    kind: String,
    label: String,
}

impl From<NotificationKind> for KindOut {
    fn from(k: NotificationKind) -> Self {
        KindOut {
            kind: k.kind,
            label: k.label,
        }
    }
}

/// `PUT /api/notify/kinds` -- the app's whole set, replacing what it declared
/// before. A kind it stops declaring takes members' choices about it with it.
async fn declare_kinds(
    State(state): State<AppState>,
    RelayApp(app): RelayApp,
    Json(req): Json<DeclareKinds>,
) -> Result<Json<ApiResponse<Vec<KindOut>>>, ApiError> {
    if req.kinds.len() > 50 {
        return Err(ApiError::BadRequest("at most 50 kinds".to_string()));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut kinds = Vec::with_capacity(req.kinds.len());
    for k in req.kinds {
        if !relay::kind_is_well_formed(&k.kind) {
            return Err(ApiError::BadRequest(format!(
                "kind {:?} must be lowercase letters, digits and . _ -, starting with a letter",
                k.kind
            )));
        }
        relay::check_label(&k.label).map_err(ApiError::BadRequest)?;
        if !seen.insert(k.kind.clone()) {
            return Err(ApiError::BadRequest(format!(
                "kind {:?} is declared twice",
                k.kind
            )));
        }
        kinds.push((k.kind, k.label.trim().to_string()));
    }
    let stored = state.db.notify_declare_kinds(app.id, &kinds)?;
    audit(
        &state,
        AuditEventType::NotificationKindsDeclared,
        None,
        json!({ "client_id": app.client_id, "app": app.name,
                "kinds": stored.iter().map(|k| k.kind.clone()).collect::<Vec<_>>() }),
    );
    Ok(Json(ApiResponse::success(
        stored.into_iter().map(KindOut::from).collect(),
    )))
}

/// `GET /api/notify/kinds`
async fn list_kinds(
    State(state): State<AppState>,
    RelayApp(app): RelayApp,
) -> Result<Json<ApiResponse<Vec<KindOut>>>, ApiError> {
    let kinds = state
        .db
        .notify_all_kinds()?
        .into_iter()
        .filter(|k| k.client_id == app.id)
        .map(KindOut::from)
        .collect();
    Ok(Json(ApiResponse::success(kinds)))
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MessageIn {
    kind: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    urgency: Option<String>,
    #[serde(default)]
    dedupe_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct Accepted {
    id: Uuid,
    accepted: bool,
}

/// `POST /api/notify/members/{sub}` -- one message to one member. Answers 202
/// with the message's id, and the same answer whether the member gets it or
/// not.
async fn notify_member(
    State(state): State<AppState>,
    RelayApp(app): RelayApp,
    Path(sub): Path<String>,
    Json(msg): Json<MessageIn>,
) -> Result<(StatusCode, Json<ApiResponse<Accepted>>), ApiError> {
    let urgency = relay::Urgency::parse(msg.urgency.as_deref())
        .ok_or_else(|| ApiError::BadRequest("urgency must be normal or high".to_string()))?;
    relay::check_message(&msg.title, &msg.body, msg.link.as_deref())
        .map_err(ApiError::BadRequest)?;
    if msg
        .dedupe_key
        .as_deref()
        .is_some_and(|k| k.is_empty() || k.len() > 200)
    {
        return Err(ApiError::BadRequest(
            "dedupe_key must be 1 to 200 characters".to_string(),
        ));
    }
    let Some(kind) = state.db.notify_kind(app.id, &msg.kind)? else {
        return Err(ApiError::BadRequest(format!(
            "kind {:?} isn't declared by this app; declare it with PUT /api/notify/kinds",
            msg.kind
        )));
    };
    // The member, by the `sub` the app got from a sign-in. An app can only
    // message someone who exists; it learns nothing it didn't already have.
    let user = Uuid::parse_str(&sub)
        .ok()
        .and_then(|id| state.db.find_user_by_id(id).transpose())
        .transpose()?
        .filter(|u| u.is_active)
        .ok_or_else(|| ApiError::NotFound("No active member with that sub".to_string()))?;

    // A repeat of a dedupe key the app has used is answered as the first one
    // was, before the rate limit: retrying is not sending more.
    if let Some(key) = msg.dedupe_key.as_deref() {
        if let Some(first) = state.db.notify_by_dedupe(app.id, key)? {
            return Ok((
                StatusCode::ACCEPTED,
                Json(ApiResponse::success(Accepted {
                    id: first.id,
                    accepted: true,
                })),
            ));
        }
    }
    let config = state.config_manager.get_config();
    let hour_ago = Utc::now() - Duration::hours(1);
    if state
        .db
        .notify_count_since(app.id, Some(user.id), hour_ago)?
        >= config.oidc.notify_per_member_per_hour
    {
        return Err(ApiError::TooManyRequests(
            "This app has sent this member as many messages as an hour allows".to_string(),
        ));
    }
    if state.db.notify_count_since(app.id, None, hour_ago)? >= config.oidc.notify_per_app_per_hour {
        return Err(ApiError::TooManyRequests(
            "This app has sent as many messages as an hour allows".to_string(),
        ));
    }

    let prefs = state.db.notify_preferences(user.id)?;
    let choice = |k: &str| {
        prefs
            .iter()
            .find(|p| p.client_id == app.id && p.kind == k)
            .map(|p| p.email)
    };
    let address = user.email_verified_at.is_some().then(|| user.email.clone());
    let wanted = relay::email_wanted(choice("*"), choice(&kind.kind), urgency) && address.is_some();

    let (row, fresh) = state.db.notify_record(&NewNotificationMessage {
        client_id: app.id,
        user_id: user.id,
        kind: kind.kind.clone(),
        urgency: urgency.as_str().to_string(),
        dedupe_key: msg.dedupe_key.clone(),
        outcome: if wanted { "queued" } else { "suppressed" }.to_string(),
    })?;
    if fresh {
        let event = json!({
            "client_id": app.client_id, "app": app.name, "kind": kind.kind,
            "urgency": urgency.as_str(), "message_id": row.id,
        });
        if wanted {
            let to = address.unwrap_or_default();
            let prefs_url = format!(
                "{}/profile/notifications",
                config.site.site_url.trim_end_matches('/')
            );
            let (subject, text) = relay::compose_email(
                &app.name,
                &msg.title,
                &msg.body,
                msg.link.as_deref(),
                &prefs_url,
            );
            let state2 = state.clone();
            let user_id = user.id;
            let message_id = row.id;
            tokio::spawn(async move {
                let outcome = match state2.mail_service.send(&to, &subject, &text).await {
                    Ok(()) => "delivered",
                    Err(e) => {
                        tracing::warn!("relay message {message_id} was not delivered: {e}");
                        "failed"
                    }
                };
                let _ = state2.db.notify_set_outcome(message_id, outcome);
                let mut event = event;
                event["outcome"] = json!(outcome);
                audit(
                    &state2,
                    AuditEventType::MemberNotified,
                    Some(user_id),
                    event,
                );
            });
        } else {
            let mut event = event;
            event["outcome"] = json!("suppressed");
            audit(&state, AuditEventType::MemberNotified, Some(user.id), event);
        }
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiResponse::success(Accepted {
            id: row.id,
            accepted: true,
        })),
    ))
}

// ---------------------------------------------------------------------------
// A member's choices
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct KindChoice {
    kind: String,
    label: String,
    /// Whether it reaches them by email.
    email: bool,
    /// True when they haven't chosen, and this is the default.
    default: bool,
}

#[derive(Debug, Serialize)]
struct AppChoices {
    client: Uuid,
    app: String,
    /// The whole app: off means nothing from it reaches them.
    enabled: bool,
    kinds: Vec<KindChoice>,
}

/// `GET /api/notify/preferences` -- every app that has declared kinds, with
/// the member's choices. With no choice made, a kind shows as off: only a
/// message the app marks high-urgency is sent before the member opts in.
async fn my_preferences(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<ApiResponse<Vec<AppChoices>>>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    let prefs = state.db.notify_preferences(user.0.id)?;
    let mut by_app: HashMap<Uuid, Vec<NotificationKind>> = HashMap::new();
    for k in state.db.notify_all_kinds()? {
        by_app.entry(k.client_id).or_default().push(k);
    }
    let mut out = Vec::new();
    for client in state.db.oauth_list_clients()? {
        let Some(kinds) = by_app.remove(&client.id) else {
            continue;
        };
        if client.disabled {
            continue;
        }
        let choice = |k: &str| {
            prefs
                .iter()
                .find(|p| p.client_id == client.id && p.kind == k)
                .map(|p| p.email)
        };
        out.push(AppChoices {
            client: client.id,
            app: client.name.clone(),
            enabled: choice("*") != Some(false),
            kinds: kinds
                .into_iter()
                .map(|k| KindChoice {
                    email: choice(&k.kind).unwrap_or(false),
                    default: choice(&k.kind).is_none(),
                    kind: k.kind,
                    label: k.label,
                })
                .collect(),
        });
    }
    Ok(Json(ApiResponse::success(out)))
}

#[derive(Debug, Deserialize)]
struct SetChoices {
    #[serde(default)]
    enabled: Option<bool>,
    /// kind -> email
    #[serde(default)]
    kinds: HashMap<String, bool>,
}

/// `PUT /api/notify/preferences/{client}`
async fn set_preferences(
    State(state): State<AppState>,
    user: AuthUser,
    Path(client): Path<Uuid>,
    Json(req): Json<SetChoices>,
) -> Result<Json<ApiResponse<Value>>, ApiError> {
    if !oidc::enabled(&state) {
        return Err(disabled());
    }
    if state.db.oauth_get_client(client)?.is_none() {
        return Err(ApiError::NotFound("App not found".to_string()));
    }
    for kind in req.kinds.keys() {
        if state.db.notify_kind(client, kind)?.is_none() {
            return Err(ApiError::BadRequest(format!(
                "this app sends no {kind:?} messages"
            )));
        }
    }
    if let Some(enabled) = req.enabled {
        state
            .db
            .notify_set_preference(user.0.id, client, "*", enabled)?;
    }
    for (kind, email) in &req.kinds {
        state
            .db
            .notify_set_preference(user.0.id, client, kind, *email)?;
    }
    Ok(Json(ApiResponse::success(json!({ "saved": true }))))
}
