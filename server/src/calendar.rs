use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use icalendar::{Component, DatePerhapsTime};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::config::{CalendarConfig, CalendarSource};

/// Represents a single calendar event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Event title/summary
    pub title: String,
    /// Event description
    pub description: Option<String>,
    /// Event start time
    pub start: DateTime<Utc>,
    /// Event end time
    pub end: Option<DateTime<Utc>>,
    /// Event location
    pub location: Option<String>,
    /// Calendar name this event belongs to
    pub calendar_name: String,
    /// Calendar color for display
    pub calendar_color: String,
    /// Whether this is an all-day event
    pub all_day: bool,
}

/// Cached calendar data
#[derive(Debug, Clone)]
struct CachedCalendar {
    /// The events
    events: Vec<CalendarEvent>,
    /// When this cache was created
    cached_at: DateTime<Utc>,
}

/// Calendar service that fetches and caches iCal feeds
pub struct CalendarService {
    /// HTTP client for fetching iCal feeds
    client: Client,
    /// Cache of calendar events by source URL
    cache: Arc<RwLock<HashMap<String, CachedCalendar>>>,
    /// Configuration
    config: CalendarConfig,
}

impl CalendarService {
    /// Create a new calendar service
    pub fn new(config: CalendarConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Get all events from all enabled calendars
    pub async fn get_events(&self) -> Result<Vec<CalendarEvent>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        let mut all_events = Vec::new();

        for source in &self.config.calendars {
            if !source.enabled {
                continue;
            }

            match self.get_calendar_events(source).await {
                Ok(events) => all_events.extend(events),
                Err(e) => {
                    error!(
                        "Failed to fetch calendar '{}' from {}: {}",
                        source.name, source.ical_link, e
                    );
                }
            }
        }

        // Sort by start time
        all_events.sort_by(|a, b| a.start.cmp(&b.start));

        // Limit to max_events_display
        all_events.truncate(self.config.max_events_display);

        Ok(all_events)
    }

    /// Get events from a specific calendar source
    async fn get_calendar_events(&self, source: &CalendarSource) -> Result<Vec<CalendarEvent>> {
        // Check cache first
        let cache_valid = {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&source.ical_link) {
                let age = Utc::now() - cached.cached_at;
                age.num_minutes() < self.config.cache_duration_minutes as i64
            } else {
                false
            }
        };

