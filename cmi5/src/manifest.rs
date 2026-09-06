//! The cmi5 course-structure manifest (`cmi5.xml`).
//!
//! A cmi5 package is a ZIP whose root holds `cmi5.xml` describing a course as a
//! tree of blocks and assignable units (AUs). This module models that tree and
//! parses/serializes it. Parsing tolerates namespace prefixes by matching on the
//! local element name, so a manifest that declares the CourseStructure namespace
//! with or without a prefix reads the same.
//!
//! Serialization exists for export ([`to_manifest_xml`]); it is checked against
//! parsing at the tree level (parse ∘ serialize ∘ parse is identity), not byte
//! for byte, because whitespace and attribute order carry no meaning here.

use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

/// The default XML version quick-xml assumes when a document omits its
/// declaration; used for attribute-value normalization.
const XML_VERSION: XmlVersion = XmlVersion::Implicit1_0;

/// The CourseStructure XML namespace, emitted on the root during serialization.
pub const COURSE_STRUCTURE_NS: &str = "https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd";

/// Everything that can go wrong turning bytes into a [`CourseStructure`].
///
/// The variants name the *reason* rather than a generic "parse failed" so the
/// import path can report which rule a bad package broke, and so the unit tests
/// can assert the specific rejection rather than merely that one occurred.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("malformed XML: {0}")]
    Xml(String),
    #[error("manifest is missing the required <{0}> element")]
    MissingElement(&'static str),
    #[error("<{element}> is missing the required '{attribute}' attribute")]
    MissingAttribute {
        element: &'static str,
        attribute: &'static str,
    },
    #[error("'{0}' is not a valid moveOn value")]
    BadMoveOn(String),
    #[error("'{0}' is not a valid launchMethod value")]
    BadLaunchMethod(String),
    #[error("masteryScore '{0}' is not a number in 0.0..=1.0")]
    BadMasteryScore(String),
    #[error("unexpected end of document inside <{0}>")]
    UnexpectedEof(&'static str),
}

impl From<quick_xml::Error> for ManifestError {
    fn from(e: quick_xml::Error) -> Self {
        ManifestError::Xml(e.to_string())
    }
}

impl From<quick_xml::events::attributes::AttrError> for ManifestError {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        ManifestError::Xml(e.to_string())
    }
}

// quick-xml's Writer surfaces the underlying sink's io::Error.
impl From<std::io::Error> for ManifestError {
    fn from(e: std::io::Error) -> Self {
        ManifestError::Xml(e.to_string())
    }
}

/// A cmi5 `moveOn` criterion: what the LMS must observe before an AU counts as
/// satisfied. The string forms match the CourseStructure XSD exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveOn {
    Passed,
    Completed,
    CompletedAndPassed,
    CompletedOrPassed,
    NotApplicable,
}

impl MoveOn {
    pub fn as_str(self) -> &'static str {
        match self {
            MoveOn::Passed => "Passed",
            MoveOn::Completed => "Completed",
            MoveOn::CompletedAndPassed => "CompletedAndPassed",
            MoveOn::CompletedOrPassed => "CompletedOrPassed",
            MoveOn::NotApplicable => "NotApplicable",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ManifestError> {
        match s {
            "Passed" => Ok(MoveOn::Passed),
            "Completed" => Ok(MoveOn::Completed),
            "CompletedAndPassed" => Ok(MoveOn::CompletedAndPassed),
            "CompletedOrPassed" => Ok(MoveOn::CompletedOrPassed),
            "NotApplicable" => Ok(MoveOn::NotApplicable),
            other => Err(ManifestError::BadMoveOn(other.to_string())),
        }
    }
}

/// How the content wants to be opened. The XSD default is `AnyWindow`; a manifest
/// that omits it is represented as `AnyWindow` here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchMethod {
    AnyWindow,
    OwnWindow,
}

