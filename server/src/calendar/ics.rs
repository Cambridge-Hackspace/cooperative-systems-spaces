//! Turning an iCalendar feed into the events a member sees.
//!
//! This is the whole of the feed's semantics and none of the fetching, so it
//! can be tested against fixture bodies without a network or a config.
//!
//! The feed this was written against is a Google Calendar `basic.ics` export,
//! and every property below appears in it. The previous implementation handled
//! `FREQ` and `INTERVAL` and nothing else, which produced four distinct kinds
//! of wrong on the live site:
//!
//! * `DTSTART;TZID=America/New_York:20261006T183000` was read as if the wall
//!   clock were UTC, so every timed event in the feed displayed four hours
//!   early -- a 6:30pm Open Project Night appeared at 2:30pm.
//! * `BYDAY` was ignored, so `FREQ=MONTHLY;BYDAY=2MO` ("second Monday")
//!   became "every 30 days from the first one" and walked across the week.
//! * `UNTIL` was ignored, so series that ended in 2025 were still generating
//!   occurrences a year later.
//! * `EXDATE` and `RECURRENCE-ID` were ignored, so cancelled occurrences still
//!   showed and a moved occurrence showed twice -- once where it was expanded
//!   from the rule and once at the time it had been moved to.
//!
//! Recurrence expansion is `rrule`'s job here, not ours: `BYDAY`, `BYSETPOS`,
//! `UNTIL`, `COUNT`, `WKST` and the daylight-saving arithmetic are a standard
//! worth implementing once, in one place, by someone whose test suite is the
//! RFC's examples. What stays here is everything `rrule` has no opinion about:
//! which component is a master and which is an override of one occurrence of
//! it, what a missing `DTEND` means, and how an all-day date becomes an
//! instant.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{Calendar, CalendarComponent, Component, Event as IcalEvent, Property};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Represents a single calendar event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Event title/summary
    pub title: String,
    /// Event description, as the feed wrote it.
    ///
    /// Kept alongside the rendered form for clients that cannot display HTML
    /// -- the kiosk and the edge both read this shape.
    pub description: Option<String>,
    /// The description rendered for display: see [`super::description`].
    ///
    /// A feed's description is HTML written by whoever can edit the calendar,
    /// so it is reduced to Markdown and re-rendered rather than trusted. The
    /// component displays this and never `description`.
    pub description_html: Option<String>,
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

/// Everything the parser needs that does not come out of the feed itself.
#[derive(Debug, Clone)]
pub struct FeedContext {
    /// Display name of the calendar this feed is.
    pub calendar_name: String,
    /// Display colour of the calendar this feed is.
    pub calendar_color: String,
    /// The space's timezone. Used for all-day dates (which carry no zone of
    /// their own) and for floating times, so that the same feed parsed on two
    /// machines in different zones yields the same instants.
    pub site_tz: Tz,
    /// Start of the window of interest. An event is kept when it *overlaps*
    /// the window, so one already in progress is not dropped.
    pub from: DateTime<Utc>,
    /// End of the window of interest.
    pub to: DateTime<Utc>,
}

/// What a parse produced, including the things it could not use.
///
/// Warnings are returned rather than logged so that a test can assert on them:
/// an oracle for "the parser skipped something" is worth more than a line in a
/// container log nobody reads.
#[derive(Debug, Default)]
pub struct ParseOutcome {
    /// Events overlapping the requested window, unsorted.
    pub events: Vec<CalendarEvent>,
    /// One line per component the parser declined to use, naming the event.
    pub warnings: Vec<String>,
}

/// Ceiling on occurrences generated from one rule within the window. A feed
/// can ask for a million hourly occurrences; the page cannot show them and the
/// allocation is not free, so the expansion stops and says so.
const MAX_OCCURRENCES_PER_RULE: u16 = 1000;

/// Parse an iCalendar body.
///
/// Returns `Err` only when the body is not iCalendar at all. A single
/// unusable `VEVENT` produces a warning and is skipped: one malformed event in
/// a calendar a dozen people can edit must not blank the whole page.
pub fn parse_feed(body: &str, ctx: &FeedContext) -> Result<ParseOutcome, String> {
    // The parser is lenient to a fault: handed an HTML error page it returns a
    // calendar with no components, which is indistinguishable from a calendar
    // with nothing on it. A feed URL that has started answering with a login
    // page or a 404 body would then quietly empty the page instead of saying
    // anything, so the one structural thing every iCalendar body has is
    // checked first.
    if !body
        .lines()
        .any(|l| l.trim_end().eq_ignore_ascii_case("BEGIN:VCALENDAR"))
    {
        return Err("no BEGIN:VCALENDAR line; not an iCalendar body".to_string());
    }

    let calendar: Calendar = body
        .parse()
        .map_err(|e| format!("not parseable as iCalendar: {e}"))?;

    let events: Vec<&IcalEvent> = calendar
        .components
        .iter()
        .filter_map(|c| match c {
            CalendarComponent::Event(e) => Some(e),
            _ => None,
        })
        .collect();

    let mut out = ParseOutcome::default();

    // Pass one: which occurrences have been overridden by a component of their
    // own. `RECURRENCE-ID` names the instant in the master's series that this
    // component replaces -- the master must not also emit it, or a moved
    // meeting appears at both the old time and the new one.
    //
    // The key is (UID, the named instant). A UID alone would suppress the
    // whole series the moment one occurrence moved.
    //
    // A *cancelled* override counts here too, and that is the whole of how a
    // producer says "this week's is off": a component naming the instant, with
    // `STATUS:CANCELLED`. It suppresses the master's occurrence (this pass)
    // and contributes no event of its own (the next one).
    let mut overridden: HashSet<(String, DateTime<Utc>)> = HashSet::new();
    for event in &events {
        if let (Some(uid), Some(prop)) = (event.get_uid(), event.properties().get("RECURRENCE-ID"))
        {
            match anchor_of(prop, ctx) {
                Ok(anchor) => {
                    overridden.insert((uid.to_string(), anchor.start_utc(ctx)));
                }
                Err(why) => out.warnings.push(format!(
                    "{}: unusable RECURRENCE-ID: {why}",
                    title_of(event)
                )),
            }
        }
    }

    // Pass two: every component, in the light of that set.
    for event in &events {
        if is_cancelled(event) {
            continue;
        }
        if let Err(why) = push_event(event, ctx, &overridden, &mut out) {
            out.warnings
                .push(format!("{}: skipped: {why}", title_of(event)));
        }
    }

    Ok(out)
}

