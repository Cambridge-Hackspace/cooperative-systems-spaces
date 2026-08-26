use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde_json::json;
use tracing::error;

use crate::calendar::CalendarEvent;
use crate::AppState;

/// Routes for calendar functionality
pub fn calendar_routes() -> Router<AppState> {
    Router::new()
        .route("/events", get(get_calendar_events))
        .route("/events/refresh", get(refresh_calendar_events))
}

/// Get calendar events
async fn get_calendar_events(
    State(state): State<AppState>,
) -> Result<Json<Vec<CalendarEvent>>, (StatusCode, Json<serde_json::Value>)> {
    let calendar_service = state.calendar_service.read().await;

    match calendar_service.get_events().await {
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
) -> Result<Json<Vec<CalendarEvent>>, (StatusCode, Json<serde_json::Value>)> {
    let calendar_service = state.calendar_service.read().await;

    // Clear all cache
    calendar_service.clear_all_cache().await;

    // Fetch fresh events
    match calendar_service.get_events().await {
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
