use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::calendar::window;
use crate::calendar::CalendarEvent;
use crate::AppState;

/// Routes for calendar functionality
pub fn calendar_routes() -> Router<AppState> {
    Router::new()
        .route("/events", get(get_calendar_events))
        .route("/events/refresh", get(refresh_calendar_events))
}

/// An optional window, as dates in the space's own timezone.
///
/// Both absent means "the configured list", which is what the home page's
/// panel wants and what this endpoint has always answered. The arithmetic,
/// and the reasoning for dates rather than instants, is in
/// [`crate::calendar::window`].
#[derive(Debug, Deserialize)]
struct EventWindow {
    /// First day to include, `YYYY-MM-DD`. Defaults to now.
    from: Option<String>,
    /// Last day to include, `YYYY-MM-DD`, inclusive. Defaults to the
    /// configured lookahead.
    to: Option<String>,
}

type ApiError = (StatusCode, Json<serde_json::Value>);

/// Get calendar events
async fn get_calendar_events(
    State(state): State<AppState>,
    Query(window): Query<EventWindow>,
) -> Result<Json<Vec<CalendarEvent>>, ApiError> {
    let bounds = resolve_window(&state, &window)?;
    let calendar_service = state.calendar_service.read().await;

    let result = match bounds {
        Some((from, to)) => calendar_service.get_events_between(from, to).await,
        None => calendar_service.get_events().await,
    };

    match result {
        Ok(events) => Ok(Json(events)),
        Err(e) => {
            error!("Failed to get calendar events: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to fetch calendar events",
                })),
            ))
        }
    }
}

/// Refresh calendar events (clear cache and fetch fresh)
async fn refresh_calendar_events(
    State(state): State<AppState>,
    Query(window): Query<EventWindow>,
) -> Result<Json<Vec<CalendarEvent>>, ApiError> {
    // Resolved before the cache is cleared: a bad date should be refused
    // without having thrown away every feed the server had.
    let bounds = resolve_window(&state, &window)?;
    let calendar_service = state.calendar_service.read().await;

    // Clear all cache
    calendar_service.clear_all_cache().await;

    // Fetch fresh events
    let result = match bounds {
        Some((from, to)) => calendar_service.get_events_between(from, to).await,
        None => calendar_service.get_events().await,
    };

    match result {
        Ok(events) => Ok(Json(events)),
        Err(e) => {
            error!("Failed to refresh calendar events: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to refresh calendar events",
                })),
            ))
        }
    }
}

/// The query's window, in the site's timezone, or a 400 naming the problem.
fn resolve_window(
    state: &AppState,
    query: &EventWindow,
) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>, ApiError> {
    let config = state.config_manager.get_config();
    window::bounds(
        query.from.as_deref(),
        query.to.as_deref(),
        crate::schedules::resolve_tz(&config.site.timezone),
        config.calendar.lookahead_days,
        Utc::now(),
    )
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e.message() })),
        )
    })
}