/// Expand one component into the events it contributes to the window.
fn push_event(
    event: &IcalEvent,
    ctx: &FeedContext,
    overridden: &HashSet<(String, DateTime<Utc>)>,
    out: &mut ParseOutcome,
) -> Result<(), String> {
    let start_prop = event
        .properties()
        .get("DTSTART")
        .ok_or("no DTSTART".to_string())?;
    let anchor = anchor_of(start_prop, ctx)?;
    let start = anchor.start_utc(ctx);

    // A missing DTEND is not an error. RFC 5545 reads it as zero length for a
    // timed event and one day for an all-day one; `end: None` is how this
    // codebase has always said "no stated end" and the edge fills in an hour.
    let end = match event.properties().get("DTEND") {
        Some(prop) => Some(anchor_of(prop, ctx)?.start_utc(ctx)),
        None => match anchor {
            // An all-day event with no DTEND covers its one day. The instant
            // is the exclusive end -- the next local midnight -- which is also
            // how DTEND;VALUE=DATE reads.
            Anchor::Date(date) => Some(local_midnight(date + Duration::days(1), ctx.site_tz)),
            Anchor::Timed { .. } => None,
        },
    };
    // One duration, taken from the master, applied to every occurrence.
    //
    // What that does not model: a *multi-day all-day series* whose first
    // occurrence straddles a daylight-saving change has a 25-hour "day", and
    // every later occurrence then ends an hour after local midnight rather
    // than on it -- which a month grid reads as one day more than the event
    // covers. Modelling it properly means carrying a count of calendar days
    // for date anchors instead of a duration. Not done, because it needs a
    // multi-day all-day *recurring* event whose master happens to begin on a
    // changeover weekend, and the single all-day event in the space's feed
    // recurs not at all. Written down rather than silently assumed correct.
    let duration = end.map(|e| e - start);

    let rules = recurrence_lines(event, &anchor, ctx);
    if rules.is_empty() {
        // A single event, or an override of one occurrence of a series. Either
        // way it is one concrete instant and needs no expansion.
        if overlaps(start, end, ctx) {
            out.events.push(build(event, ctx, start, end, &anchor));
        }
        return Ok(());
    }

    let uid = event.get_uid().unwrap_or_default().to_string();
    let set: rrule::RRuleSet = rules
        .join("\n")
        .parse()
        .map_err(|e| format!("unusable recurrence rule: {e}"))?;

    // Widen the lower bound by the event's own length so an occurrence that
    // started before the window and is still running is still generated. The
    // overlap test below, not the bound, decides what is kept.
    let slack = duration
        .unwrap_or_else(Duration::zero)
        .max(Duration::zero());
    let after = (ctx.from - slack - Duration::seconds(1)).with_timezone(&rrule::Tz::UTC);
    let before = (ctx.to + Duration::seconds(1)).with_timezone(&rrule::Tz::UTC);
    let result = set
        .after(after)
        .before(before)
        .all(MAX_OCCURRENCES_PER_RULE);
    if result.limited {
        out.warnings.push(format!(
            "{}: recurrence stopped at {MAX_OCCURRENCES_PER_RULE} occurrences",
            title_of(event)
        ));
    }

    for occurrence in result.dates {
        let occurrence_start = occurrence.with_timezone(&Utc);
        if overridden.contains(&(uid.clone(), occurrence_start)) {
            continue;
        }
        let occurrence_end = duration.map(|d| occurrence_start + d);
        if overlaps(occurrence_start, occurrence_end, ctx) {
            out.events
                .push(build(event, ctx, occurrence_start, occurrence_end, &anchor));
        }
    }

    Ok(())
}