impl LaunchMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            LaunchMethod::AnyWindow => "AnyWindow",
            LaunchMethod::OwnWindow => "OwnWindow",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ManifestError> {
        match s {
            "AnyWindow" => Ok(LaunchMethod::AnyWindow),
            "OwnWindow" => Ok(LaunchMethod::OwnWindow),
            other => Err(ManifestError::BadLaunchMethod(other.to_string())),
        }
    }
}

/// A localized string: a `<langstring lang="…">value</langstring>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LangString {
    pub lang: String,
    pub value: String,
}

/// The `<course>`: the package's top-level identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Course {
    pub id: String,
    #[serde(default)]
    pub title: Vec<LangString>,
    #[serde(default)]
    pub description: Vec<LangString>,
}

/// A declared `<objective>` (referenced by AUs/blocks). We keep the identity and
/// labels; objective references on AUs/blocks are kept as bare id strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub id: String,
    #[serde(default)]
    pub title: Vec<LangString>,
    #[serde(default)]
    pub description: Vec<LangString>,
}

/// An ordered child of the course/block tree: either a nested block or an AU.
/// Order is preserved because cmi5 sequencing and display depend on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Block(Block),
    Au(AssignableUnit),
}

/// A `<block>`: a grouping of AUs and/or nested blocks. Multi-AU blocks are just
/// blocks whose `children` hold more than one AU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(default)]
    pub title: Vec<LangString>,
    #[serde(default)]
    pub description: Vec<LangString>,
    #[serde(default)]
    pub objective_ids: Vec<String>,
    #[serde(default)]
    pub children: Vec<Node>,
}

/// An `<au>`: the launchable unit. `url` is relative to the package root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssignableUnit {
    pub id: String,
    pub move_on: MoveOn,
    #[serde(default)]
    pub mastery_score: Option<f64>,
    #[serde(default)]
    pub launch_method: Option<LaunchMethod>,
    #[serde(default)]
    pub title: Vec<LangString>,
    #[serde(default)]
    pub description: Vec<LangString>,
    #[serde(default)]
    pub objective_ids: Vec<String>,
    pub url: String,
    #[serde(default)]
    pub launch_parameters: Option<String>,
    #[serde(default)]
    pub entitlement_key: Option<String>,
}

/// A parsed `cmi5.xml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseStructure {
    pub course: Course,
    #[serde(default)]
    pub objectives: Vec<Objective>,
    /// Top-level blocks and AUs, in document order.
    #[serde(default)]
    pub nodes: Vec<Node>,
}

impl CourseStructure {
    /// Every AU in the tree, in document order, regardless of nesting depth.
    ///
    /// This is what the import path walks to create one `training_step`-linkable
    /// row per AU, and what export re-emits.
    pub fn assignable_units(&self) -> Vec<&AssignableUnit> {
        let mut out = Vec::new();
        collect_aus(&self.nodes, &mut out);
        out
    }
}

