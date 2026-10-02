//! Rendering a feed's event description into HTML it is safe to display.
//!
//! The problem, as seen on the live home page: Google Calendar puts HTML in
//! `DESCRIPTION`, because that is what its own editor produces --
//!
//! ```text
//! ...give you some cool trinkets to take home!<br><br><b>BEFORE YOU
//! RSVP</b> - Please briefly review <a href="https://docs.google.com/...">
//! Pre-Reading Training</a>.
//! ```
//!
//! -- and the component interpolated it, so members read the tags. The tags
//! are not decoration: the link is how somebody finds the pre-reading before a
//! workshop they have signed up for.
//!
//! The fix is not `v-html` on the feed. A feed the space does not control
//! (anyone a calendar is shared with can edit an event) would then be able to
//! put script into every visitor's page, including signed-in ones, on the
//! public home page. So the markup is taken apart here and rebuilt from a
//! short list of things an event description is allowed to be:
//!
//! 1. the tags this module knows become Markdown, and every other tag is
//!    dropped -- the output of this step is text, not markup;
//! 2. a `<` that was not part of a tag is escaped, so that nothing
//!    downstream can read it as a tag;
//! 3. comrak renders that Markdown with raw HTML passthrough left off, which
//!    is also what blanks a `javascript:` or `data:` link destination.
//!
//! Two independent things therefore have to fail before markup from a feed
//! reaches a page: this module would have to emit a tag it was asked to drop,
//! *and* comrak would have to pass raw HTML through. The tests below assert
//! both halves, and `frontend/tests/structure/markdown-rendering.spec.ts`
//! asserts the second has not been switched on.

use comrak::{markdown_to_html, Options};

/// Render a feed description as display HTML.
///
/// Returns `None` for a description that renders to nothing, so that a
/// template can skip the element rather than draw an empty paragraph.
pub fn to_html(raw: &str) -> Option<String> {
    let markdown = to_markdown(raw);
    if markdown.trim().is_empty() {
        return None;
    }

    let mut options = Options::default();
    // A description that is plain text with a URL in it is the common case
    // after step one, and a bare URL that is not a link is a URL nobody can
    // follow from a phone. This extension does not affect which destinations
    // are allowed: comrak blanks a dangerous scheme either way, asserted in
    // `a_dangerous_link_destination_is_blanked` below.
    options.extension.autolink = true;
    // `options.render.unsafe_` is deliberately left at its default of false.
    // Setting it would hand every feed editor an XSS on the home page.

    let html = markdown_to_html(&markdown, &options);
    let html = html.trim().to_string();
    if html.is_empty() {
        None
    } else {
        Some(html)
    }
}

/// Reduce a feed description to Markdown.
///
/// Hand-written rather than regex-driven because the input is not
/// well-formed HTML and never will be: unclosed tags, bare `<`, attributes
/// in any order, and whatever a decade of people pasting from Word has left
/// behind. A scanner degrades into "drop it" on anything it does not
/// recognise, which is the behaviour wanted here.
pub fn to_markdown(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    // Destinations of the links currently open, so `</a>` knows what to close
    // with. A stack rather than one slot because nesting, while invalid, does
    // occur and must not lose the outer link.
    let mut link_hrefs: Vec<Option<String>> = Vec::new();
    let mut text = String::new();
    let bytes: Vec<char> = raw.chars().collect();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != '<' {
            text.push(bytes[i]);
            i += 1;
            continue;
        }

        // A `<` only opens a tag when what follows it could be a tag name.
        // Without that test, "a < b and c > d" is read as text, a tag, and
        // more text -- and the middle of the sentence disappears. A `<` with
        // no `>` after it at all is likewise just a less-than sign.
        let opens_a_tag = bytes
            .get(i + 1)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '/' || *c == '!');
        let close = if opens_a_tag {
            bytes[i..].iter().position(|c| *c == '>')
        } else {
            None
        };
        let Some(close) = close else {
            text.push('<');
            i += 1;
            continue;
        };
        let tag: String = bytes[i + 1..i + close].iter().collect();
        i += close + 1;

        flush(&mut out, &mut text);
        if let Some(markdown) = replacement(&tag, &mut link_hrefs) {
            out.push_str(&markdown);
        }
    }

    flush(&mut out, &mut text);
    tidy(&out)
}