/// The `DTSTART`/`RRULE`/`EXDATE`/`RDATE` lines to hand to `rrule`, or an
/// empty vector when this component is not a series.
///
/// The lines are rebuilt rather than taken from the body, because by this
/// point the body has already been unfolded and parsed, and because `DTSTART`
/// needs a zone stated explicitly: `rrule` reads a zoneless `DTSTART` as the
/// *host's* local time, which would make expansion depend on which machine
/// parsed the feed.
fn recurrence_lines(event: &IcalEvent, anchor: &Anchor, ctx: &FeedContext) -> Vec<String> {
    // An override names one instant and carries no rule of its own even when
    // the master's RRULE is repeated in it, which some producers do.
    if event.properties().contains_key("RECURRENCE-ID") {
        return Vec::new();
    }

    let rrules: Vec<&Property> = properties_named(event, "RRULE");
    let rdates: Vec<&Property> = properties_named(event, "RDATE");
    if rrules.is_empty() && rdates.is_empty() {
        return Vec::new();
    }

    let (tz, stamp) = match anchor {
        Anchor::Date(date) => (ctx.site_tz, date.format("%Y%m%dT000000").to_string()),
        Anchor::Timed { naive, tz } => (*tz, naive.format("%Y%m%dT%H%M%S").to_string()),
    };

    let mut lines = vec![format!("DTSTART;TZID={}:{}", tz.name(), stamp)];
    for prop in rrules {
        lines.push(format!("RRULE:{}", prop.value()));
    }
    for key in ["EXDATE", "RDATE"] {
        for prop in properties_named(event, key) {
            // The zone travels with the value: an EXDATE in local time must
            // name the same instant the rule generates, or it cancels nothing.
            match prop.get_param_as("TZID", |s| Some(s.to_string())) {
                Some(tzid) => lines.push(format!("{key};TZID={tzid}:{}", prop.value())),
                None => lines.push(format!("{key}:{}", prop.value())),
            }
        }
    }
    lines
}

/// Every occurrence of a property, whether the parser filed it as a single
/// property or (for the repeatable ones) as a list.
///
/// `EXDATE` is the reason this exists: a Google feed emits one line per
/// excluded occurrence, and reading only the first would restore occurrences
/// somebody cancelled.
fn properties_named<'a>(event: &'a IcalEvent, key: &str) -> Vec<&'a Property> {
    let mut found: Vec<&Property> = event
        .multi_properties()
        .get(key)
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    if found.is_empty() {
        if let Some(prop) = event.properties().get(key) {
            found.push(prop);
        }
    }
    found
}

/// A `DTSTART`-shaped value: either a date with no time at all, or a wall
/// clock with the zone it is to be read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Date(NaiveDate),
    Timed { naive: NaiveDateTime, tz: Tz },
}

impl Anchor {
    fn start_utc(&self, ctx: &FeedContext) -> DateTime<Utc> {
        match self {
            Anchor::Date(date) => local_midnight(*date, ctx.site_tz),
            Anchor::Timed { naive, tz } => to_utc(*naive, *tz),
        }
    }
}

/// Read one date-or-time property.
///
/// Three forms appear in the wild and all three are here: `VALUE=DATE` (an
/// all-day date), a value ending in `Z` (UTC), and a wall clock with `TZID`.
/// A wall clock with neither is "floating" -- RFC 5545 says it means local
/// time wherever it is read, which for a calendar of events happening in one
/// building means the building's timezone.
fn anchor_of(prop: &Property, ctx: &FeedContext) -> Result<Anchor, String> {
    let value = prop.value().trim();
    // Some producers put several values in one property (RDATE does this).
    // For a start time only the first can be meant.
    let value = value.split(',').next().unwrap_or(value);

    if value.len() == 8 && !value.contains('T') {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .map(Anchor::Date)
            .map_err(|e| format!("bad date {value:?}: {e}"));
    }

    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S")
            .map_err(|e| format!("bad UTC date-time {value:?}: {e}"))?;
        return Ok(Anchor::Timed {
            naive,
            tz: chrono_tz::UTC,
        });
    }

    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
        .map_err(|e| format!("bad date-time {value:?}: {e}"))?;
    let tz = match prop.get_param_as("TZID", |s| Some(s.to_string())) {
        // An unknown TZID is not a reason to drop the event, but it is a
        // reason not to pretend: the site's own zone is the best available
        // reading of a wall clock, and it is what a floating time gets too.
        Some(tzid) => tzid.parse::<Tz>().unwrap_or(ctx.site_tz),
        None => ctx.site_tz,
    };
    Ok(Anchor::Timed { naive, tz })
}

/// A wall clock in a zone, as an instant.
///
/// Spring-forward produces a wall clock that does not exist and autumn
/// fall-back one that happens twice. Neither may panic and neither may depend
/// on anything but the inputs, so: the earlier of two, and for a gap, the
/// instant the gap begins.
fn to_utc(naive: NaiveDateTime, tz: Tz) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive).earliest() {
        Some(dt) => dt.with_timezone(&Utc),
        None => {
            // Inside a spring-forward gap. Walking forward an hour at a time
            // lands on the first wall clock that exists.
            for hours in 1..=3 {
                let shifted = naive + Duration::hours(hours);
                if let Some(dt) = tz.from_local_datetime(&shifted).earliest() {
                    return dt.with_timezone(&Utc);
                }
            }
            naive.and_utc()
        }
    }
}