        if cache_valid {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&source.ical_link) {
                info!("Using cached calendar data for '{}'", source.name);
                return Ok(cached.events.clone());
            }
        }

        // Fetch fresh data
        info!("Fetching calendar '{}' from {}", source.name, source.ical_link);
        let events = self.fetch_and_parse_ical(source).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                source.ical_link.clone(),
                CachedCalendar {
                    events: events.clone(),
                    cached_at: Utc::now(),
                },
            );
        }

        Ok(events)
    }

    /// Fetch and parse an iCal feed
    async fn fetch_and_parse_ical(&self, source: &CalendarSource) -> Result<Vec<CalendarEvent>> {
        // Fetch the iCal data
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

        let ical_data = response
            .text()
            .await
            .context("Failed to read iCal response body")?;

        // Parse the iCal data
        self.parse_ical(&ical_data, source)
    }

    /// Parse iCal data into events
    fn parse_ical(&self, ical_data: &str, source: &CalendarSource) -> Result<Vec<CalendarEvent>> {
        let calendar = ical_data
            .parse::<icalendar::Calendar>()
            .map_err(|e| anyhow::anyhow!("Failed to parse iCal data: {}", e))?;

        let now = Utc::now();
        let lookahead_limit = now + Duration::days(self.config.lookahead_days);

        let mut events = Vec::new();

        for component in calendar.components {
            if let icalendar::CalendarComponent::Event(event) = component {
                // Extract event details
                let title = event
                    .get_summary()
                    .unwrap_or("Untitled Event")
                    .to_string();

                let description = event.get_description().map(|d| d.to_string());
                
                // Get location from properties
                let location = event.property_value("LOCATION").map(|s| s.to_string());

                // Parse start time
                let start_opt = event.get_start();
                let end_opt = event.get_end();

                if let Some(start) = start_opt {
                    let (start_dt, all_day) = match start {
                        DatePerhapsTime::DateTime(cal_dt) => {
                            // Convert CalendarDateTime to chrono DateTime
                            let dt = match cal_dt {
                                icalendar::CalendarDateTime::Floating(ndt) => {
                                    DateTime::from_naive_utc_and_offset(ndt, Utc)
                                }
                                icalendar::CalendarDateTime::Utc(dt) => dt,
                                icalendar::CalendarDateTime::WithTimezone { date_time, tzid: _ } => {
                                    // For simplicity, treat as UTC
                                    DateTime::from_naive_utc_and_offset(date_time, Utc)
                                }
                            };
                            (dt, false)
                        }
                        DatePerhapsTime::Date(date) => {
                            let dt = date.and_hms_opt(0, 0, 0).unwrap();
                            (DateTime::from_naive_utc_and_offset(dt, Utc), true)
                        }
                    };

                    let end_dt = end_opt.and_then(|end| match end {
                        DatePerhapsTime::DateTime(cal_dt) => {
                            let dt = match cal_dt {
                                icalendar::CalendarDateTime::Floating(ndt) => {
                                    DateTime::from_naive_utc_and_offset(ndt, Utc)
                                }
                                icalendar::CalendarDateTime::Utc(dt) => dt,
                                icalendar::CalendarDateTime::WithTimezone { date_time, tzid: _ } => {
                                    DateTime::from_naive_utc_and_offset(date_time, Utc)
                                }
                            };
                            Some(dt)
                        }
                        DatePerhapsTime::Date(date) => {
                            let dt = date.and_hms_opt(23, 59, 59).unwrap();
                            Some(DateTime::from_naive_utc_and_offset(dt, Utc))
                        }
                    });

                    // Check if this is a recurring event
                    let has_rrule = event.property_value("RRULE").is_some();
                    
                    if has_rrule {
                        // For recurring events, try to expand them
                        let expanded = self.expand_recurring_event(
                            &title,
                            &description,
                            &location,
                            start_dt,
                            end_dt,
                            all_day,
                            &event,
                            now,
                            lookahead_limit,
                            source,
                        );
                        events.extend(expanded);
                    } else {
                        // Single event - check if it's in our time window
                        if start_dt <= lookahead_limit && start_dt >= now {
                            events.push(CalendarEvent {
                                title,
                                description,
                                start: start_dt,
                                end: end_dt,
                                location,
                                calendar_name: source.name.clone(),
                                calendar_color: source.color.clone(),
                                all_day,
                            });
                        }
                    }
                }
            }
        }

        info!(
            "Parsed {} events from calendar '{}'",
            events.len(),
            source.name
        );

        Ok(events)
    }

    /// Expand a recurring event into individual occurrences
    /// This is a basic implementation that handles common RRULE patterns
    fn expand_recurring_event(
        &self,
        title: &str,
        description: &Option<String>,
        location: &Option<String>,
        start_dt: DateTime<Utc>,
        end_dt: Option<DateTime<Utc>>,
        all_day: bool,
        event: &icalendar::Event,
        now: DateTime<Utc>,
        lookahead_limit: DateTime<Utc>,
        source: &CalendarSource,
    ) -> Vec<CalendarEvent> {
        let mut occurrences = Vec::new();
        
        // Get the RRULE string
        let rrule_str = match event.property_value("RRULE") {
            Some(s) => s,
            None => return occurrences,
        };

        // Parse basic RRULE patterns
        // Format: FREQ=DAILY;INTERVAL=1;COUNT=10
        // or: FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20251231T235959Z
        
        let freq = if rrule_str.contains("FREQ=DAILY") {
            "DAILY"
        } else if rrule_str.contains("FREQ=WEEKLY") {
            "WEEKLY"
        } else if rrule_str.contains("FREQ=MONTHLY") {
            "MONTHLY"
        } else {
            info!("Unsupported RRULE frequency in: {}", rrule_str);
            return occurrences;
        };

        // Extract interval (default 1)
        let interval: i64 = rrule_str
            .split(';')
            .find(|s| s.starts_with("INTERVAL="))
            .and_then(|s| s.strip_prefix("INTERVAL="))
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        // Extract count limit (if specified)
        let count_limit: Option<usize> = rrule_str
            .split(';')
            .find(|s| s.starts_with("COUNT="))
            .and_then(|s| s.strip_prefix("COUNT="))
            .and_then(|s| s.parse().ok());

        // Calculate event duration for repeating
        let duration = end_dt.map(|end| end - start_dt);

        // Generate occurrences
        let mut current_dt = start_dt;
        let mut count = 0;
        let max_iterations = count_limit.unwrap_or(365); // Safety limit

        while current_dt <= lookahead_limit && count < max_iterations {
            // Only include events that are after "now"
            if current_dt >= now {
                let occurrence_end = duration.map(|d| current_dt + d);
                
                occurrences.push(CalendarEvent {
                    title: title.to_string(),
                    description: description.clone(),
                    start: current_dt,
                    end: occurrence_end,
                    location: location.clone(),
                    calendar_name: source.name.clone(),
                    calendar_color: source.color.clone(),
                    all_day,
                });
            }

            // Advance to next occurrence
            current_dt = match freq {
                "DAILY" => current_dt + Duration::days(interval),
                "WEEKLY" => current_dt + Duration::weeks(interval),
                "MONTHLY" => {
                    // Approximate month as 30 days (simplified)
                    current_dt + Duration::days(30 * interval)
                }
                _ => break,
            };

            count += 1;

            // Break if we've reached the count limit
            if let Some(limit) = count_limit {
                if count >= limit {
                    break;
                }
            }
        }

        info!(
            "Expanded recurring event '{}' into {} occurrences",
            title,
            occurrences.len()
        );

        occurrences
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

    /// Update configuration
    pub fn update_config(&mut self, config: CalendarConfig) {
        self.config = config;
    }
}
