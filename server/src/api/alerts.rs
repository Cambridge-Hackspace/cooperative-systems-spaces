//! The alert feed (#87): audit events classified `Notice` or above, seen by
//! whoever holds `alerts.view`, acknowledged by whoever holds
//! `alerts.acknowledge`. Permission keys, never role names: the grants live in
//! `role_permissions` and an operator edits them in the roles screen.
//!
//! Every route is `AuthUser` + an in-handler permission check, the same shape
//! as `users.manage` elsewhere. The offline route matrix therefore sees these
//! as `Guard::Auth` and asserts only that an anonymous caller is refused; the
//! permission half is proven by the e2e `alerts` stage against the live
//! database (a plain member gets 403, a staff holder gets the feed).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    api::{
        errors::ApiError,
        responses::{ApiResponse, PaginatedResponse},
    },
    auth::AuthUser,
    database::AlertFilter,
    models::{AlertAcknowledgement, AuditEventType, AuditLog, Category, Severity},
    AppState,
};

pub const VIEW: &str = "alerts.view";
pub const ACKNOWLEDGE: &str = "alerts.acknowledge";

/// The feed shows this and above unless asked otherwise.
const DEFAULT_MIN_SEVERITY: Severity = Severity::Notice;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_alerts))
        .route("/summary", get(summary))
        .route("/classification", get(classification))
        .route("/{id}/acknowledge", post(acknowledge))
}

async fn require(state: &AppState, user: &AuthUser, key: &str) -> Result<(), ApiError> {
    if state
        .db
        .user_has_permission(user.0.id, key)
        .map_err(ApiError::from)?
    {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!(
            "requires the {key} permission"
        )))
    }
}

#[derive(Debug, Deserialize)]
pub struct AlertQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// One of `Category::ALL`, or absent for every category.
    pub category: Option<String>,
    /// One of `Severity::ALL`; defaults to `notice`.
    pub min_severity: Option<String>,
    /// `true` = acknowledged only, `false` = unacknowledged only, absent = both.
    pub acknowledged: Option<bool>,
}

/// One alert on the wire: the audit row plus its classification and
/// acknowledgement. The classification is sent per row so a client never
/// has to carry the map to read a feed.
#[derive(Debug, Serialize)]
pub struct AlertResponse {
    pub id: Uuid,
    pub event_type: String,
    pub category: Category,
    pub severity: Severity,
    pub user_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub event_data: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub acknowledgement: Option<AlertAcknowledgement>,
}

impl AlertResponse {
    fn from_row(log: AuditLog, ack: Option<AlertAcknowledgement>) -> Option<Self> {
        let kind = AuditEventType::parse(&log.event_type)?;
        Some(Self::from_parts(log, kind, ack))
    }

    fn from_parts(log: AuditLog, kind: AuditEventType, ack: Option<AlertAcknowledgement>) -> Self {
        Self {
            id: log.id,
            event_type: log.event_type,
            category: kind.category(),
            severity: kind.severity(),
            user_id: log.user_id,
            actor_id: log.actor_id,
            event_data: log.event_data,
            created_at: log.created_at,
            acknowledgement: ack,
        }
    }
}

/// The event types that match a severity floor and an optional category.
fn types_for(min: Severity, category: Option<Category>) -> Vec<String> {
    AuditEventType::all()
        .iter()
        .filter(|t| t.severity() >= min)
        .filter(|t| match category {
            Some(c) => t.category() == c,
            None => true,
        })
        .map(|t| t.as_str().to_string())
        .collect()
}

fn parse_filter(q: &AlertQuery) -> Result<(Severity, Option<Category>), ApiError> {
    let min = match q.min_severity.as_deref() {
        None => DEFAULT_MIN_SEVERITY,
        Some(s) => Severity::parse(s).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "unknown min_severity {s:?}; one of {}",
                Severity::ALL
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?,
    };
    let category = match q.category.as_deref() {
        None => None,
        Some(c) => Some(Category::parse(c).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "unknown category {c:?}; one of {}",
                Category::ALL
                    .iter()
                    .map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?),
    };
    Ok((min, category))
}

/// `GET /api/alerts` -- the feed, newest first, with a real total.
async fn list_alerts(
    user: AuthUser,
    State(state): State<AppState>,
    Query(q): Query<AlertQuery>,
) -> Result<Json<ApiResponse<PaginatedResponse<AlertResponse>>>, ApiError> {
    require(&state, &user, VIEW).await?;
    let page = q.page.unwrap_or(1);
    let per_page = q.per_page.unwrap_or(50);
    if page == 0 || per_page == 0 || per_page > 200 {
        return Err(ApiError::BadRequest(
            "Invalid pagination parameters".to_string(),
        ));
    }
    let (min, category) = parse_filter(&q)?;
    let filter = AlertFilter {
        types: types_for(min, category),
        acknowledged: q.acknowledged,
    };
    let total = state.db.count_alerts(&filter).map_err(ApiError::from)?;
    let rows = state
        .db
        .list_alerts(&filter, per_page as i64, ((page - 1) * per_page) as i64)
        .map_err(ApiError::from)?;
    let items: Vec<AlertResponse> = rows
        .into_iter()
        .filter_map(|(log, ack)| AlertResponse::from_row(log, ack))
        .collect();
    Ok(Json(ApiResponse::success(PaginatedResponse::new(
        items,
        page,
        per_page,
        total as u32,
    ))))
}