/// Local midnight on a date, as an instant.
///
/// An all-day event carries a date and no zone. Pinning it to UTC midnight --
/// which is what this code used to do -- renders as the *previous* day for
/// every viewer west of Greenwich, so an all-day event in a Boston hackspace
/// showed up a day early. Local midnight is the instant that renders as this
/// date in the space's own zone.
fn local_midnight(date: NaiveDate, tz: Tz) -> DateTime<Utc> {
    to_utc(
        date.and_hms_opt(0, 0, 0)
            .expect("midnight exists on every date"),
        tz,
    )
}

/// Does this event touch the window?
///
/// Overlap, not containment: an event that began an hour ago and runs for
/// three is happening now, and the kiosk's "on now" display depends on it
/// still being in the list.
fn overlaps(start: DateTime<Utc>, end: Option<DateTime<Utc>>, ctx: &FeedContext) -> bool {
    if start >= ctx.to {
        return false;
    }
    match end {
        // A stated end is exclusive, so an event ending exactly at `from` is
        // over.
        Some(end) if end > start => end > ctx.from,
        _ => start >= ctx.from,
    }
}

fn build(
    event: &IcalEvent,
    ctx: &FeedContext,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
    anchor: &Anchor,
) -> CalendarEvent {
    CalendarEvent {
        title: title_of(event),
        description: event.get_description().map(|d| d.to_string()),
        description_html: event
            .get_description()
            .and_then(super::description::to_html),
        start,
        end,
        location: event.property_value("LOCATION").map(|s| s.to_string()),
        calendar_name: ctx.calendar_name.clone(),
        calendar_color: ctx.calendar_color.clone(),
        all_day: matches!(anchor, Anchor::Date(_)),
    }
}

fn title_of(event: &IcalEvent) -> String {
    event.get_summary().unwrap_or("Untitled Event").to_string()
}

