//! The document as its author declares it.
//!
//! This is the contract between whatever produced the document and the
//! engine. It describes **intent** — a heading, a table, a bold word — never
//! measurements: no atom, no line, no page appears here, because none of that
//! is known until the engine has looked at the fonts.
//!
//! # It is not a React format
//!
//! React is the first producer, not the only possible one. The IR is plain
//! JSON with a version on it, so Vue, Svelte, Python, Go or a hand-written
//! file can target the same engine. Nothing in this module knows that React
//! exists.
//!
//! # Assets are referenced, not embedded
//!
//! A font is half a megabyte and a logo is not much smaller. Base64 in JSON
//! would inflate both by a third and force the whole payload through a string
//! parser. Fonts and images are named here and passed alongside as bytes.

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Length, Pt};
use serde::{Deserialize, Serialize};

/// A whole document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub page: PageSetup,
    /// Repeated at the top of every page.
    #[serde(default)]
    pub header: Option<Band>,
    /// Repeated at the bottom of every page.
    #[serde(default)]
    pub footer: Option<Band>,
    /// Running totals the document keeps, named so a footer can refer to one.
    #[serde(default)]
    pub accumulators: Vec<String>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageSetup {
    /// Width and height in points. A4 unless said otherwise.
    pub width: Pt,
    pub height: Pt,
    pub margin: Edges<Pt>,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            width: Pt(595.2756),
            height: Pt(841.8898),
            margin: Edges::all(Pt(34.015747)),
        }
    }
}

/// A strip repeated at the top or the bottom of every page.
///
/// Declared once and built again for each page, because a page number and a
/// carried-forward total are different words on every sheet and glyphs
/// cannot be substituted after they are shaped. What varies is written as a
/// token — `{{page}}`, `{{pages}}`, `{{opening:total}}`, `{{closing:total}}`
/// — and filled in as the page is painted.
///
/// The height is reserved out of the content box rather than out of the
/// margin, so a band can never overlap the last line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub height: Pt,
    #[serde(default)]
    pub children: Vec<Node>,
}

/// One thing the author declared.
///
/// Serialised with the kind in a `"t"` field: `{"t": "text", "runs": […]}`.
/// Read back by hand rather than by `#[serde(tag = "t")]`, because the derive
/// buffers the whole map into an intermediate tree before it knows which
/// variant to build — see the `Deserialize` impl below, and the test in
/// `tests/allocations.rs` that holds the line.
///
/// Every variant wraps a named struct. That is what lets the hand-written
/// reader be one line per kind, and it gives each kind somewhere to document
/// itself.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Node {
    Text(Text),
    Box(Container),
    Row(Container),
    Table(Table),
    List(List),
    Image(Image),
    Link(Link),
    Canvas(Canvas),
    Spacer(Spacer),
    PageBreak(PageBreak),
}

/// A paragraph. One or more stretches, each with its own style.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub runs: Vec<Run>,
    #[serde(default)]
    pub style: TextStyle,
}

/// Children with padding and decoration around them.
///
/// Shared by `box` and `row`: the two differ only in whether the children are
/// stacked or set side by side, which is the enum variant, not the data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    #[serde(default)]
    pub style: BoxStyle,
    #[serde(default)]
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    /// Name of an asset supplied alongside the document.
    pub src: String,
    pub width: Pt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub href: String,
    pub child: std::boxed::Box<Node>,
}

/// Vertical space that draws nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spacer {
    pub height: Pt,
}

/// Forces the next content onto a new page.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBreak {
    #[serde(default)]
    pub to: BreakTo,
}