fn collect_aus<'a>(nodes: &'a [Node], out: &mut Vec<&'a AssignableUnit>) {
    for node in nodes {
        match node {
            Node::Au(au) => out.push(au),
            Node::Block(b) => collect_aus(&b.children, out),
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `cmi5.xml` document into a [`CourseStructure`].
pub fn parse_manifest(xml: &str) -> Result<CourseStructure, ManifestError> {
    // Note: text is *not* trimmed. Whitespace-only runs between elements are
    // ignored by the structural loops' `_ => {}` arms, while significant
    // whitespace inside leaf text (and around entity references in mixed
    // content) is preserved — trimming would corrupt the latter.
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event()? {
            Event::Start(e) if start_is(&e, "courseStructure") => {
                return parse_course_structure(&mut reader);
            }
            Event::Eof => return Err(ManifestError::MissingElement("courseStructure")),
            _ => {}
        }
    }
}

/// The local part of an element name (namespace prefix stripped), owned so it can
/// be matched without holding a borrow of the event.
fn start_local(e: &BytesStart<'_>) -> String {
    let ln = e.local_name();
    let s: &str = ln.as_ref();
    s.to_string()
}

fn start_is(e: &BytesStart<'_>, want: &str) -> bool {
    let ln = e.local_name();
    let s: &str = ln.as_ref();
    s == want
}

fn end_is(e: &BytesEnd<'_>, want: &str) -> bool {
    let ln = e.local_name();
    let s: &str = ln.as_ref();
    s == want
}

/// Read a single attribute value (by local name) off a start tag.
fn attr(e: &BytesStart<'_>, want: &str) -> Result<Option<String>, ManifestError> {
    for a in e.attributes() {
        let a = a?;
        let ln = a.key.local_name();
        let key: &str = ln.as_ref();
        if key == want {
            return Ok(Some(a.normalized_value(XML_VERSION)?.into_owned()));
        }
    }
    Ok(None)
}

fn parse_course_structure(reader: &mut Reader<&[u8]>) -> Result<CourseStructure, ManifestError> {
    let mut course: Option<Course> = None;
    let mut objectives: Vec<Objective> = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => match start_local(&e).as_str() {
                "course" => course = Some(parse_course(reader, &e, false)?),
                "objectives" => objectives = parse_objectives(reader)?,
                "block" => nodes.push(Node::Block(parse_block(reader, &e, false)?)),
                "au" => nodes.push(Node::Au(parse_au(reader, &e, false)?)),
                other => skip_to_end(reader, other)?,
            },
            Event::Empty(e) => match start_local(&e).as_str() {
                "course" => course = Some(parse_course(reader, &e, true)?),
                "block" => nodes.push(Node::Block(parse_block(reader, &e, true)?)),
                "au" => nodes.push(Node::Au(parse_au(reader, &e, true)?)),
                _ => {}
            },
            Event::End(e) if end_is(&e, "courseStructure") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("courseStructure")),
            _ => {}
        }
    }

    let course = course.ok_or(ManifestError::MissingElement("course"))?;
    Ok(CourseStructure {
        course,
        objectives,
        nodes,
    })
}

fn parse_course(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    empty: bool,
) -> Result<Course, ManifestError> {
    let id = attr(start, "id")?.ok_or(ManifestError::MissingAttribute {
        element: "course",
        attribute: "id",
    })?;
    let mut title = Vec::new();
    let mut description = Vec::new();

    if empty {
        return Ok(Course {
            id,
            title,
            description,
        });
    }
    loop {
        match reader.read_event()? {
            Event::Start(e) => match start_local(&e).as_str() {
                "title" => title = parse_langstrings(reader, "title")?,
                "description" => description = parse_langstrings(reader, "description")?,
                other => skip_to_end(reader, other)?,
            },
            Event::End(e) if end_is(&e, "course") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("course")),
            _ => {}
        }
    }

    Ok(Course {
        id,
        title,
        description,
    })
}

fn parse_objectives(reader: &mut Reader<&[u8]>) -> Result<Vec<Objective>, ManifestError> {
    let mut objectives = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if start_is(&e, "objective") => {
                let id = attr(&e, "id")?.ok_or(ManifestError::MissingAttribute {
                    element: "objective",
                    attribute: "id",
                })?;
                let mut title = Vec::new();
                let mut description = Vec::new();
                loop {
                    match reader.read_event()? {
                        Event::Start(inner) => match start_local(&inner).as_str() {
                            "title" => title = parse_langstrings(reader, "title")?,
                            "description" => {
                                description = parse_langstrings(reader, "description")?
                            }
                            other => skip_to_end(reader, other)?,
                        },
                        Event::End(end) if end_is(&end, "objective") => break,
                        Event::Eof => return Err(ManifestError::UnexpectedEof("objective")),
                        _ => {}
                    }
                }
                objectives.push(Objective {
                    id,
                    title,
                    description,
                });
            }
            Event::End(e) if end_is(&e, "objectives") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("objectives")),
            _ => {}
        }
    }
    Ok(objectives)
}

