//! Fetching and caching the space's iCalendar feeds.
//!
//! The feed semantics -- zones, recurrence, overrides -- live in [`ics`] and
//! are tested there. What is here is the part that needs a network and a
//! clock: which feeds to fetch, how long to hold them, and what window of
//! time a caller is asking about.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::{CalendarSource, ConfigManager};

pub mod description;
pub mod ics;
pub mod window;

pub use ics::CalendarEvent;

/// A fetched feed body, held so that a second question about the same feed
/// does not fetch it again.
///
/// The *body* is cached rather than the events parsed out of it. Parsing is
/// microseconds and depends on the window asked for, so caching events would
/// mean either refusing to answer a different window or serving one computed
/// against an hour-old "now".
#[derive(Debug, Clone)]
struct CachedFeed {
    body: String,
    fetched_at: DateTime<Utc>,
}

/// Calendar service that fetches and caches iCal feeds
pub struct CalendarService {
    /// HTTP client for fetching iCal feeds
    client: Client,
    /// Cache of feed bodies by source URL
    cache: Arc<RwLock<HashMap<String, CachedFeed>>>,
    /// Live configuration. Held as the manager rather than a snapshot so that
    /// a reload reaches the calendar without a restart, and so the site's
    /// timezone -- which the feeds need and which lives outside
    /// `[calendar]` -- is always the current one.
    config: Arc<ConfigManager>,
}

/// Ceiling on events returned when a caller names its own window.
///
/// `max_events_display` is the operator's answer to "how long is the list on
/// the home page", which is a different question from "how many events are in
/// March"; a month view asking for the second would be cut to ten by the
/// first. This is the safety limit for the explicit-window case.
const MAX_EVENTS_IN_WINDOW: usize = 1000;

impl CalendarService {
    /// Create a new calendar service
    pub fn new(config: Arc<ConfigManager>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Events from now to the configured lookahead, for the home page.
    ///
    /// Truncated to `max_events_display`. An event already under way is
    /// included -- it is what is happening in the building right now.
    pub async fn get_events(&self) -> Result<Vec<CalendarEvent>> {
        // One snapshot, so a reload between the two reads cannot produce a
        // window and a limit from different configurations.
        let calendar = self.config.get_config().calendar.clone();
        let now = Utc::now();
        let mut events = self
            .events_between(now, now + Duration::days(calendar.lookahead_days))
            .await?;
        events.truncate(calendar.max_events_display);
        Ok(events)
    }

    /// Events overlapping an explicit window, for a view that draws a month
    /// or a week rather than a list.
    pub async fn get_events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let mut events = self.events_between(from, to).await?;
        if events.len() > MAX_EVENTS_IN_WINDOW {
            warn!(
                "Calendar window {from}..{to} produced {} events; returning the first {MAX_EVENTS_IN_WINDOW}",
                events.len()
            );
            events.truncate(MAX_EVENTS_IN_WINDOW);
        }
        Ok(events)
    }

    /// The shared path: every enabled feed, parsed against one window, sorted.
    async fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let config = self.config.get_config();
        if !config.calendar.enabled {
            return Ok(Vec::new());
        }
        let site_tz = crate::schedules::resolve_tz(&config.site.timezone);

        let mut all_events = Vec::new();
        for source in &config.calendar.calendars {
            if !source.enabled {
                continue;
            }

            let (body, fresh) = match self.feed_body(source, &config.calendar).await {
                Ok(pair) => pair,
                Err(e) => {
                    // One unreachable feed must not fail the request: the
                    // other calendars are still answerable, and an empty page
                    // is a worse answer than a short one.
                    error!(
                        "Failed to fetch calendar '{}' from {}: {}",
                        source.name, source.ical_link, e
                    );
                    continue;
                }
            };

            let ctx = ics::FeedContext {
                calendar_name: source.name.clone(),
                calendar_color: source.color.clone(),
                site_tz,
                from,
                to,
            };
            match ics::parse_feed(&body, &ctx) {
                Ok(outcome) => {
                    // Only for a body just off the wire. Parsing happens per
                    // request now, so warning every time would repeat the
                    // same line about the same unchanged feed all day; this
                    // says it once per fetch, which is once per cache period.
                    if fresh {
                        for warning in &outcome.warnings {
                            warn!("Calendar '{}': {}", source.name, warning);
                        }
                    }
                    // Debug, not info: parsing now happens per request
                    // rather than per fetch, and the kiosk polls. A line per
                    // poll is noise that hides the warnings above it.
                    debug!(
                        "Parsed {} events from calendar '{}'",
                        outcome.events.len(),
                        source.name
                    );
                    all_events.extend(outcome.events);
                }
                // Gated the same way, and for the same reason: a feed URL
                // that has started answering with a login page stays in the
                // cache for its lifetime, and one line per request about it
                // would bury everything else.
                Err(e) => {
                    if fresh {
                        error!("Failed to parse calendar '{}': {}", source.name, e);
                    }
                }
            }
        }

        all_events.sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));
        Ok(all_events)
    }

    /// The feed body, and whether it was just fetched rather than cached.
    async fn feed_body(
        &self,
        source: &CalendarSource,
        calendar: &crate::config::CalendarConfig,
    ) -> Result<(String, bool)> {
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&source.ical_link) {
                let age = Utc::now() - cached.fetched_at;
                if age.num_minutes() < calendar.cache_duration_minutes as i64 {
                    return Ok((cached.body.clone(), false));
                }
            }
        }

        info!(
            "Fetching calendar '{}' from {}",
            source.name, source.ical_link
        );
        let response = self
            .client
            .get(&source.ical_link)
            .send()
            .await
            .with_context(|| format!("Failed to fetch iCal from {}", source.ical_link))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP error fetching calendar: {}",
                response.status()
            ));
        }

        let body = response
            .text()
            .await
            .context("Failed to read iCal response body")?;

        let mut cache = self.cache.write().await;
        cache.insert(
            source.ical_link.clone(),
            CachedFeed {
                body: body.clone(),
                fetched_at: Utc::now(),
            },
        );

        Ok((body, true))
    }

    /// Clear the cache for a specific calendar source
    pub async fn clear_cache(&self, ical_link: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(ical_link);
        info!("Cleared cache for calendar: {}", ical_link);
    }

    /// Clear all cached calendar data
    pub async fn clear_all_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        info!("Cleared all calendar cache");
    }
}
