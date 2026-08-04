//! What a cell looks like, and the table Excel keeps them in.
//!
//! # Why this is a table and not a field
//!
//! A cell does not carry its own formatting. It carries an **index** into
//! `cellXfs`, a list of formats declared once for the whole workbook, and that
//! list in turn indexes lists of fonts, fills, borders and number formats.
//!
//! That is not a quirk to work around; it is the thing that makes a large
//! spreadsheet possible. A hundred thousand rows of a ledger use perhaps six
//! distinct formats between them, so six entries serve ten million cells. Emit
//! a format per cell instead and `styles.xml` becomes larger than the data.
//!
//! So everything here is interned. It is this crate's equivalent of the
//! shaping cache in the PDF engine: the same work asked for repeatedly, done
//! once, and the repetition is not an edge case but the normal shape of the
//! input.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use imprenta_core::color::Color;
use imprenta_core::units::Edges;
use serde::{Deserialize, Serialize};

use crate::xml::escaped;

/// A length in points, hashable.
///
/// Its own type because a style has to be hashable to be interned and `f64` is
/// not. Two sizes are the same size when their bits match, which for a number
/// that came out of a stylesheet rather than out of arithmetic is exactly the
/// right question to ask.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Points(pub f64);

impl PartialEq for Points {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for Points {}
impl Hash for Points {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Everything Excel can be told about one cell's appearance.
///
/// A `Style` that equals [`Style::default`] costs nothing: it interns to index
/// 0, which every workbook has anyway.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Style {
    pub font: Font,
    /// A solid fill. Excel has patterns and gradients; nobody asks for them.
    pub fill: Option<Color>,
    pub border: Edges<Option<Border>>,
    pub align: Alignment,
    /// A number format code, such as `#,##0.00` or `dd/mm/yyyy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

impl Style {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Font {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Points>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// The typeface by name. Nothing is embedded — a spreadsheet asks for a
    /// font and the machine that opens it supplies one or does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Border {
    pub style: Line,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

/// The border widths Excel has.
///
/// An enumeration and not a number, which is the one place a stylesheet and a
/// spreadsheet genuinely cannot agree: `border-2` is two pixels everywhere
/// else and here it is "medium". The mapping is lossy and saying so is better
/// than rounding silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Line {
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
    Double,
}

impl Line {
    fn name(self) -> &'static str {
        match self {
            Line::Thin => "thin",
            Line::Medium => "medium",
            Line::Thick => "thick",
            Line::Dashed => "dashed",
            Line::Dotted => "dotted",
            Line::Double => "double",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Alignment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal: Option<Across>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical: Option<Down>,
    /// Wrap long text onto more lines inside the cell.
    pub wrap: bool,
    /// Excel's indent, in units of about three characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent: Option<u32>,
}

impl Alignment {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Across {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Down {
    Top,
    Middle,
    Bottom,
}

/// A list that gives the same thing the same index every time.
#[derive(Debug)]
struct Interner<T> {
    index: HashMap<T, u32>,
    order: Vec<T>,
}

/// Written out rather than derived: a derive would demand `T: Default`, which
/// an interned entry has no reason to be. An empty table is empty whatever it
/// is a table of.
impl<T> Default for Interner<T> {
    fn default() -> Self {
        Self {
            index: HashMap::new(),
            order: Vec::new(),
        }
    }
}

impl<T: Clone + Eq + Hash> Interner<T> {
    fn intern(&mut self, value: &T) -> u32 {
        if let Some(&at) = self.index.get(value) {
            return at;
        }
        let at = self.order.len() as u32;
        self.order.push(value.clone());
        self.index.insert(value.clone(), at);
        at
    }