fn parse_block(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    empty: bool,
) -> Result<Block, ManifestError> {
    let id = attr(start, "id")?.ok_or(ManifestError::MissingAttribute {
        element: "block",
        attribute: "id",
    })?;
    let mut title = Vec::new();
    let mut description = Vec::new();
    let mut objective_ids = Vec::new();
    let mut children = Vec::new();

    if empty {
        return Ok(Block {
            id,
            title,
            description,
            objective_ids,
            children,
        });
    }
    loop {
        match reader.read_event()? {
            Event::Start(e) => match start_local(&e).as_str() {
                "title" => title = parse_langstrings(reader, "title")?,
                "description" => description = parse_langstrings(reader, "description")?,
                "objectives" => objective_ids = parse_objective_refs(reader)?,
                "block" => children.push(Node::Block(parse_block(reader, &e, false)?)),
                "au" => children.push(Node::Au(parse_au(reader, &e, false)?)),
                other => skip_to_end(reader, other)?,
            },
            Event::Empty(e) => match start_local(&e).as_str() {
                "block" => children.push(Node::Block(parse_block(reader, &e, true)?)),
                "au" => children.push(Node::Au(parse_au(reader, &e, true)?)),
                _ => {}
            },
            Event::End(e) if end_is(&e, "block") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("block")),
            _ => {}
        }
    }

    Ok(Block {
        id,
        title,
        description,
        objective_ids,
        children,
    })
}

fn parse_au(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    empty: bool,
) -> Result<AssignableUnit, ManifestError> {
    let id = attr(start, "id")?.ok_or(ManifestError::MissingAttribute {
        element: "au",
        attribute: "id",
    })?;
    // The XSD default for a missing moveOn is NotApplicable.
    let move_on = match attr(start, "moveOn")? {
        Some(s) => MoveOn::parse(&s)?,
        None => MoveOn::NotApplicable,
    };
    let mastery_score = match attr(start, "masteryScore")? {
        Some(s) => Some(
            s.parse::<f64>()
                .ok()
                .filter(|v| (0.0..=1.0).contains(v))
                .ok_or(ManifestError::BadMasteryScore(s))?,
        ),
        None => None,
    };
    let launch_method = match attr(start, "launchMethod")? {
        Some(s) => Some(LaunchMethod::parse(&s)?),
        None => None,
    };

    let mut title = Vec::new();
    let mut description = Vec::new();
    let mut objective_ids = Vec::new();
    let mut url: Option<String> = None;
    let mut launch_parameters = None;
    let mut entitlement_key = None;

    // A self-closing AU carries no <url>, which the required-url check below
    // rejects; there is nothing to read, so skip straight to it.
    if empty {
        return Err(ManifestError::MissingElement("url"));
    }
    loop {
        match reader.read_event()? {
            Event::Start(e) => match start_local(&e).as_str() {
                "title" => title = parse_langstrings(reader, "title")?,
                "description" => description = parse_langstrings(reader, "description")?,
                "objectives" => objective_ids = parse_objective_refs(reader)?,
                "url" => url = Some(parse_text(reader, "url")?),
                "launchParameters" => {
                    launch_parameters = Some(parse_text(reader, "launchParameters")?)
                }
                "entitlementKey" => entitlement_key = Some(parse_text(reader, "entitlementKey")?),
                other => skip_to_end(reader, other)?,
            },
            // A self-closing leaf (e.g. <url/>) carries empty content.
            Event::Empty(e) => match start_local(&e).as_str() {
                "url" => url = Some(String::new()),
                "launchParameters" => launch_parameters = Some(String::new()),
                "entitlementKey" => entitlement_key = Some(String::new()),
                _ => {}
            },
            Event::End(e) if end_is(&e, "au") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("au")),
            _ => {}
        }
    }

    let url = url.ok_or(ManifestError::MissingElement("url"))?;
    Ok(AssignableUnit {
        id,
        move_on,
        mastery_score,
        launch_method,
        title,
        description,
        objective_ids,
        url,
        launch_parameters,
        entitlement_key,
    })
}