/// Append a run of text, neutralising anything that could be read as markup
/// further down.
///
/// Entity references are left exactly as the feed wrote them, and that is
/// deliberate. comrak resolves them -- in text and in link destinations, so
/// `javascript&#58;` is blanked like `javascript:` -- and doing it here first
/// would be worse than redundant: `&#42;` would become a `*` that the
/// renderer then reads as emphasis the author never asked for, and a `&gt;`
/// inside a URL would become a character this module strips out of
/// destinations.
fn flush(out: &mut String, text: &mut String) {
    if text.is_empty() {
        return;
    }
    for ch in text.chars() {
        match ch {
            // A `<` reaching here was not the start of a tag (there was no
            // `>` after it), so it is content -- and must not become one.
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    text.clear();
}

/// What one tag becomes.
fn replacement(tag: &str, link_hrefs: &mut Vec<Option<String>>) -> Option<String> {
    let tag = tag.trim();
    if tag.starts_with('!') {
        // A comment or a doctype. Neither is content.
        return None;
    }

    let closing = tag.starts_with('/');
    let body = tag.trim_start_matches('/');
    let name = body
        .split([' ', '\t', '\n', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match (name.as_str(), closing) {
        ("br", _) => Some("\n".into()),
        ("p", _) | ("div", _) | ("tr", _) => Some("\n\n".into()),
        ("ul", _) | ("ol", _) | ("table", _) => Some("\n\n".into()),
        ("li", false) => Some("\n- ".into()),
        ("li", true) => None,
        ("b", _) | ("strong", _) => Some("**".into()),
        ("i", _) | ("em", _) => Some("*".into()),
        ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => Some("\n\n**".into()),
        ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => Some("**\n\n".into()),
        ("a", false) => {
            let href = attribute(body, "href");
            // The opening bracket is emitted only when there is somewhere for
            // the link to go. Emitting it unconditionally and dropping the
            // closing half leaves a stray `[` in front of the text.
            let bracket = href.is_some();
            link_hrefs.push(href);
            Some(if bracket { "[".into() } else { String::new() })
        }
        ("a", true) => match link_hrefs.pop() {
            // `[text](<dest>)` rather than `[text](dest)`: a destination with
            // a space or a bracket in it ends the link early in the bare form,
            // which puts the rest of the URL on the page as text.
            Some(Some(href)) => Some(format!("](<{}>)", href.replace(['<', '>'], ""))),
            // A link with no destination is just its text. The brackets would
            // render literally, which looks like a broken link.
            _ => Some(String::new()),
        },
        // Everything else -- span, font, img, style, script, whatever a
        // producer invents -- contributes nothing. Note that the *content* of
        // a <script> block would survive as text; it cannot execute, because
        // step three escapes it, and the alternative (tracking which elements
        // hide their contents) is a parser this does not need to be.
        _ => None,
    }
}

/// Read one attribute out of a tag body. Quoted or not, any case.
fn attribute(body: &str, name: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let start = from + at;
        // Must be preceded by whitespace, or it is the tail of another
        // attribute's name (`data-href` is not `href`).
        let preceded_ok = start == 0
            || lower[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = &body[start + name.len()..];
        let trimmed = after.trim_start();
        if preceded_ok && trimmed.starts_with('=') {
            let value = trimmed[1..].trim_start();
            let value = match value.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    value[1..].split(quote).next().unwrap_or("").to_string()
                }
                _ => value
                    .split([' ', '\t', '\n'])
                    .next()
                    .unwrap_or("")
                    .to_string(),
            };
            return if value.is_empty() { None } else { Some(value) };
        }
        from = start + name.len();
    }
    None
}

/// Collapse the whitespace the tag substitutions leave behind.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut newlines = 0;
    for ch in text.chars() {
        if ch == '\n' {
            newlines += 1;
            // Two newlines is a paragraph break; more adds nothing and a run
            // of `<br>` at the end of a Google description is very common.
            if newlines <= 2 {
                out.push('\n');
            }
            continue;
        }
        newlines = 0;
        out.push(ch);
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- the live descriptions -------------------------------------------

    #[test]
    fn the_live_workshop_description_renders_as_text_and_a_link() {
        // Trimmed from the feed the space actually publishes, which is where
        // this whole module came from.
        let raw = "Ever wanted to learn how to use a 3D printer?<br><br><b>BEFORE YOU \
                   RSVP</b> - Please briefly review <a \
                   href=\"https://docs.google.com/document/d/1xESM/edit?usp=sharing\">\
                   Pre-Reading Training</a>. It's a 15 minute read.<br>";
        let html = to_html(raw).expect("renders");
        assert!(html.contains("<strong>BEFORE YOU RSVP</strong>"), "{html}");
        assert!(
            html.contains(
                "<a href=\"https://docs.google.com/document/d/1xESM/edit?usp=sharing\">Pre-Reading Training</a>"
            ),
            "{html}"
        );
        // And none of the tags are left for a member to read.
        assert!(!html.contains("&lt;br&gt;"), "{html}");
        assert!(!html.contains("<br>"), "{html}");
    }

    #[test]
    fn a_description_that_is_only_a_link_becomes_a_link() {
        let raw = "<a href=\"https://honkfest.org/2025-festival/schedule-2025/\">\
                   https://honkfest.org/2025-festival/schedule-2025/</a>";
        let html = to_html(raw).expect("renders");
        assert_eq!(
            html,
            "<p><a href=\"https://honkfest.org/2025-festival/schedule-2025/\">\
             https://honkfest.org/2025-festival/schedule-2025/</a></p>"
        );
    }

    #[test]
    fn plain_text_is_still_plain_text() {
        let html = to_html("Bring yourself, your ideas, and your projects.").expect("renders");
        assert_eq!(
            html,
            "<p>Bring yourself, your ideas, and your projects.</p>"
        );
    }

    #[test]
    fn markdown_in_a_plain_text_description_renders() {
        // The other half of the issue: descriptions written as Markdown were
        // displayed as their source.
        let html = to_html("**Bring** a _project_").expect("renders");
        assert_eq!(html, "<p><strong>Bring</strong> a <em>project</em></p>");
    }

    #[test]
    fn a_bare_url_becomes_a_link() {
        let html = to_html("Details at https://example.org/x").expect("renders");
        assert!(
            html.contains("<a href=\"https://example.org/x\">https://example.org/x</a>"),
            "{html}"
        );
    }

    #[test]
    fn an_empty_description_renders_to_nothing() {
        assert_eq!(to_html(""), None);
        assert_eq!(to_html("   \n  "), None);
        assert_eq!(to_html("<br><br>"), None);
        assert_eq!(to_html("<span></span>"), None);
    }

    // ----- structure -------------------------------------------------------

    #[test]
    fn line_breaks_and_paragraphs_survive_as_structure() {
        let html = to_html("one<br>two<br><br>three").expect("renders");
        // A single break inside a paragraph, a blank line between paragraphs.
        assert_eq!(html, "<p>one\ntwo</p>\n<p>three</p>");
    }

    #[test]
    fn a_list_becomes_a_list() {
        let html = to_html("Bring:<ul><li>wood</li><li>a plan</li></ul>").expect("renders");
        assert!(html.contains("<li>wood</li>"), "{html}");
        assert!(html.contains("<li>a plan</li>"), "{html}");
    }

    #[test]
    fn an_ampersand_survives_as_an_ampersand() {
        // comrak carries this claim, not this module -- entity handling is
        // deliberately left to it. The test is here as a pin on the contract
        // (a description reads as written) so that an upgrade which started
        // double-encoding would be caught where it would be noticed.
        let html = to_html("Tom &amp; Jerry &quot;live&quot;").expect("renders");
        assert_eq!(html, "<p>Tom &amp; Jerry &quot;live&quot;</p>");
    }

    #[test]
    fn an_unclosed_tag_does_not_eat_the_rest_of_the_description() {
        let html = to_html("before <b>bold and then nothing").expect("renders");
        assert!(html.contains("before"), "{html}");
        assert!(html.contains("bold and then nothing"), "{html}");
    }

    #[test]
    fn a_bare_less_than_is_text() {
        // Asserted at this module's own boundary, not only end to end: the
        // contract is that `to_markdown` emits text and never markup, and
        // comrak escaping a stray `<` for us is not the same promise.
        let markdown = to_markdown("keep the temperature < 5 degrees");
        assert!(!markdown.contains('<'), "{markdown}");
        let html = to_html("keep the temperature < 5 degrees").expect("renders");
        assert!(html.contains("&lt; 5 degrees"), "{html}");
    }

    #[test]
    fn a_comparison_in_prose_keeps_both_sides() {
        // "a < b and c > d" has a `<` and a later `>`, so reading the middle
        // as a tag drops "b and c" out of the sentence.
        let html = to_html("if a < b and c > d then stop").expect("renders");
        assert!(html.contains("b and c"), "{html}");
        assert!(html.contains("then stop"), "{html}");
    }

    #[test]
    fn an_awkward_url_keeps_all_of_itself() {
        // Both of these break the bare `](dest)` form: the first ends the
        // destination at the `)` and spills `b` onto the page as text, and the
        // second is not read as a link at all. Balanced parentheses --
        // Wikipedia's `Foo_(bar)` -- survive either way, which is why they are
        // not the test.
        let bracket = to_html("<a href=\"https://example.org/a)b\">Foo</a>").expect("renders");
        assert!(
            bracket.contains("href=\"https://example.org/a)b\""),
            "{bracket}"
        );
        assert!(!bracket.contains("b)"), "{bracket}");

        let spaced = to_html("<a href=\"https://example.org/a b\">Foo</a>").expect("renders");
        assert!(spaced.contains("<a href="), "{spaced}");
        assert!(spaced.contains(">Foo</a>"), "{spaced}");
    }

    #[test]
    fn an_href_less_link_keeps_its_text_without_brackets() {
        let html = to_html("<a name=\"anchor\">just words</a>").expect("renders");
        assert_eq!(html, "<p>just words</p>");
    }

    #[test]
    fn a_similarly_named_attribute_is_not_mistaken_for_href() {
        let html = to_html("<a data-href=\"https://evil.example\">text</a>").expect("renders");
        assert!(!html.contains("evil.example"), "{html}");
    }

    // ----- what must not get through ---------------------------------------
    //
    // Asserted at both layers: `to_markdown` must not emit a tag (so the
    // renderer is never asked to be safe), and `to_html` must not contain one
    // (so the renderer being safe is confirmed rather than assumed).

    #[test]
    fn a_script_tag_does_not_survive_either_layer() {
        let raw = "before<script>alert(1)</script>after";
        let markdown = to_markdown(raw);
        assert!(!markdown.contains("<script"), "{markdown}");
        let html = to_html(raw).expect("renders");
        assert!(!html.contains("<script"), "{html}");
        // The call is inert text, and says so rather than vanishing silently.
        assert!(html.contains("alert(1)"), "{html}");
    }

    #[test]
    fn an_event_handler_attribute_cannot_arrive_on_an_element() {
        let raw = "<img src=x onerror=\"alert(1)\">caption";
        let markdown = to_markdown(raw);
        assert!(!markdown.contains("onerror"), "{markdown}");
        let html = to_html(raw).expect("renders");
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("<img"), "{html}");
    }

    #[test]
    fn an_escaped_tag_stays_escaped() {
        // `&lt;script&gt;` decodes to `<script>`; re-escaping is what stops
        // that from reaching the renderer as markup.
        let raw = "&lt;script&gt;alert(1)&lt;/script&gt;";
        let markdown = to_markdown(raw);
        assert!(!markdown.contains("<script"), "{markdown}");
        let html = to_html(raw).expect("renders");
        assert!(!html.contains("<script"), "{html}");
    }

    #[test]
    fn a_dangerous_link_destination_is_blanked() {
        for raw in [
            "<a href=\"javascript:alert(1)\">click</a>",
            "<a href=\"JaVaScRiPt:alert(1)\">click</a>",
            "<a href=\"data:text/html;base64,PHNjcmlwdD4=\">click</a>",
            "[click](javascript:alert(1))",
        ] {
            let html = to_html(raw).expect("renders");
            assert!(
                !html.to_lowercase().contains("javascript:"),
                "{raw}: {html}"
            );
            assert!(
                !html.to_lowercase().contains("data:text/html"),
                "{raw}: {html}"
            );
            assert!(html.contains("click"), "{raw}: {html}");
        }
    }

    #[test]
    fn an_entity_encoded_scheme_is_blanked_too() {
        // comrak resolves the entity before deciding whether the destination
        // is dangerous, which is why this module does not need to. Asserted
        // because the alternative -- a browser resolving it after we hand it
        // over -- is a live link to `javascript:`.
        for raw in [
            "<a href=\"javascript&#58;alert(1)\">click</a>",
            "<a href=\"&#106;avascript:alert(1)\">click</a>",
        ] {
            let html = to_html(raw).expect("renders");
            assert!(html.contains("href=\"\""), "{raw}: {html}");
        }
    }

    #[test]
    fn an_http_link_is_not_blanked() {
        // Anti-vacuity for the test above: if every destination were blanked,
        // it would pass while the feature was broken.
        let html = to_html("<a href=\"http://example.org/x\">click</a>").expect("renders");
        assert!(html.contains("href=\"http://example.org/x\""), "{html}");
    }

    #[test]
    fn a_style_block_contributes_nothing_executable() {
        let html = to_html("<style>body{display:none}</style>Hello").expect("renders");
        assert!(!html.contains("<style"), "{html}");
        assert!(html.contains("Hello"), "{html}");
    }

    #[test]
    fn no_raw_html_marker_is_ever_rendered() {
        // comrak announces a dropped tag with `<!-- raw HTML omitted -->`. If
        // one of those reaches the page, this module let a tag through to the
        // renderer and the output is littered with comments; the member sees
        // nothing but the shape is wrong, so it is worth knowing.
        for raw in [
            "<script>x</script>",
            "<b>bold</b>",
            "&lt;b&gt;escaped&lt;/b&gt;",
            "<div onclick=\"x\">d</div>",
            "<iframe src=\"https://evil.example\"></iframe>text",
        ] {
            let html = to_html(raw).unwrap_or_default();
            assert!(!html.contains("raw HTML omitted"), "{raw}: {html}");
        }
    }
}