    fn len(&self) -> usize {
        self.order.len()
    }
}

/// One entry of `cellXfs`: the combination a cell actually points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Xf {
    font: u32,
    fill: u32,
    border: u32,
    format: u32,
    align: Alignment,
}

/// Every format the workbook uses, each declared once.
#[derive(Debug)]
pub struct Styles {
    fonts: Interner<Font>,
    fills: Interner<Option<Color>>,
    borders: Interner<Edges<Option<Border>>>,
    formats: Interner<String>,
    xfs: Interner<Xf>,
}

/// Where Excel's own number formats stop and a file's own may begin.
const FIRST_CUSTOM_FORMAT: u32 = 164;

impl Default for Styles {
    fn default() -> Self {
        Self::new()
    }
}

impl Styles {
    pub fn new() -> Self {
        let mut styles = Self {
            fonts: Interner::default(),
            fills: Interner::default(),
            borders: Interner::default(),
            formats: Interner::default(),
            xfs: Interner::default(),
        };

        // Index 0 of each list has to be the empty one, because a cell that
        // says nothing about its format points at 0 and must get nothing.
        styles.fonts.intern(&Font::default());
        styles.fills.intern(&None);
        // And fill 1 has to be `gray125`. Nothing will ever use it, but Excel
        // indexes fills from a table it assumes begins with these two, and a
        // file that omits it comes back with every fill shifted by one.
        styles.fills.intern(&Some(GRAY125));
        styles.borders.intern(&Edges::default());
        styles.xfs.intern(&Xf {
            font: 0,
            fill: 0,
            border: 0,
            format: 0,
            align: Alignment::default(),
        });

        styles
    }

    /// The `cellXfs` index for a style, adding it to the table if it is new.
    pub fn intern(&mut self, style: &Style) -> u32 {
        if style.is_default() {
            return 0;
        }

        let xf = Xf {
            font: self.fonts.intern(&style.font),
            // Fill 1 is reserved for gray125, so a solid fill can never be 1.
            fill: match &style.fill {
                None => 0,
                Some(color) => self.fills.intern(&Some(*color)),
            },
            border: self.borders.intern(&style.border),
            format: match &style.format {
                None => 0,
                Some(code) => self.number_format(code),
            },
            align: style.align.clone(),
        };

        self.xfs.intern(&xf)
    }

    /// How many distinct cell formats the workbook has. For tests, and worth
    /// looking at: a number in the thousands means something is not interning.
    pub fn len(&self) -> usize {
        self.xfs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.xfs.len() <= 1
    }

    /// The id for a number format code, built in where Excel has one.
    ///
    /// Excel knows a couple of dozen formats by number, and using those means
    /// the file says "the local currency format" rather than pinning a comma
    /// and a euro sign that will be wrong in another locale.
    fn number_format(&mut self, code: &str) -> u32 {
        if let Some(builtin) = builtin_format(code) {
            return builtin;
        }
        FIRST_CUSTOM_FORMAT + self.formats.intern(&code.to_string())
    }

    pub fn to_xml(&self) -> String {
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        );