/// Read a `<title>`/`<description>` container holding zero or more `<langstring>`.
fn parse_langstrings(
    reader: &mut Reader<&[u8]>,
    end_tag: &str,
) -> Result<Vec<LangString>, ManifestError> {
    let mut out = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if start_is(&e, "langstring") => {
                let lang = attr(&e, "lang")?.unwrap_or_default();
                let value = parse_text(reader, "langstring")?;
                out.push(LangString { lang, value });
            }
            Event::Empty(e) if start_is(&e, "langstring") => {
                let lang = attr(&e, "lang")?.unwrap_or_default();
                out.push(LangString {
                    lang,
                    value: String::new(),
                });
            }
            Event::End(e) if end_is(&e, end_tag) => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("langstring")),
            _ => {}
        }
    }
    Ok(out)
}

/// Read an `<objectives>` block that holds `<objective idref="…">` references.
fn parse_objective_refs(reader: &mut Reader<&[u8]>) -> Result<Vec<String>, ManifestError> {
    let mut refs = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Empty(e) | Event::Start(e) if start_is(&e, "objective") => {
                if let Some(idref) = attr(&e, "idref")?.or(attr(&e, "id")?) {
                    refs.push(idref);
                }
                // A non-empty <objective>…</objective> reference has a matching
                // End, handled below; an Empty <objective/> has none.
            }
            Event::End(e) if end_is(&e, "objective") => {}
            Event::End(e) if end_is(&e, "objectives") => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("objectives")),
            _ => {}
        }
    }
    Ok(refs)
}

/// Read the text content of a simple element, up to its end tag.
///
/// quick-xml delivers literal runs as `Text`/`CData` and each XML entity as a
/// separate `GeneralRef`, so this reassembles them: predefined and numeric
/// character references are resolved back to their characters.
fn parse_text(reader: &mut Reader<&[u8]>, end_tag: &str) -> Result<String, ManifestError> {
    let mut text = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(e) => text.push_str(&e.into_inner()),
            Event::CData(e) => text.push_str(&e.into_inner()),
            Event::GeneralRef(e) => {
                if let Some(c) = e.resolve_char_ref()? {
                    text.push(c);
                } else {
                    let name = e.into_inner();
                    match resolve_predefined_entity(&name) {
                        Some(replacement) => text.push_str(replacement),
                        None => {
                            return Err(ManifestError::Xml(format!("unknown entity &{name};")));
                        }
                    }
                }
            }
            Event::End(e) if end_is(&e, end_tag) => break,
            Event::Eof => return Err(ManifestError::UnexpectedEof("text")),
            _ => {}
        }
    }
    Ok(text)
}