impl<'de> serde::Deserialize<'de> for Node {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> serde::de::Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(r#"a node object with a "t" field"#)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        use serde::de::Error;

        // The fast path, and the one every producer takes: `"t"` comes first,
        // so the kind is known before a single field of the body is read and
        // the body can be handed straight to the right struct.
        let Some(first) = map.next_key::<String>()? else {
            return Err(A::Error::custom("an empty node object"));
        };
        if first == "t" {
            let tag = map.next_value::<String>()?;
            return node(&tag, serde::de::value::MapAccessDeserializer::new(map));
        }

        // The slow path. JSON says key order carries no meaning, so a
        // document that puts the kind last is legal and has to work; it just
        // has to be buffered first, which is what the derive would have done
        // to every node.
        let mut rest = serde_json::Map::new();
        rest.insert(first, map.next_value()?);
        while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
            rest.insert(key, value);
        }
        let tag = rest
            .remove("t")
            .ok_or_else(|| A::Error::custom(r#"a node with no "t" field"#))?;
        let tag = tag
            .as_str()
            .ok_or_else(|| A::Error::custom(r#"a "t" field that is not a string"#))?
            .to_string();
        node(&tag, serde_json::Value::Object(rest)).map_err(A::Error::custom)
    }
}

/// Builds the variant `tag` names out of the body `body`.
fn node<'de, D: serde::Deserializer<'de>>(tag: &str, body: D) -> Result<Node, D::Error> {
    use serde::Deserialize;
    use serde::de::Error;

    Ok(match tag {
        "text" => Node::Text(Text::deserialize(body)?),
        "box" => Node::Box(Container::deserialize(body)?),
        "row" => Node::Row(Container::deserialize(body)?),
        "table" => Node::Table(Table::deserialize(body)?),
        "list" => Node::List(List::deserialize(body)?),
        "image" => Node::Image(Image::deserialize(body)?),
        "link" => Node::Link(Link::deserialize(body)?),
        "canvas" => Node::Canvas(Canvas::deserialize(body)?),
        "spacer" => Node::Spacer(Spacer::deserialize(body)?),
        "pageBreak" => Node::PageBreak(PageBreak::deserialize(body)?),
        other => {
            return Err(D::Error::unknown_variant(
                other,
                &[
                    "text",
                    "box",
                    "row",
                    "table",
                    "list",
                    "image",
                    "link",
                    "canvas",
                    "spacer",
                    "pageBreak",
                ],
            ));
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakTo {
    #[default]
    Next,
    /// The next odd page, so a chapter opens on the recto.
    Odd,
    Even,
}

/// One stretch of text with its own style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    #[serde(default)]
    pub weight: Weight,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub color: Option<Color>,
}

impl Run {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            weight: Weight::Regular,
            italic: false,
            color: None,
        }
    }

    pub fn bold(mut self) -> Self {
        self.weight = Weight::Bold;
        self
    }

    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn colored(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Weight {
    #[default]
    Regular,
    Bold,
}

/// How a paragraph is set.
///
/// Every field may be omitted. A format that makes you spell out four
/// defaults to change one is a format nobody writes by hand.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextStyle {
    pub size: Pt,
    pub color: Color,
    /// Minimum lines left at the foot of a page, and carried to the top.
    pub orphans: u8,
    pub widows: u8,
    /// Space after the paragraph.
    #[serde(default)]
    pub space_after: Pt,
    /// Keeps the paragraph with whatever follows — a heading with its text.
    #[serde(default)]
    pub keep_with_next: bool,
    /// Which edge of its box the lines are set against.
    ///
    /// The same `Align` a table column uses, and deliberately so: an amount
    /// under a table has to line up with the amounts in it, and two notions of
    /// "the right edge" would eventually disagree by a fraction of a point.
    #[serde(default)]
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: Pt(10.0),
            color: Color::BLACK,
            orphans: 2,
            widows: 2,
            space_after: Pt(0.0),
            keep_with_next: false,
            align: Align::Start,
        }
    }
}

/// How a box is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BoxStyle {
    #[serde(default)]
    pub background: Option<Color>,
    #[serde(default)]
    pub border: Edges<Option<Border>>,
    /// Corner radius. Applies to the background always, and to the border
    /// when it runs all the way round in one width and colour.
    #[serde(default)]
    pub radius: Pt,
    #[serde(default)]
    pub padding: Edges<Pt>,
    /// Explicit width. Without one a box fills what it is offered, which is
    /// wrong for two panels side by side.
    #[serde(default)]
    pub width: Option<Pt>,
    #[serde(default)]
    pub space_after: Pt,
    #[serde(default)]
    pub keep_with_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Border {
    pub width: Pt,
    pub color: Color,
}

/// A table: columns, an optional repeating header, and rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default = "Table::empty")]
pub struct Table {
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub header: Option<Row>,
    pub rows: Vec<Row>,
    /// Whether the header comes back at the top of each continuation page.
    ///
    /// On by default: declaring a header is a statement of intent, and a
    /// continuation without one is unreadable. Turning it off is the
    /// author's call, not the engine's.
    pub repeat_header: bool,
    #[serde(default)]
    pub padding: Edges<Pt>,
    #[serde(default)]
    pub space_after: Pt,
}

/// Everything about a table except its rows.
///
/// A table sent in pieces sends this first: the shape of the thing, which is
/// small and known up front, and then the rows, which are neither.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableHead {
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub header: Option<Row>,
    #[serde(default = "yes")]
    pub repeat_header: bool,
    #[serde(default)]
    pub padding: Edges<Pt>,
    #[serde(default)]
    pub space_after: Pt,
}

fn yes() -> bool {
    true
}

impl Table {
    /// The table without its rows.
    pub fn head(&self) -> TableHead {
        TableHead {
            columns: self.columns.clone(),
            header: self.header.clone(),
            repeat_header: self.repeat_header,
            padding: self.padding,
            space_after: self.space_after,
        }
    }

    /// A table with no columns and no rows.
    ///
    /// Not [`Default`], because `repeat_header` defaults to true and a
    /// derived `Default` would quietly say the opposite.
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            header: None,
            rows: Vec::new(),
            repeat_header: true,
            padding: Edges::default(),
            space_after: Pt(0.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnSpec {
    #[serde(default)]
    pub width: Length,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub overflow: Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Start,
    End,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Overflow {
    #[default]
    Wrap,
    Ellipsis,
    Clip,
}

/// One row of cells, and how it is drawn.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Row {
    pub cells: Vec<Cell>,
    #[serde(default)]
    pub style: BoxStyle,
    /// Values this row adds to the document's running totals, by accumulator
    /// index. This is how "carried forward" is fed.
    #[serde(default)]
    pub totals: Vec<TotalContribution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalContribution {
    pub accumulator: usize,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cell {
    pub text: String,
    #[serde(default)]
    pub size: Option<Pt>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub weight: Weight,
    #[serde(default)]
    pub italic: bool,
}

impl Cell {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: None,
            color: None,
            weight: Weight::Regular,
            italic: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct List {
    #[serde(default)]
    pub marker: Marker,
    pub items: Vec<String>,
    #[serde(default)]
    pub style: TextStyle,
    /// Width of the marker gutter. Twice the font size unless said otherwise.
    #[serde(default)]
    pub gutter: Option<Pt>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Marker {
    Bullet {
        glyph: String,
    },
    #[default]
    Decimal,
    LowerAlpha,
    LowerRoman,
    None,
}

fn bullet() -> String {
    "•".into()
}

/// The name alone when there is nothing else to say, and an object when there
/// is.
///
/// `"decimal"` is what an author writes and what a producer emits; the long
/// form exists for the one marker that carries a setting. Writing the short
/// form whenever it loses nothing keeps a document of nine thousand lists
/// readable.
impl Serialize for Marker {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Marker::Bullet { glyph } if glyph != &bullet() => MarkerRepr::Bullet {
                glyph: glyph.clone(),
            }
            .serialize(serializer),
            other => serializer.serialize_str(other.name()),
        }
    }
}

impl<'de> Deserialize<'de> for Marker {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Name(String),
            Long(MarkerRepr),
        }

        Ok(match Written::deserialize(deserializer)? {
            Written::Name(name) => Marker::named(&name)
                .ok_or_else(|| serde::de::Error::unknown_variant(&name, Marker::NAMES))?,
            Written::Long(MarkerRepr::Bullet { glyph }) => Marker::Bullet { glyph },
            Written::Long(MarkerRepr::Decimal) => Marker::Decimal,
            Written::Long(MarkerRepr::LowerAlpha) => Marker::LowerAlpha,
            Written::Long(MarkerRepr::LowerRoman) => Marker::LowerRoman,
            Written::Long(MarkerRepr::None) => Marker::None,
        })
    }
}

/// The long form, derived so the field names stay in one place.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum MarkerRepr {
    Bullet {
        #[serde(default = "bullet")]
        glyph: String,
    },
    Decimal,
    LowerAlpha,
    LowerRoman,
    None,
}

impl Marker {
    const NAMES: &'static [&'static str] =
        &["bullet", "decimal", "lowerAlpha", "lowerRoman", "none"];

    fn name(&self) -> &'static str {
        match self {
            Marker::Bullet { .. } => "bullet",
            Marker::Decimal => "decimal",
            Marker::LowerAlpha => "lowerAlpha",
            Marker::LowerRoman => "lowerRoman",
            Marker::None => "none",
        }
    }

    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "bullet" => Marker::Bullet { glyph: bullet() },
            "decimal" => Marker::Decimal,
            "lowerAlpha" => Marker::LowerAlpha,
            "lowerRoman" => Marker::LowerRoman,
            "none" => Marker::None,
            _ => return None,
        })
    }
}