        if self.formats.len() > 0 {
            xml.push_str(&format!(r#"<numFmts count="{}">"#, self.formats.len()));
            for (at, code) in self.formats.order.iter().enumerate() {
                xml.push_str(&format!(
                    r#"<numFmt numFmtId="{}" formatCode="{}"/>"#,
                    FIRST_CUSTOM_FORMAT + at as u32,
                    escaped(code)
                ));
            }
            xml.push_str("</numFmts>");
        }

        xml.push_str(&format!(r#"<fonts count="{}">"#, self.fonts.len()));
        for font in &self.fonts.order {
            xml.push_str(&font_xml(font));
        }
        xml.push_str("</fonts>");

        xml.push_str(&format!(r#"<fills count="{}">"#, self.fills.len()));
        for (at, fill) in self.fills.order.iter().enumerate() {
            xml.push_str(&fill_xml(fill.as_ref(), at));
        }
        xml.push_str("</fills>");

        xml.push_str(&format!(r#"<borders count="{}">"#, self.borders.len()));
        for border in &self.borders.order {
            xml.push_str(&border_xml(border));
        }
        xml.push_str("</borders>");

        xml.push_str(
            r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
        );

        xml.push_str(&format!(r#"<cellXfs count="{}">"#, self.xfs.len()));
        for xf in &self.xfs.order {
            xml.push_str(&xf_xml(xf));
        }
        xml.push_str("</cellXfs>");

        xml.push_str(
            r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
        );
        xml.push_str("</styleSheet>");
        xml
    }
}

/// The pattern Excel insists on having at fill index 1.
const GRAY125: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};

/// Excel's own number formats, by the code they correspond to.
///
/// Only the ones worth having: a date, a time, a percentage, a thousands
/// separator. Anything else becomes a custom format, which is fine — there is
/// no penalty beyond a line in `numFmts`.
fn builtin_format(code: &str) -> Option<u32> {
    Some(match code {
        "General" => 0,
        "0" => 1,
        "0.00" => 2,
        "#,##0" => 3,
        "#,##0.00" => 4,
        "0%" => 9,
        "0.00%" => 10,
        "mm-dd-yy" => 14,
        "d-mmm-yy" => 15,
        "d-mmm" => 16,
        "mmm-yy" => 17,
        "h:mm AM/PM" => 18,
        "h:mm:ss AM/PM" => 19,
        "h:mm" => 20,
        "h:mm:ss" => 21,
        "m/d/yy h:mm" => 22,
        "@" => 49,
        _ => return None,
    })
}

/// Excel writes colours as `AARRGGBB`, alpha first.
fn argb(color: &Color) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}",
        color.a, color.r, color.g, color.b
    )
}

fn font_xml(font: &Font) -> String {
    let mut xml = String::from("<font>");
    if font.bold {
        xml.push_str("<b/>");
    }
    if font.italic {
        xml.push_str("<i/>");
    }
    if font.underline {
        xml.push_str("<u/>");
    }
    if font.strike {
        xml.push_str("<strike/>");
    }
    // A size and a name are written always, defaults included: Excel takes a
    // font with neither as a reason to apply whatever the theme says, which
    // is not the same as the eleven-point Calibri everybody assumes.
    xml.push_str(&format!(
        r#"<sz val="{}"/>"#,
        font.size.map(|s| s.0).unwrap_or(11.0)
    ));
    if let Some(color) = &font.color {
        xml.push_str(&format!(r#"<color rgb="{}"/>"#, argb(color)));
    }
    xml.push_str(&format!(
        r#"<name val="{}"/>"#,
        escaped(font.name.as_deref().unwrap_or("Calibri"))
    ));
    xml.push_str("</font>");
    xml
}

fn fill_xml(color: Option<&Color>, at: usize) -> String {
    match (at, color) {
        (0, _) => r#"<fill><patternFill patternType="none"/></fill>"#.to_string(),
        (1, _) => r#"<fill><patternFill patternType="gray125"/></fill>"#.to_string(),
        (_, None) => r#"<fill><patternFill patternType="none"/></fill>"#.to_string(),
        (_, Some(color)) => format!(
            r#"<fill><patternFill patternType="solid"><fgColor rgb="{}"/><bgColor indexed="64"/></patternFill></fill>"#,
            argb(color)
        ),
    }
}

fn border_xml(border: &Edges<Option<Border>>) -> String {
    // The order is the one the schema fixes — left, right, top, bottom,
    // diagonal — and not the CSS one. Getting it wrong puts the top rule on
    // the left, which looks like a layout bug and is a spelling mistake.
    let mut xml = String::from("<border>");
    for (name, side) in [
        ("left", &border.left),
        ("right", &border.right),
        ("top", &border.top),
        ("bottom", &border.bottom),
    ] {
        match side {
            None => xml.push_str(&format!("<{name}/>")),
            Some(Border { style, color }) => {
                xml.push_str(&format!(r#"<{name} style="{}">"#, style.name()));
                if let Some(color) = color {
                    xml.push_str(&format!(r#"<color rgb="{}"/>"#, argb(color)));
                }
                xml.push_str(&format!("</{name}>"));
            }
        }
    }
    xml.push_str("<diagonal/></border>");
    xml
}

fn xf_xml(xf: &Xf) -> String {
    let mut attributes = format!(
        r#"numFmtId="{}" fontId="{}" fillId="{}" borderId="{}" xfId="0""#,
        xf.format, xf.font, xf.fill, xf.border
    );
    // Excel ignores a font, fill, border or format on an xf unless the
    // matching `applyX` says to look at it. Silently ignores — the file is
    // valid and the formatting is simply absent.
    if xf.format != 0 {
        attributes.push_str(r#" applyNumberFormat="1""#);
    }
    if xf.font != 0 {
        attributes.push_str(r#" applyFont="1""#);
    }
    if xf.fill != 0 {
        attributes.push_str(r#" applyFill="1""#);
    }
    if xf.border != 0 {
        attributes.push_str(r#" applyBorder="1""#);
    }
    if xf.align.is_default() {
        return format!("<xf {attributes}/>");
    }

    let mut alignment = String::new();
    if let Some(across) = xf.align.horizontal {
        let name = match across {
            Across::Left => "left",
            Across::Center => "center",
            Across::Right => "right",
            Across::Justify => "justify",
        };
        alignment.push_str(&format!(r#" horizontal="{name}""#));
    }
    if let Some(down) = xf.align.vertical {
        let name = match down {
            Down::Top => "top",
            Down::Middle => "center",
            Down::Bottom => "bottom",
        };
        alignment.push_str(&format!(r#" vertical="{name}""#));
    }
    if xf.align.wrap {
        alignment.push_str(r#" wrapText="1""#);
    }
    if let Some(indent) = xf.align.indent {
        alignment.push_str(&format!(r#" indent="{indent}""#));
    }

    format!(r#"<xf {attributes} applyAlignment="1"><alignment{alignment}/></xf>"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color {
        r: 220,
        g: 38,
        b: 38,
        a: 255,
    };
    const SLATE: Color = Color {
        r: 241,
        g: 245,
        b: 249,
        a: 255,
    };

    fn bold() -> Style {
        Style {
            font: Font {
                bold: true,
                ..Font::default()
            },
            ..Style::default()
        }
    }

    #[test]
    fn a_cell_that_asks_for_nothing_gets_index_zero() {
        // Which every workbook has anyway, so an unstyled sheet adds nothing
        // at all to styles.xml.
        let mut styles = Styles::new();
        assert_eq!(styles.intern(&Style::default()), 0);
        assert_eq!(styles.len(), 1);
    }

    #[test]
    fn the_same_style_asked_for_a_hundred_thousand_times_is_one_entry() {
        // The property the whole module exists for. A ledger is one format
        // repeated down a column; if this ever regresses, styles.xml grows
        // past the data and Excel takes minutes to open the file.
        let mut styles = Styles::new();
        let first = styles.intern(&bold());
        for _ in 0..100_000 {
            assert_eq!(styles.intern(&bold()), first);
        }
        // The default, plus bold. Nothing else.
        assert_eq!(styles.len(), 2);
    }

    #[test]
    fn styles_that_differ_in_one_thing_are_different_entries() {
        let mut styles = Styles::new();
        let plain_bold = styles.intern(&bold());

        let mut red_bold = bold();
        red_bold.font.color = Some(RED);

        assert_ne!(styles.intern(&red_bold), plain_bold);
        assert_eq!(styles.len(), 3);
    }

    #[test]
    fn a_font_and_a_fill_are_shared_between_the_formats_that_use_them() {
        // Two formats that are bold, one of them also filled: one font entry
        // between them, not two. The tables nest, and that is where the
        // saving compounds on a real sheet.
        let mut styles = Styles::new();
        styles.intern(&bold());
        let mut filled = bold();
        filled.fill = Some(SLATE);
        styles.intern(&filled);

        assert_eq!(styles.fonts.len(), 2, "the default and one bold");
        assert_eq!(styles.xfs.len(), 3, "the default and two combinations");
    }

    #[test]
    fn a_solid_fill_never_lands_on_the_index_excel_reserves() {
        // Fill 1 is gray125 whatever anybody wanted. A solid colour landing
        // there would come back as a hatch pattern.
        let mut styles = Styles::new();
        let filled = Style {
            fill: Some(SLATE),
            ..Style::default()
        };
        styles.intern(&filled);

        assert!(styles.to_xml().contains(r#"patternType="gray125""#));
        assert!(styles.to_xml().contains(&argb(&SLATE)));
    }

    #[test]
    fn a_format_excel_already_knows_is_referred_to_by_its_number() {
        // Not redeclared: `#,##0.00` as builtin 4 means the file asks for the
        // reader's own thousands separator rather than pinning a comma that
        // is a full stop in half of Europe.
        let mut styles = Styles::new();
        styles.intern(&Style {
            format: Some("#,##0.00".into()),
            ..Style::default()
        });

        let xml = styles.to_xml();
        assert!(xml.contains(r#"numFmtId="4""#), "{xml}");
        assert!(!xml.contains("<numFmts"), "a built-in needs no declaration");
    }

    #[test]
    fn a_format_of_its_own_is_declared_above_the_reserved_range() {
        let mut styles = Styles::new();
        styles.intern(&Style {
            format: Some("#,##0.00 €".into()),
            ..Style::default()
        });

        let xml = styles.to_xml();
        assert!(xml.contains(r#"numFmtId="164""#), "{xml}");
        assert!(xml.contains("#,##0.00 €"));
    }

    #[test]
    fn every_applied_part_says_it_is_applied() {
        // Excel reads a font, fill, border or number format off an xf only if
        // the matching applyX flag is set, and says nothing when it is not.
        let mut styles = Styles::new();
        styles.intern(&Style {
            font: Font {
                bold: true,
                ..Font::default()
            },
            fill: Some(SLATE),
            border: Edges {
                top: Some(Border {
                    style: Line::Thin,
                    color: None,
                }),
                ..Edges::default()
            },
            format: Some("0.00".into()),
            align: Alignment {
                horizontal: Some(Across::Right),
                ..Alignment::default()
            },
        });

        let xml = styles.to_xml();
        for flag in [
            "applyFont",
            "applyFill",
            "applyBorder",
            "applyNumberFormat",
            "applyAlignment",
        ] {
            assert!(xml.contains(flag), "{flag} is missing from {xml}");
        }
    }

    #[test]
    fn borders_are_written_in_the_order_the_schema_fixes() {
        // Left, right, top, bottom — not the CSS order. A top rule that comes
        // out on the left looks like a layout bug and is a spelling mistake.
        let border = Edges {
            top: Some(Border {
                style: Line::Thick,
                color: None,
            }),
            ..Edges::default()
        };
        let xml = border_xml(&border);
        let left = xml.find("<left").expect("a left side");
        let top = xml.find("<top").expect("a top side");
        assert!(left < top, "{xml}");
        assert!(xml.contains(r#"<top style="thick">"#), "{xml}");
    }

    #[test]
    fn a_colour_is_written_with_alpha_first() {
        // AARRGGBB, which is not the order anybody writes a colour in.
        assert_eq!(argb(&RED), "FFDC2626");
    }

    #[test]
    fn a_style_survives_a_json_round_trip() {
        let style = Style {
            font: Font {
                bold: true,
                size: Some(Points(10.5)),
                color: Some(RED),
                ..Font::default()
            },
            fill: Some(SLATE),
            align: Alignment {
                horizontal: Some(Across::Right),
                wrap: true,
                ..Alignment::default()
            },
            format: Some("#,##0.00".into()),
            ..Style::default()
        };

        let json = serde_json::to_string(&style).expect("a style should serialise");
        let back: Style = serde_json::from_str(&json).expect("and read back");

        assert_eq!(back, style);
    }
}
