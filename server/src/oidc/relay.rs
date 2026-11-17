//! The notification relay's rules (#226): who is emailed, what a message may
//! contain, and how it is written. Pure, so these are tested on their own.

/// How much a message matters, as the app says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Normal,
    High,
}

impl Urgency {
    pub fn parse(s: Option<&str>) -> Option<Self> {
        match s.unwrap_or("normal") {
            "normal" => Some(Urgency::Normal),
            "high" => Some(Urgency::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::Normal => "normal",
            Urgency::High => "high",
        }
    }
}

/// Should this member get this message by email? Their switch for the whole
/// app wins; then their choice for the kind; and with neither, only a
/// high-urgency message is sent -- anything else waits for them to opt in.
pub fn email_wanted(app_switch: Option<bool>, kind_choice: Option<bool>, urgency: Urgency) -> bool {
    if app_switch == Some(false) {
        return false;
    }
    kind_choice.unwrap_or(urgency == Urgency::High)
}

/// `print.done`, `queue.your_turn`: lowercase, starting with a letter.
pub fn kind_is_well_formed(kind: &str) -> bool {
    let mut chars = kind.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && kind.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_.-".contains(c))
}

pub const TITLE_MAX: usize = 120;
pub const BODY_MAX: usize = 2000;
pub const LINK_MAX: usize = 2000;
pub const LABEL_MAX: usize = 120;

/// A message is plain text. The title becomes an email subject, so it may
/// hold no line breaks at all; the body may hold line breaks and tabs and no
/// other control characters. A link is an absolute http(s) URL, shown as text.
pub fn check_message(title: &str, body: &str, link: Option<&str>) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX {
        return Err(format!("title must be 1 to {TITLE_MAX} characters"));
    }
    if title.chars().any(char::is_control) {
        return Err("title must be one line of plain text".to_string());
    }
    if body.chars().count() > BODY_MAX {
        return Err(format!("body must be at most {BODY_MAX} characters"));
    }
    if body
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err("body must be plain text".to_string());
    }
    if let Some(link) = link {
        let ok = link.len() <= LINK_MAX
            && url::Url::parse(link)
                .ok()
                .is_some_and(|u| matches!(u.scheme(), "https" | "http") && u.host_str().is_some());
        if !ok {
            return Err("link must be an absolute http(s) URL".to_string());
        }
    }
    Ok(())
}

/// A kind's label, as members read it.
pub fn check_label(label: &str) -> Result<(), String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > LABEL_MAX || label.chars().any(char::is_control)
    {
        return Err(format!(
            "label must be one line of 1 to {LABEL_MAX} characters"
        ));
    }
    Ok(())
}

/// The email: the app named in the subject, the body as the app wrote it, the
/// link on its own line, and a footer saying how to change what arrives.
pub fn compose_email(
    app: &str,
    title: &str,
    body: &str,
    link: Option<&str>,
    preferences_url: &str,
) -> (String, String) {
    let subject = format!("[{app}] {}", title.trim());
    let mut text = String::new();
    if !body.trim().is_empty() {
        text.push_str(body.trim_end());
        text.push_str("\n\n");
    }
    if let Some(link) = link {
        text.push_str(link);
        text.push_str("\n\n");
    }
    text.push_str(&format!(
        "--\nSent by {app} through CS:S, because of how you chose to be notified.\n\
         Change what reaches you: {preferences_url}\n"
    ));
    (subject, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_switch_wins_over_everything() {
        assert!(!email_wanted(Some(false), Some(true), Urgency::High));
    }

    #[test]
    fn a_kind_choice_wins_over_urgency() {
        assert!(!email_wanted(None, Some(false), Urgency::High));
        assert!(email_wanted(Some(true), Some(true), Urgency::Normal));
    }

    #[test]
    fn with_no_choice_only_high_urgency_is_sent() {
        assert!(email_wanted(None, None, Urgency::High));
        assert!(!email_wanted(None, None, Urgency::Normal));
        assert!(!email_wanted(Some(true), None, Urgency::Normal));
    }

    #[test]
    fn kinds_are_lowercase_words() {
        assert!(kind_is_well_formed("print.done"));
        assert!(kind_is_well_formed("queue.your_turn"));
        assert!(!kind_is_well_formed("Print.done"));
        assert!(!kind_is_well_formed("1print"));
        assert!(!kind_is_well_formed(""));
        assert!(!kind_is_well_formed(&"a".repeat(65)));
        assert!(!kind_is_well_formed("print done"));
    }

    #[test]
    fn a_title_cannot_carry_a_header() {
        // A line break in a subject is how a header would be smuggled in.
        assert!(check_message("Done\r\nBcc: x@y", "", None).is_err());
        assert!(check_message("Done\nBcc: x@y", "", None).is_err());
        assert!(check_message("", "", None).is_err());
        assert!(check_message(&"t".repeat(121), "", None).is_err());
        assert!(check_message("Your print is done", "Collect it.\nThanks", None).is_ok());
    }

    #[test]
    fn a_body_is_plain_text() {
        assert!(check_message("t", "bell\u{7}", None).is_err());
        assert!(check_message("t", &"b".repeat(2001), None).is_err());
        assert!(check_message("t", "line\n\tindented", None).is_ok());
    }

    #[test]
    fn a_link_is_an_absolute_web_url() {
        assert!(check_message("t", "", Some("https://pop.example/jobs/1")).is_ok());
        assert!(check_message("t", "", Some("javascript:alert(1)")).is_err());
        assert!(check_message("t", "", Some("/jobs/1")).is_err());
    }

    #[test]
    fn the_email_names_the_app_and_says_how_to_stop_it() {
        let (subject, text) = compose_email(
            "POP",
            "Your print is done",
            "Collect it from the X1.",
            Some("https://pop.example/jobs/1"),
            "https://css.example/notifications",
        );
        assert_eq!(subject, "[POP] Your print is done");
        assert!(text.starts_with("Collect it from the X1.\n\nhttps://pop.example/jobs/1\n\n--\n"));
        assert!(text.contains("https://css.example/notifications"));
    }

    #[test]
    fn urgency_defaults_to_normal_and_refuses_anything_else() {
        assert_eq!(Urgency::parse(None), Some(Urgency::Normal));
        assert_eq!(Urgency::parse(Some("high")), Some(Urgency::High));
        assert_eq!(Urgency::parse(Some("urgent")), None);
    }
}