/// A shape drawn from raw path operations — the escape hatch for barcodes,
/// QR codes, sparklines and anything else the engine has no primitive for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Canvas {
    pub width: Pt,
    pub height: Pt,
    pub ops: Vec<Op>,
    #[serde(default)]
    pub fill: Option<Color>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default)]
    pub space_after: Pt,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub color: Color,
    pub width: Pt,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    MoveTo {
        x: Pt,
        y: Pt,
    },
    LineTo {
        x: Pt,
        y: Pt,
    },
    CurveTo {
        x1: Pt,
        y1: Pt,
        x2: Pt,
        y2: Pt,
        x: Pt,
        y: Pt,
    },
    Rect {
        x: Pt,
        y: Pt,
        w: Pt,
        h: Pt,
    },
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every node must survive a round trip through JSON, or a producer in
    /// another language would silently lose part of the document.
    #[track_caller]
    fn round_trip(node: &Node) {
        let json = serde_json::to_string(node).expect("serialise");
        let back: Node = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(&back, node, "changed by a round trip:\n{json}");
    }

    #[test]
    fn every_node_survives_a_round_trip() {
        for node in [
            Node::Text(Text {
                runs: vec![Run::new("hola"), Run::new("mundo").bold()],
                style: TextStyle::default(),
            }),
            Node::Box(Container {
                style: BoxStyle::default(),
                children: vec![Node::Spacer(Spacer { height: Pt(4.0) })],
            }),
            Node::Row(Container {
                style: BoxStyle::default(),
                children: vec![],
            }),
            Node::Image(Image {
                src: "logo".into(),
                width: Pt(120.0),
            }),
            Node::Link(Link {
                href: "https://example.org".into(),
                child: Box::new(Node::Spacer(Spacer { height: Pt(1.0) })),
            }),
            Node::Spacer(Spacer { height: Pt(10.0) }),
            Node::PageBreak(PageBreak { to: BreakTo::Odd }),
        ] {
            round_trip(&node);
        }
    }

    #[test]
    fn a_table_survives_a_round_trip() {
        round_trip(&Node::Table(Table {
            columns: vec![ColumnSpec {
                width: Length::Pt(Pt(80.0)),
                align: Align::End,
                overflow: Overflow::Ellipsis,
            }],
            header: Some(Row {
                cells: vec![Cell::new("Importe")],
                ..Default::default()
            }),
            rows: vec![Row {
                cells: vec![Cell::new("1.234,56")],
                totals: vec![TotalContribution {
                    accumulator: 0,
                    value: 1234.56,
                }],
                ..Default::default()
            }],
            repeat_header: true,
            padding: Edges::all(Pt(3.0)),
            space_after: Pt(8.0),
        }));
    }

    #[test]
    fn a_canvas_survives_a_round_trip() {
        round_trip(&Node::Canvas(Canvas {
            width: Pt(100.0),
            height: Pt(40.0),
            ops: vec![
                Op::MoveTo {
                    x: Pt(0.0),
                    y: Pt(0.0),
                },
                Op::Rect {
                    x: Pt(0.0),
                    y: Pt(0.0),
                    w: Pt(10.0),
                    h: Pt(5.0),
                },
                Op::Close,
            ],
            fill: Some(Color::BLACK),
            stroke: None,
            space_after: Pt(0.0),
        }));
    }

    #[test]
    fn a_node_is_tagged_by_kind_so_a_producer_can_write_it_by_hand() {
        let json = serde_json::to_string(&Node::Spacer(Spacer { height: Pt(6.0) })).unwrap();

        assert!(json.contains(r#""t":"spacer""#), "{json}");
    }

    #[test]
    fn every_style_field_may_be_omitted() {
        // A producer changing one thing should not have to restate the rest.
        let node: Node =
            serde_json::from_str(r#"{"t":"text","runs":[{"text":"x"}],"style":{"size":14}}"#)
                .expect("parse");

        match node {
            Node::Text(Text { style, .. }) => {
                assert_eq!(style.size, Pt(14.0));
                assert_eq!(style.orphans, 2, "the rest kept their defaults");
                assert_eq!(style.color, Color::BLACK);
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_cell_needs_only_its_text() {
        let cell: Cell = serde_json::from_str(r#"{"text":"1.234,56"}"#).expect("parse");

        assert_eq!(cell.text, "1.234,56");
        assert_eq!(cell.weight, Weight::Regular);
    }

    #[test]
    fn optional_style_can_be_left_out_entirely() {
        // A producer should not have to spell out every default.
        let node: Node = serde_json::from_str(r#"{"t":"box"}"#).expect("parse");

        assert_eq!(
            node,
            Node::Box(Container {
                style: BoxStyle::default(),
                children: vec![],
            })
        );
    }

    #[test]
    fn a_table_header_repeats_unless_the_author_says_otherwise() {
        let table: Table = serde_json::from_str(r#"{"columns":[],"rows":[]}"#).expect("parse");

        assert!(table.repeat_header, "the default must be to repeat");
    }

    #[test]
    fn a_page_defaults_to_a4_with_twelve_millimetre_margins() {
        let doc: Document = serde_json::from_str(r#"{"children":[]}"#).expect("parse");

        assert!((doc.page.width.get() - 595.2756).abs() < 0.01);
        assert!((doc.page.height.get() - 841.8898).abs() < 0.01);
        assert!((doc.page.margin.top.get() - 34.0157).abs() < 0.01);
    }

    #[test]
    fn a_paragraph_keeps_two_lines_at_each_end_unless_told_otherwise() {
        let style = TextStyle::default();

        assert_eq!((style.orphans, style.widows), (2, 2));
    }

    #[test]
    fn an_unknown_node_kind_is_a_named_error_not_a_silent_skip() {
        // A producer emitting something this build does not know must hear
        // about it, not watch it vanish from the page.
        let err = serde_json::from_str::<Node>(r#"{"t":"hologram"}"#)
            .expect_err("an unknown kind must be rejected");

        assert!(err.to_string().contains("hologram"), "{err}");
    }

    #[test]
    fn a_document_round_trips_whole() {
        let doc = Document {
            page: PageSetup::default(),
            accumulators: vec!["debit".into(), "credit".into()],
            children: vec![
                Node::Text(Text {
                    runs: vec![Run::new("Título").bold()],
                    style: TextStyle {
                        size: Pt(18.0),
                        keep_with_next: true,
                        ..Default::default()
                    },
                }),
                Node::Spacer(Spacer { height: Pt(8.0) }),
            ],
            header: None,
            footer: None,
        };

        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();

        assert_eq!(back, doc);
    }

    // ── reading the tag by hand ─────────────────────────────────────────

    #[test]
    fn the_kind_is_read_from_the_t_field() {
        let node: Node = serde_json::from_str(r#"{"t":"text","runs":[{"text":"Hola"}]}"#).unwrap();

        assert_eq!(
            node,
            Node::Text(Text {
                runs: vec![Run::new("Hola")],
                style: TextStyle::default()
            })
        );
    }

    #[test]
    fn the_kind_is_found_wherever_in_the_object_it_sits() {
        // JSON says key order means nothing, so a producer that emits the
        // body first is producing a legal document. The fast path assumes the
        // tag comes first; this is the path that copes when it does not.
        let first: Node = serde_json::from_str(r#"{"t":"spacer","height":12}"#).unwrap();
        let last: Node = serde_json::from_str(r#"{"height":12,"t":"spacer"}"#).unwrap();

        assert_eq!(first, last);
        assert_eq!(first, Node::Spacer(Spacer { height: Pt(12.0) }));
    }

    #[test]
    fn a_tag_that_arrives_last_still_reaches_every_field() {
        // Not just the tag: the buffered body has to survive intact.
        let node: Node = serde_json::from_str(r#"{"src":"logo","width":120,"t":"image"}"#).unwrap();

        assert_eq!(
            node,
            Node::Image(Image {
                src: "logo".into(),
                width: Pt(120.0)
            })
        );
    }

    #[test]
    fn a_nested_document_reads_the_same_whichever_order_the_tags_come_in() {
        let tidy = r#"{"t":"box","children":[{"t":"text","runs":[{"text":"a"}]}]}"#;
        let awkward = r#"{"children":[{"runs":[{"text":"a"}],"t":"text"}],"t":"box"}"#;

        let tidy: Node = serde_json::from_str(tidy).unwrap();
        let awkward: Node = serde_json::from_str(awkward).unwrap();

        assert_eq!(tidy, awkward);
    }

    #[test]
    fn a_kind_the_engine_does_not_have_names_the_ones_it_does() {
        let refused = serde_json::from_str::<Node>(r#"{"t":"marquee"}"#).unwrap_err();

        let message = refused.to_string();
        assert!(message.contains("marquee"), "{message}");
        assert!(message.contains("table"), "{message}");
    }

    #[test]
    fn a_node_with_no_kind_is_refused() {
        assert!(serde_json::from_str::<Node>(r#"{"height":12}"#).is_err());
        assert!(serde_json::from_str::<Node>("{}").is_err());
    }

    #[test]
    fn every_kind_survives_the_round_trip() {
        // The reader is written by hand and the writer is derived, so nothing
        // but this stops the two drifting apart when a variant is added.
        let kinds = vec![
            Node::Text(Text {
                runs: vec![Run::new("a")],
                style: TextStyle::default(),
            }),
            Node::Box(Container::default()),
            Node::Row(Container::default()),
            Node::Table(Table::empty()),
            Node::List(List::default()),
            Node::Image(Image {
                src: "logo".into(),
                width: Pt(10.0),
            }),
            Node::Link(Link {
                href: "https://example.org".into(),
                child: std::boxed::Box::new(Node::Spacer(Spacer { height: Pt(1.0) })),
            }),
            Node::Canvas(Canvas {
                width: Pt(10.0),
                height: Pt(10.0),
                ops: Vec::new(),
                fill: None,
                stroke: None,
                space_after: Pt(0.0),
            }),
            Node::Spacer(Spacer { height: Pt(4.0) }),
            Node::PageBreak(PageBreak { to: BreakTo::Odd }),
        ];

        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let back: Node = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind, "{json}");
        }
    }

    // ── how a marker crosses the wire ───────────────────────────────────

    fn quoted(text: &str) -> String {
        format!("\"{text}\"")
    }

    #[test]
    fn a_marker_with_nothing_to_configure_is_just_its_name() {
        let decimal: Marker = serde_json::from_str(&quoted("decimal")).unwrap();
        let none: Marker = serde_json::from_str(&quoted("none")).unwrap();

        assert_eq!(decimal, Marker::Decimal);
        assert_eq!(none, Marker::None);
        assert_eq!(serde_json::to_string(&decimal).unwrap(), quoted("decimal"));
    }

    #[test]
    fn a_bullet_keeps_its_glyph_when_it_has_one_to_keep() {
        let plain: Marker = serde_json::from_str(&quoted("bullet")).unwrap();
        let dashed: Marker = serde_json::from_str("{\"kind\":\"bullet\",\"glyph\":\"–\"}").unwrap();

        assert_eq!(
            plain,
            Marker::Bullet {
                glyph: "•".into()
            }
        );
        assert_eq!(
            dashed,
            Marker::Bullet {
                glyph: "–".into()
            }
        );
    }

    #[test]
    fn a_bullet_is_written_short_only_when_nothing_is_lost() {
        let plain = Marker::Bullet {
            glyph: "•".into()
        };
        let dashed = Marker::Bullet {
            glyph: "–".into()
        };

        assert_eq!(serde_json::to_string(&plain).unwrap(), quoted("bullet"));
        assert!(serde_json::to_string(&dashed).unwrap().contains("–"));
    }

    #[test]
    fn every_marker_survives_the_round_trip() {
        for marker in [
            Marker::Bullet {
                glyph: "•".into()
            },
            Marker::Bullet {
                glyph: "→".into()
            },
            Marker::Decimal,
            Marker::LowerAlpha,
            Marker::LowerRoman,
            Marker::None,
        ] {
            let json = serde_json::to_string(&marker).unwrap();
            let back: Marker = serde_json::from_str(&json).unwrap();
            assert_eq!(back, marker, "{json}");
        }
    }

    #[test]
    fn a_marker_the_engine_does_not_have_names_the_ones_it_does() {
        let refused = serde_json::from_str::<Marker>(&quoted("emoji")).unwrap_err();

        let message = refused.to_string();
        assert!(message.contains("emoji"), "{message}");
        assert!(message.contains("decimal"), "{message}");
    }
}