/// Consume events up to and including the end tag matching `name`, for elements
/// we do not model. Handles nesting of same-named elements.
fn skip_to_end(reader: &mut Reader<&[u8]>, name: &str) -> Result<(), ManifestError> {
    let mut depth = 1usize;
    loop {
        match reader.read_event()? {
            Event::Start(e) if start_is(&e, name) => depth += 1,
            Event::End(e) if end_is(&e, name) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => return Err(ManifestError::UnexpectedEof("element")),
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize a [`CourseStructure`] back to a `cmi5.xml` document.
///
/// The output is semantically equal to what [`parse_manifest`] would accept; it
/// is not guaranteed byte-identical to any particular input (whitespace and
/// attribute ordering are not significant).
pub fn to_manifest_xml(cs: &CourseStructure) -> Result<String, ManifestError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    let mut root = BytesStart::new("courseStructure");
    root.push_attribute(("xmlns", COURSE_STRUCTURE_NS));
    writer.write_event(Event::Start(root))?;

    write_course(&mut writer, &cs.course)?;
    if !cs.objectives.is_empty() {
        write_objectives(&mut writer, &cs.objectives)?;
    }
    for node in &cs.nodes {
        write_node(&mut writer, node)?;
    }

    writer.write_event(Event::End(BytesEnd::new("courseStructure")))?;

    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| ManifestError::Xml(e.to_string()))
}

type XmlWriter = Writer<Cursor<Vec<u8>>>;

fn write_course(w: &mut XmlWriter, c: &Course) -> Result<(), ManifestError> {
    let mut start = BytesStart::new("course");
    start.push_attribute(("id", c.id.as_str()));
    w.write_event(Event::Start(start))?;
    write_langstrings(w, "title", &c.title)?;
    write_langstrings(w, "description", &c.description)?;
    w.write_event(Event::End(BytesEnd::new("course")))?;
    Ok(())
}

fn write_objectives(w: &mut XmlWriter, objs: &[Objective]) -> Result<(), ManifestError> {
    w.write_event(Event::Start(BytesStart::new("objectives")))?;
    for o in objs {
        let mut start = BytesStart::new("objective");
        start.push_attribute(("id", o.id.as_str()));
        w.write_event(Event::Start(start))?;
        write_langstrings(w, "title", &o.title)?;
        write_langstrings(w, "description", &o.description)?;
        w.write_event(Event::End(BytesEnd::new("objective")))?;
    }
    w.write_event(Event::End(BytesEnd::new("objectives")))?;
    Ok(())
}

fn write_node(w: &mut XmlWriter, node: &Node) -> Result<(), ManifestError> {
    match node {
        Node::Block(b) => write_block(w, b),
        Node::Au(au) => write_au(w, au),
    }
}

fn write_block(w: &mut XmlWriter, b: &Block) -> Result<(), ManifestError> {
    let mut start = BytesStart::new("block");
    start.push_attribute(("id", b.id.as_str()));
    w.write_event(Event::Start(start))?;
    write_langstrings(w, "title", &b.title)?;
    write_langstrings(w, "description", &b.description)?;
    write_objective_refs(w, &b.objective_ids)?;
    for child in &b.children {
        write_node(w, child)?;
    }
    w.write_event(Event::End(BytesEnd::new("block")))?;
    Ok(())
}

fn write_au(w: &mut XmlWriter, au: &AssignableUnit) -> Result<(), ManifestError> {
    let mut start = BytesStart::new("au");
    start.push_attribute(("id", au.id.as_str()));
    start.push_attribute(("moveOn", au.move_on.as_str()));
    let mastery_owned;
    if let Some(m) = au.mastery_score {
        mastery_owned = m.to_string();
        start.push_attribute(("masteryScore", mastery_owned.as_str()));
    }
    if let Some(lm) = au.launch_method {
        start.push_attribute(("launchMethod", lm.as_str()));
    }
    w.write_event(Event::Start(start))?;
    write_langstrings(w, "title", &au.title)?;
    write_langstrings(w, "description", &au.description)?;
    write_objective_refs(w, &au.objective_ids)?;
    write_text_element(w, "url", &au.url)?;
    if let Some(lp) = &au.launch_parameters {
        write_text_element(w, "launchParameters", lp)?;
    }
    if let Some(ek) = &au.entitlement_key {
        write_text_element(w, "entitlementKey", ek)?;
    }
    w.write_event(Event::End(BytesEnd::new("au")))?;
    Ok(())
}

fn write_langstrings(
    w: &mut XmlWriter,
    wrapper: &str,
    strings: &[LangString],
) -> Result<(), ManifestError> {
    if strings.is_empty() {
        return Ok(());
    }
    w.write_event(Event::Start(BytesStart::new(wrapper)))?;
    for s in strings {
        let mut ls = BytesStart::new("langstring");
        ls.push_attribute(("lang", s.lang.as_str()));
        w.write_event(Event::Start(ls))?;
        w.write_event(Event::Text(BytesText::new(&s.value)))?;
        w.write_event(Event::End(BytesEnd::new("langstring")))?;
    }
    w.write_event(Event::End(BytesEnd::new(wrapper)))?;
    Ok(())
}

fn write_objective_refs(w: &mut XmlWriter, ids: &[String]) -> Result<(), ManifestError> {
    if ids.is_empty() {
        return Ok(());
    }
    w.write_event(Event::Start(BytesStart::new("objectives")))?;
    for id in ids {
        let mut o = BytesStart::new("objective");
        o.push_attribute(("idref", id.as_str()));
        w.write_event(Event::Empty(o))?;
    }
    w.write_event(Event::End(BytesEnd::new("objectives")))?;
    Ok(())
}

fn write_text_element(w: &mut XmlWriter, name: &str, text: &str) -> Result<(), ManifestError> {
    w.write_event(Event::Start(BytesStart::new(name)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(name)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_AU: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
  <course id="http://example.com/course/laser">
    <title><langstring lang="en-US">Laser Safety</langstring></title>
    <description><langstring lang="en-US">How not to lose a finger</langstring></description>
  </course>
  <au id="http://example.com/au/laser-basics" moveOn="CompletedAndPassed" masteryScore="0.8" launchMethod="OwnWindow">
    <title><langstring lang="en-US">Basics</langstring></title>
    <description><langstring lang="en-US">The basics</langstring></description>
    <url>content/index.html</url>
    <launchParameters>mode=strict</launchParameters>
  </au>
</courseStructure>"#;

    const MULTI_AU_BLOCK: &str = r#"<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
  <course id="c1"><title><langstring lang="en">C</langstring></title></course>
  <objectives>
    <objective id="obj1"><title><langstring lang="en">O1</langstring></title></objective>
  </objectives>
  <block id="b1">
    <title><langstring lang="en">Block One</langstring></title>
    <objectives><objective idref="obj1"/></objectives>
    <au id="au1" moveOn="Passed"><title><langstring lang="en">A1</langstring></title><url>a1.html</url></au>
    <au id="au2" moveOn="Completed"><title><langstring lang="en">A2</langstring></title><url>a2.html</url></au>
    <block id="b2">
      <title><langstring lang="en">Nested</langstring></title>
      <au id="au3" moveOn="CompletedOrPassed"><title><langstring lang="en">A3</langstring></title><url>a3.html</url></au>
    </block>
  </block>
  <au id="au4" moveOn="NotApplicable"><title><langstring lang="en">A4</langstring></title><url>a4.html</url></au>
</courseStructure>"#;

    #[test]
    fn parses_a_single_au_course() {
        let cs = parse_manifest(SINGLE_AU).expect("parse");
        assert_eq!(cs.course.id, "http://example.com/course/laser");
        assert_eq!(cs.course.title[0].value, "Laser Safety");
        assert_eq!(cs.course.title[0].lang, "en-US");
        let aus = cs.assignable_units();
        assert_eq!(aus.len(), 1);
        let au = aus[0];
        assert_eq!(au.id, "http://example.com/au/laser-basics");
        assert_eq!(au.move_on, MoveOn::CompletedAndPassed);
        assert_eq!(au.mastery_score, Some(0.8));
        assert_eq!(au.launch_method, Some(LaunchMethod::OwnWindow));
        assert_eq!(au.url, "content/index.html");
        assert_eq!(au.launch_parameters.as_deref(), Some("mode=strict"));
    }

    #[test]
    fn parses_a_multi_au_block_tree_in_order() {
        let cs = parse_manifest(MULTI_AU_BLOCK).expect("parse");
        assert_eq!(cs.objectives.len(), 1);
        assert_eq!(cs.objectives[0].id, "obj1");

        // Document order across nesting: au1, au2, au3 (nested), au4 (top level).
        let aus = cs.assignable_units();
        let ids: Vec<&str> = aus.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["au1", "au2", "au3", "au4"]);

        // The block references its objective and holds a nested block.
        let Node::Block(b1) = &cs.nodes[0] else {
            panic!("first node should be a block");
        };
        assert_eq!(b1.objective_ids, vec!["obj1"]);
        assert_eq!(b1.children.len(), 3); // au1, au2, nested block
        assert!(matches!(b1.children[2], Node::Block(_)));
    }

    #[test]
    fn round_trips_through_serialization_at_the_tree_level() {
        for xml in [SINGLE_AU, MULTI_AU_BLOCK] {
            let once = parse_manifest(xml).expect("parse 1");
            let serialized = to_manifest_xml(&once).expect("serialize");
            let twice = parse_manifest(&serialized).expect("parse 2");
            assert_eq!(once, twice, "parse∘serialize∘parse must be identity");
        }
    }

    #[test]
    fn round_trips_xml_entities_in_text_and_attributes() {
        // Ampersands and angle brackets in content must survive parse→serialize
        // →parse. If escaping on write or unescaping on read is wrong, the second
        // parse either fails or yields a different value.
        let xml = r#"<courseStructure xmlns="x">
            <course id="c &amp; co &lt;1&gt;">
                <title><langstring lang="en">Tools &amp; Dies &lt;intro&gt;</langstring></title>
            </course>
            <au id="a" moveOn="Passed"><url>p?x=1&amp;y=2</url></au>
        </courseStructure>"#;
        let once = parse_manifest(xml).expect("parse 1");
        assert_eq!(once.course.id, "c & co <1>");
        assert_eq!(once.course.title[0].value, "Tools & Dies <intro>");
        assert_eq!(once.assignable_units()[0].url, "p?x=1&y=2");

        let serialized = to_manifest_xml(&once).expect("serialize");
        let twice = parse_manifest(&serialized).expect("parse 2");
        assert_eq!(once, twice);
    }

    #[test]
    fn tolerates_a_namespace_prefix() {
        let prefixed = r#"<cs:courseStructure xmlns:cs="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
            <cs:course id="c"><cs:title><cs:langstring lang="en">T</cs:langstring></cs:title></cs:course>
            <cs:au id="a" moveOn="Passed"><cs:url>x.html</cs:url></cs:au>
        </cs:courseStructure>"#;
        let cs = parse_manifest(prefixed).expect("parse prefixed");
        assert_eq!(cs.course.id, "c");
        assert_eq!(cs.assignable_units()[0].url, "x.html");
    }

    #[test]
    fn rejects_a_missing_course() {
        let xml = r#"<courseStructure xmlns="x"><au id="a" moveOn="Passed"><url>u</url></au></courseStructure>"#;
        let err = parse_manifest(xml).unwrap_err();
        assert!(
            matches!(err, ManifestError::MissingElement("course")),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_an_au_with_no_url() {
        let xml = r#"<courseStructure xmlns="x"><course id="c"/><au id="a" moveOn="Passed"><title><langstring lang="en">t</langstring></title></au></courseStructure>"#;
        let err = parse_manifest(xml).unwrap_err();
        assert!(
            matches!(err, ManifestError::MissingElement("url")),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_a_bad_move_on() {
        let xml = r#"<courseStructure xmlns="x"><course id="c"/><au id="a" moveOn="Sideways"><url>u</url></au></courseStructure>"#;
        let err = parse_manifest(xml).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadMoveOn(ref v) if v == "Sideways"),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_an_out_of_range_mastery_score() {
        let xml = r#"<courseStructure xmlns="x"><course id="c"/><au id="a" moveOn="Passed" masteryScore="1.5"><url>u</url></au></courseStructure>"#;
        let err = parse_manifest(xml).unwrap_err();
        assert!(
            matches!(err, ManifestError::BadMasteryScore(ref v) if v == "1.5"),
            "got {err:?}"
        );
    }

    #[test]
    fn a_missing_move_on_defaults_to_not_applicable() {
        let xml = r#"<courseStructure xmlns="x"><course id="c"/><au id="a"><url>u</url></au></courseStructure>"#;
        let cs = parse_manifest(xml).expect("parse");
        assert_eq!(cs.assignable_units()[0].move_on, MoveOn::NotApplicable);
    }
}