/// A cancelled component is not an event. For a master this removes the
/// series; for an override it removes that one occurrence -- which is how a
/// producer says "this week's is off" without touching the rule.
fn is_cancelled(event: &IcalEvent) -> bool {
    event
        .property_value("STATUS")
        .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NY: Tz = chrono_tz::America::New_York;

    fn ctx(from: &str, to: &str) -> FeedContext {
        FeedContext {
            calendar_name: "Space".into(),
            calendar_color: "#428bca".into(),
            site_tz: NY,
            from: utc(from),
            to: utc(to),
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Wrap VEVENT bodies in a calendar. Written with real CRLF line endings,
    /// because that is what a feed is and a parser that only copes with `\n`
    /// would pass every test here and fail in production.
    fn feed(vevents: &[&str]) -> String {
        let mut s = String::from("BEGIN:VCALENDAR\r\nPRODID:-//test//EN\r\nVERSION:2.0\r\n");
        for v in vevents {
            s.push_str("BEGIN:VEVENT\r\n");
            for line in v.trim().lines() {
                s.push_str(line.trim());
                s.push_str("\r\n");
            }
            s.push_str("END:VEVENT\r\n");
        }
        s.push_str("END:VCALENDAR\r\n");
        s
    }

    fn parse(vevents: &[&str], ctx: &FeedContext) -> ParseOutcome {
        let mut outcome = parse_feed(&feed(vevents), ctx).expect("feed parses");
        outcome.events.sort_by_key(|e| e.start);
        outcome
    }

    /// Occurrence starts as wall clocks in the space's zone.
    ///
    /// Deliberately not UTC instants: an expectation of "19:00 EST" can be
    /// checked against the calendar by eye, where "2026-11-10T00:00:00Z" is a
    /// value only arithmetic can confirm -- and an expectation nobody can
    /// check is one that gets edited to match the code. Where the instant
    /// itself is the claim, the test asserts `start` directly as well.
    fn starts(outcome: &ParseOutcome) -> Vec<String> {
        outcome
            .events
            .iter()
            .map(|e| {
                e.start
                    .with_timezone(&NY)
                    .format("%Y-%m-%d %H:%M %Z")
                    .to_string()
            })
            .collect()
    }

    // ----- timezones ------------------------------------------------------

    #[test]
    fn a_wall_clock_is_read_in_the_zone_it_names() {
        // The live defect, in one assertion: 6:30pm in New York in October is
        // 22:30Z, not 18:30Z.
        let out = parse(
            &["UID:a
               SUMMARY:Open Project Night
               DTSTART;TZID=America/New_York:20261006T183000
               DTEND;TZID=America/New_York:20261006T220000"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 18:30 EDT"]);
        // The same claim as an instant, because the instant is the defect.
        assert_eq!(out.events[0].start, utc("2026-10-06T22:30:00Z"));
        assert_eq!(out.events[0].end, Some(utc("2026-10-07T02:00:00Z")));
        assert!(!out.events[0].all_day);
    }

    #[test]
    fn a_utc_value_is_already_an_instant() {
        let out = parse(
            &["UID:a
               SUMMARY:Workshop
               DTSTART:20261003T200000Z
               DTEND:20261003T220000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-03 16:00 EDT"]);
        assert_eq!(out.events[0].start, utc("2026-10-03T20:00:00Z"));
    }

    #[test]
    fn a_floating_time_is_read_in_the_spaces_zone() {
        // Not UTC: the alternative reading would move every floating event by
        // the offset, and the events are happening in the building.
        let out = parse(
            &["UID:a
               SUMMARY:Floating
               DTSTART:20261006T183000"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 18:30 EDT"]);
        assert_eq!(out.events[0].start, utc("2026-10-06T22:30:00Z"));
    }

    #[test]
    fn an_unknown_tzid_falls_back_to_the_spaces_zone_rather_than_dropping_the_event() {
        let out = parse(
            &["UID:a
               SUMMARY:Mystery zone
               DTSTART;TZID=Mars/Olympus_Mons:20261006T183000"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 18:30 EDT"]);
    }

    #[test]
    fn an_all_day_date_is_local_midnight_not_utc_midnight() {
        // UTC midnight renders as the previous day for every viewer west of
        // Greenwich, which is the whole membership.
        let out = parse(
            &["UID:a
               SUMMARY:HONK!
               DTSTART;VALUE=DATE:20261009
               DTEND;VALUE=DATE:20261011"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-09 00:00 EDT"]);
        assert_eq!(out.events[0].start, utc("2026-10-09T04:00:00Z"));
        // DTEND on a date is exclusive: the event covers the 9th and 10th.
        assert_eq!(out.events[0].end, Some(utc("2026-10-11T04:00:00Z")));
        assert!(out.events[0].all_day);
    }

    #[test]
    fn an_all_day_event_with_no_end_covers_its_own_day() {
        let out = parse(
            &["UID:a
               SUMMARY:One day
               DTSTART;VALUE=DATE:20261009"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events[0].end, Some(utc("2026-10-10T04:00:00Z")));
    }

    #[test]
    fn a_timed_event_with_no_end_has_none() {
        let out = parse(
            &["UID:a
               SUMMARY:Open ended
               DTSTART;TZID=America/New_York:20261006T183000"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events[0].end, None);
    }

    // ----- recurrence -----------------------------------------------------

    #[test]
    fn monthly_by_weekday_lands_on_that_weekday() {
        // FREQ=MONTHLY;BYDAY=2MO is the second Monday. Stepping 30 days from
        // the first occurrence -- the old behaviour -- walks across the week:
        // it would put the October occurrence on the 11th, a Sunday.
        let out = parse(
            &["UID:a
               SUMMARY:3D Printing Monthly Drop-In Training
               DTSTART;TZID=America/New_York:20260914T190000
               DTEND;TZID=America/New_York:20260914T210000
               RRULE:FREQ=MONTHLY;BYDAY=2MO"],
            &ctx("2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z"),
        );
        // 12 October and 9 November, both Mondays, both 7pm local.
        assert_eq!(
            starts(&out),
            ["2026-10-12 19:00 EDT", "2026-11-09 19:00 EST"]
        );
    }

    #[test]
    fn a_series_that_has_ended_produces_nothing() {
        // UNTIL was ignored before, so a series that stopped in July 2026 was
        // still putting events on the page in October.
        let out = parse(
            &["UID:a
               SUMMARY:Ended in July
               DTSTART;TZID=America/New_York:20251110T190000
               DTEND;TZID=America/New_York:20251110T210000
               RRULE:FREQ=MONTHLY;UNTIL=20260713T035959Z;BYDAY=2MO"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), [] as [String; 0]);
    }

    #[test]
    fn count_limits_the_series() {
        let out = parse(
            &["UID:a
               SUMMARY:Three times
               DTSTART;TZID=America/New_York:20261006T183000
               RRULE:FREQ=WEEKLY;COUNT=3"],
            &ctx("2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 3);
    }

    #[test]
    fn an_excluded_occurrence_does_not_appear() {
        let out = parse(
            &["UID:a
               SUMMARY:Weekly with a gap
               DTSTART;TZID=America/New_York:20261006T183000
               RRULE:FREQ=WEEKLY;BYDAY=TU
               EXDATE;TZID=America/New_York:20261013T183000"],
            &ctx("2026-10-01T00:00:00Z", "2026-10-25T00:00:00Z"),
        );
        assert_eq!(
            starts(&out),
            ["2026-10-06 18:30 EDT", "2026-10-20 18:30 EDT"]
        );
    }

    #[test]
    fn every_exdate_line_counts_not_just_the_first() {
        // A Google feed emits one EXDATE line per excluded occurrence. Reading
        // only the first restores occurrences somebody cancelled.
        let out = parse(
            &["UID:a
               SUMMARY:Weekly with two gaps
               DTSTART;TZID=America/New_York:20261006T183000
               RRULE:FREQ=WEEKLY;BYDAY=TU
               EXDATE;TZID=America/New_York:20261013T183000
               EXDATE;TZID=America/New_York:20261020T183000"],
            &ctx("2026-10-01T00:00:00Z", "2026-10-25T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 18:30 EDT"]);
    }

    #[test]
    fn a_moved_occurrence_appears_once_at_its_new_time() {
        // The duplicate seen live: the master expands to 6 October and an
        // override component also names 6 October, moved to 7:30pm.
        let out = parse(
            &[
                "UID:a
                 SUMMARY:Open Project Night
                 DTSTART;TZID=America/New_York:20261006T183000
                 DTEND;TZID=America/New_York:20261006T220000
                 RRULE:FREQ=WEEKLY;BYDAY=TU",
                "UID:a
                 SUMMARY:Open Project Night
                 RECURRENCE-ID;TZID=America/New_York:20261006T183000
                 DTSTART;TZID=America/New_York:20261006T193000
                 DTEND;TZID=America/New_York:20261006T220000",
            ],
            &ctx("2026-10-01T00:00:00Z", "2026-10-12T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 19:30 EDT"]);
    }

    #[test]
    fn an_override_only_suppresses_its_own_occurrence() {
        // A UID-keyed suppression would delete the rest of the series.
        let out = parse(
            &[
                "UID:a
                 SUMMARY:Weekly
                 DTSTART;TZID=America/New_York:20261006T183000
                 RRULE:FREQ=WEEKLY;BYDAY=TU",
                "UID:a
                 SUMMARY:Weekly
                 RECURRENCE-ID;TZID=America/New_York:20261013T183000
                 DTSTART;TZID=America/New_York:20261014T183000",
            ],
            &ctx("2026-10-01T00:00:00Z", "2026-10-25T00:00:00Z"),
        );
        assert_eq!(
            starts(&out),
            [
                "2026-10-06 18:30 EDT",
                "2026-10-14 18:30 EDT",
                "2026-10-20 18:30 EDT"
            ]
        );
    }

    #[test]
    fn an_override_of_a_different_series_does_not_suppress_this_one() {
        let out = parse(
            &[
                "UID:a
                 SUMMARY:Series A
                 DTSTART;TZID=America/New_York:20261006T183000
                 RRULE:FREQ=WEEKLY;BYDAY=TU;COUNT=1",
                "UID:b
                 SUMMARY:Series B moved
                 RECURRENCE-ID;TZID=America/New_York:20261006T183000
                 DTSTART;TZID=America/New_York:20261007T183000",
            ],
            &ctx("2026-10-01T00:00:00Z", "2026-10-12T00:00:00Z"),
        );
        assert_eq!(
            starts(&out),
            ["2026-10-06 18:30 EDT", "2026-10-07 18:30 EDT"]
        );
    }

    #[test]
    fn a_cancelled_override_removes_that_occurrence() {
        let out = parse(
            &[
                "UID:a
                 SUMMARY:Weekly
                 DTSTART;TZID=America/New_York:20261006T183000
                 RRULE:FREQ=WEEKLY;BYDAY=TU",
                "UID:a
                 SUMMARY:Weekly
                 STATUS:CANCELLED
                 RECURRENCE-ID;TZID=America/New_York:20261013T183000
                 DTSTART;TZID=America/New_York:20261013T183000",
            ],
            &ctx("2026-10-01T00:00:00Z", "2026-10-18T00:00:00Z"),
        );
        // Asserted from both sides: the master's 13 October occurrence is gone
        // (the cancellation reached the expansion) and the cancelled component
        // contributed nothing of its own (it is not an event).
        assert!(!starts(&out).contains(&"2026-10-13 18:30 EDT".to_string()));
        assert_eq!(starts(&out), ["2026-10-06 18:30 EDT"]);
    }

    #[test]
    fn a_cancelled_master_removes_the_whole_series() {
        let out = parse(
            &["UID:a
               SUMMARY:Called off
               STATUS:CANCELLED
               DTSTART;TZID=America/New_York:20261006T183000
               RRULE:FREQ=WEEKLY;BYDAY=TU"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(starts(&out), [] as [String; 0]);
    }

    #[test]
    fn a_yearly_rule_is_expanded_rather_than_dropped() {
        // FREQ=YEARLY was unsupported and silently produced nothing.
        let out = parse(
            &["UID:a
               SUMMARY:Annual meeting
               DTSTART;TZID=America/New_York:20260308T100000
               RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU"],
            &ctx("2027-01-01T00:00:00Z", "2027-12-31T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2027-03-14 10:00 EDT"]);
    }

    #[test]
    fn an_all_day_series_expands_in_the_spaces_zone() {
        // A zoneless DTSTART handed to rrule is read as the *host's* local
        // time, so without an explicit zone this test passes or fails
        // depending on which machine runs it.
        let out = parse(
            &["UID:a
               SUMMARY:First Saturday
               DTSTART;VALUE=DATE:20261003
               RRULE:FREQ=MONTHLY;BYDAY=1SA;COUNT=2"],
            &ctx("2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z"),
        );
        assert_eq!(
            starts(&out),
            ["2026-10-03 00:00 EDT", "2026-11-07 00:00 EST"]
        );
        assert!(out.events.iter().all(|e| e.all_day));
    }

    #[test]
    fn daylight_saving_keeps_the_wall_clock() {
        // Weekly at 7pm across the November change: the instant moves by an
        // hour, the wall clock does not. Arithmetic on instants gets this
        // backwards, which is the other half of why rrule does the expansion.
        let out = parse(
            &["UID:a
               SUMMARY:Across the change
               DTSTART;TZID=America/New_York:20261029T190000
               RRULE:FREQ=WEEKLY;COUNT=2"],
            &ctx("2026-10-01T00:00:00Z", "2026-12-01T00:00:00Z"),
        );
        assert_eq!(
            starts(&out),
            ["2026-10-29 19:00 EDT", "2026-11-05 19:00 EST"]
        );
        // The wall clock is the same and the instants are an hour apart --
        // which is the arithmetic that "+1 week" on instants gets wrong.
        assert_eq!(out.events[0].start, utc("2026-10-29T23:00:00Z"));
        assert_eq!(out.events[1].start, utc("2026-11-06T00:00:00Z"));
    }

    // ----- the window -----------------------------------------------------

    #[test]
    fn an_event_already_under_way_is_still_in_the_window() {
        // The kiosk's "on now" line depends on it.
        let out = parse(
            &["UID:a
               SUMMARY:Started an hour ago
               DTSTART:20261006T170000Z
               DTEND:20261006T210000Z"],
            &ctx("2026-10-06T18:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 1);
    }

    #[test]
    fn a_finished_event_is_not() {
        let out = parse(
            &["UID:a
               SUMMARY:Over
               DTSTART:20261006T150000Z
               DTEND:20261006T170000Z"],
            &ctx("2026-10-06T18:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 0);
    }

    #[test]
    fn an_occurrence_under_way_survives_the_recurrence_window_too() {
        // The lower bound handed to rrule is widened by the event's length;
        // without that this occurrence is never generated to be tested.
        let out = parse(
            &["UID:a
               SUMMARY:Weekly, long
               DTSTART;TZID=America/New_York:20261006T130000
               DTEND;TZID=America/New_York:20261006T220000
               RRULE:FREQ=WEEKLY;BYDAY=TU"],
            &ctx("2026-10-06T18:00:00Z", "2026-10-08T00:00:00Z"),
        );
        assert_eq!(starts(&out), ["2026-10-06 13:00 EDT"]);
    }

    #[test]
    fn an_event_starting_after_the_window_is_excluded() {
        let out = parse(
            &["UID:a
               SUMMARY:Next month
               DTSTART:20261106T170000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 0);
    }

    // ----- the rest of the event ------------------------------------------

    #[test]
    fn the_feeds_own_fields_come_through() {
        let out = parse(
            &["UID:a
               SUMMARY:Wood Shop Drop-in Training
               LOCATION:Union Square\\, Somerville\\, MA
               DESCRIPTION:Bring wood.
               DTSTART:20261006T170000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        let event = &out.events[0];
        assert_eq!(event.title, "Wood Shop Drop-in Training");
        assert_eq!(
            event.location.as_deref(),
            Some("Union Square, Somerville, MA")
        );
        assert_eq!(event.description.as_deref(), Some("Bring wood."));
        assert_eq!(event.calendar_name, "Space");
        assert_eq!(event.calendar_color, "#428bca");
    }

    #[test]
    fn a_description_arrives_both_as_written_and_rendered() {
        // The live feed's descriptions are HTML. Both forms are carried: the
        // raw one for clients that show plain text, the rendered one for the
        // page.
        let out = parse(
            &["UID:a
               SUMMARY:Workshop
               DESCRIPTION:Review <a href=\"https://example.org/pre\">the reading</a> first.
               DTSTART:20261006T170000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        let event = &out.events[0];
        assert_eq!(
            event.description.as_deref(),
            Some("Review <a href=\"https://example.org/pre\">the reading</a> first.")
        );
        let html = event.description_html.as_deref().expect("rendered");
        assert!(
            html.contains("<a href=\"https://example.org/pre\">the reading</a>"),
            "{html}"
        );
        assert!(!html.contains("&lt;a href"), "{html}");
    }

    #[test]
    fn an_event_with_no_description_has_neither_form() {
        let out = parse(
            &["UID:a
               SUMMARY:Bare
               DTSTART:20261006T170000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events[0].description, None);
        assert_eq!(out.events[0].description_html, None);
    }

    #[test]
    fn an_untitled_event_is_labelled_rather_than_dropped() {
        let out = parse(
            &["UID:a
               DTSTART:20261006T170000Z"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events[0].title, "Untitled Event");
    }

    // ----- the real feed --------------------------------------------------

    /// Fourteen components lifted from the feed the space publishes, with the
    /// UIDs shortened and the descriptions dropped (bar one): the weekly
    /// master with six of its overrides, two monthly series that have expired,
    /// two that have not, a monthly series with an occurrence moved into the
    /// following month, and an all-day event.
    ///
    /// The hand-written cases above each isolate one rule. This one is the
    /// producer's actual output, which is the thing that was wrong -- every
    /// defect in this module's header shows up in it at once, so a change that
    /// reintroduces any of them fails here even if it slips past the others.
    const REAL_FEED: &str = include_str!("testdata/chack-general.ics");

    #[test]
    fn the_spaces_own_feed_reads_as_the_calendar_it_is() {
        // October 2026 in New York: the 1st at midnight to the end of the
        // 31st. A fixed window, because a test that moves with the clock
        // proves something different every day.
        let ctx = FeedContext {
            calendar_name: "CHACK General Calendar".into(),
            calendar_color: "#428bca".into(),
            site_tz: NY,
            from: utc("2026-10-01T04:00:00Z"),
            to: utc("2026-11-01T04:00:00Z"),
        };
        let mut out = parse_feed(REAL_FEED, &ctx).expect("the feed parses");
        out.events
            .sort_by(|a, b| a.start.cmp(&b.start).then(a.title.cmp(&b.title)));

        let listing: Vec<String> = out
            .events
            .iter()
            .map(|e| {
                format!(
                    "{} {}",
                    e.start.with_timezone(&NY).format("%a %Y-%m-%d %H:%M %Z"),
                    e.title.trim()
                )
            })
            .collect();

        assert_eq!(
            listing,
            [
                // Tuesdays at 6:30pm, once each. The live site showed this
                // series at 2:30pm, and showed 6 October twice.
                "Tue 2026-10-06 18:30 EDT Open Project Night",
                // Second Thursday at 7pm, plus the September occurrence its
                // organiser moved to the 8th at 7:30. Both are real.
                "Thu 2026-10-08 19:00 EDT Wood Shop Drop-in Training",
                "Thu 2026-10-08 19:30 EDT Wood Shop Drop-in Training",
                // Second Monday. The live site put this on the 6th, the 7th
                // and the 11th -- three occurrences of a monthly series in one
                // week, two of them from series that ended in 2025 and 2026.
                "Mon 2026-10-12 19:00 EDT 3D Printing Monthly Drop-In Training",
                "Tue 2026-10-13 18:30 EDT Open Project Night",
                // Third Monday.
                "Mon 2026-10-19 19:00 EDT CO2 Laser Training",
                "Tue 2026-10-20 18:30 EDT Open Project Night",
                "Tue 2026-10-27 18:30 EDT Open Project Night",
                // Last Saturday. `BYDAY=-1SA` was unreachable before, so this
                // one did not appear at all.
                "Sat 2026-10-31 10:30 EDT Hack the Space",
            ]
        );
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    #[test]
    fn the_real_feeds_all_day_event_lands_on_its_own_dates() {
        // HONK! is 9-10 October 2025 in the feed, as two days of all-day.
        let ctx = FeedContext {
            calendar_name: "CHACK General Calendar".into(),
            calendar_color: "#428bca".into(),
            site_tz: NY,
            from: utc("2025-10-01T04:00:00Z"),
            to: utc("2025-11-01T04:00:00Z"),
        };
        let out = parse_feed(REAL_FEED, &ctx).expect("the feed parses");
        let honk = out
            .events
            .iter()
            .find(|e| e.title.starts_with("HONK!"))
            .expect("HONK! is in the window");
        assert!(honk.all_day);
        assert_eq!(
            honk.start.with_timezone(&NY).format("%Y-%m-%d").to_string(),
            "2025-10-09"
        );
        // Its description in the feed is a bare anchor tag, which is why the
        // rendered form exists at all.
        let html = honk.description_html.as_deref().expect("rendered");
        assert!(html.contains("<a href=\"https://honkfest.org/"), "{html}");
    }

    // ----- failure modes --------------------------------------------------

    #[test]
    fn one_unusable_event_does_not_cost_the_others() {
        let out = parse(
            &[
                "UID:a
                 SUMMARY:Broken
                 DTSTART;TZID=America/New_York:not-a-date",
                "UID:b
                 SUMMARY:Fine
                 DTSTART:20261006T170000Z",
            ],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].title, "Fine");
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("Broken"), "{:?}", out.warnings);
    }

    #[test]
    fn an_event_with_no_start_is_skipped_with_a_warning() {
        let out = parse(
            &["UID:a
               SUMMARY:When?"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 0);
        assert!(out.warnings[0].contains("no DTSTART"), "{:?}", out.warnings);
    }

    #[test]
    fn an_unusable_rule_skips_the_series_and_says_so() {
        let out = parse(
            &["UID:a
               SUMMARY:Bad rule
               DTSTART:20261006T170000Z
               RRULE:FREQ=NEVER"],
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"),
        );
        assert_eq!(out.events.len(), 0);
        assert!(
            out.warnings[0].contains("unusable recurrence rule"),
            "{:?}",
            out.warnings
        );
    }

    #[test]
    fn a_body_that_is_not_icalendar_is_an_error() {
        assert!(parse_feed(
            "<html>404</html>",
            &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z")
        )
        .is_err());
    }

    #[test]
    fn a_runaway_rule_is_capped_and_reported() {
        // An hourly rule over a month is 744 occurrences; over a year it is
        // 8760. The cap is what keeps one feed from deciding how much memory
        // the server uses.
        let out = parse(
            &["UID:a
               SUMMARY:Hourly forever
               DTSTART:20260101T000000Z
               RRULE:FREQ=HOURLY"],
            &ctx("2026-01-01T00:00:00Z", "2026-12-31T00:00:00Z"),
        );
        assert_eq!(out.events.len(), MAX_OCCURRENCES_PER_RULE as usize);
        assert!(
            out.warnings.iter().any(|w| w.contains("stopped at")),
            "{:?}",
            out.warnings
        );
    }

    #[test]
    fn a_feed_with_no_events_is_not_an_error() {
        let out = parse(&[], &ctx("2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z"));
        assert!(out.events.is_empty());
        assert!(out.warnings.is_empty());
    }
}