#[derive(Debug, Serialize)]
pub struct AlertSummary {
    /// Unacknowledged alerts (Notice and above) by severity.
    pub unacknowledged: BTreeMap<Severity, i64>,
    pub unacknowledged_total: i64,
}

/// `GET /api/alerts/summary` -- what is waiting, by severity. Drives the badge.
async fn summary(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<AlertSummary>>, ApiError> {
    require(&state, &user, VIEW).await?;
    let types = types_for(DEFAULT_MIN_SEVERITY, None);
    let counts = state
        .db
        .unacknowledged_alert_counts(&types)
        .map_err(ApiError::from)?;
    let mut by: BTreeMap<Severity, i64> = BTreeMap::new();
    for sev in Severity::ALL {
        if sev >= DEFAULT_MIN_SEVERITY {
            by.insert(sev, 0);
        }
    }
    let mut total = 0;
    for (kind, n) in counts {
        if let Some(t) = AuditEventType::parse(&kind) {
            *by.entry(t.severity()).or_insert(0) += n;
            total += n;
        }
    }
    Ok(Json(ApiResponse::success(AlertSummary {
        unacknowledged: by,
        unacknowledged_total: total,
    })))
}

#[derive(Debug, Serialize)]
pub struct Classification {
    pub category: Category,
    pub severity: Severity,
}

#[derive(Debug, Serialize)]
pub struct ClassificationResponse {
    pub categories: Vec<Category>,
    pub severities: Vec<Severity>,
    /// Every audit event type with its class. The audit log view colours
    /// its badges from this rather than from a map of its own.
    pub events: BTreeMap<String, Classification>,
}

/// `GET /api/alerts/classification` -- the map, for any signed-in user.
async fn classification(
    _user: AuthUser,
) -> Result<Json<ApiResponse<ClassificationResponse>>, ApiError> {
    let events = AuditEventType::all()
        .iter()
        .map(|t| {
            (
                t.as_str().to_string(),
                Classification {
                    category: t.category(),
                    severity: t.severity(),
                },
            )
        })
        .collect();
    Ok(Json(ApiResponse::success(ClassificationResponse {
        categories: Category::ALL.to_vec(),
        severities: Severity::ALL.to_vec(),
        events,
    })))
}

#[derive(Debug, Deserialize, Default)]
pub struct AcknowledgeRequest {
    pub note: Option<String>,
}

/// `POST /api/alerts/{id}/acknowledge` -- record that someone looked.
/// Idempotent; 404 for a row that is not an alert (below Notice) or absent,
/// so the feed's own view is the only thing this can act on.
async fn acknowledge(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    payload: Option<Json<AcknowledgeRequest>>,
) -> Result<Json<ApiResponse<AlertResponse>>, ApiError> {
    require(&state, &user, ACKNOWLEDGE).await?;
    let payload = payload.map(|Json(p)| p).unwrap_or_default();
    let note = payload
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > 1000) {
        return Err(ApiError::BadRequest(
            "note must be 1000 characters or fewer".to_string(),
        ));
    }
    let log = state
        .db
        .get_audit_log(id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("No such alert".to_string()))?;
    let kind = AuditEventType::parse(&log.event_type)
        .filter(|t| t.severity() >= DEFAULT_MIN_SEVERITY)
        .ok_or_else(|| ApiError::NotFound("No such alert".to_string()))?;

    let fresh = state
        .db
        .acknowledge_alert(id, user.0.id, note)
        .map_err(ApiError::from)?;
    if fresh {
        if let Err(e) = state
            .audit_logger
            .log_event(
                AuditEventType::AlertAcknowledged,
                log.user_id,
                Some(user.0.id),
                serde_json::json!({
                    "alert_id": id,
                    "alert_event_type": kind.as_str(),
                    "severity": kind.severity(),
                    "note": note,
                }),
                None,
                None,
            )
            .await
        {
            tracing::warn!("failed to log alert acknowledgement: {e}");
        }
    }

    let ack = state
        .db
        .get_alert_acknowledgement(id)
        .map_err(ApiError::from)?;
    let out = AlertResponse::from_parts(log, kind, ack);
    Ok(Json(ApiResponse::success_with_message(
        out,
        if fresh {
            "Alert acknowledged"
        } else {
            "Already acknowledged"
        }
        .to_string(),
    )))
}
