//! The workbook as its author declares it.
//!
//! This is the contract between whatever produced the spreadsheet and the
//! writer, and it is deliberately **not** the PDF IR with different field
//! names. The two models disagree at the root:
//!
//! * A PDF document is measured, paginated and painted here. Every glyph on
//!   the page was decided by the engine.
//! * A spreadsheet has no page and nothing is painted. A cell carries a
//!   **value and a type**, and Excel decides what that looks like when
//!   somebody opens it, in their locale, with their column widths.
//!
//! That inversion is the whole reason for a separate crate. Write `1200` as
//! text into a PDF and you get the glyphs `1200`; write it as text into a
//! spreadsheet and `SUM` returns zero, which is a broken deliverable rather
//! than a cosmetic defect.

use serde::{Deserialize, Serialize};

use crate::style::Style;

/// A workbook: sheets, in the order they will appear as tabs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

impl Workbook {
    pub fn new(sheets: Vec<Sheet>) -> Self {
        Self { sheets }
    }
}

/// One sheet, and everything that is true of it as a whole.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    /// Column widths and defaults, from the left. A column nobody said
    /// anything about is simply absent from here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<Column>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Row>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub merges: Vec<Merge>,
    /// Rows and columns held still while the rest scrolls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeze: Option<Freeze>,
    /// Images floating over the grid, each hung off a cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<Picture>,
}

/// An image hung off a cell.
///
/// The only thing in a workbook that is not a value in the grid. It sits over
/// the sheet rather than in it, anchored to a cell so that inserting a row
/// above carries it down, and it is the one measurement here in points —
/// everything else is characters or row height.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Picture {
    /// The name the bytes were handed over under. The IR carries the name and
    /// never the image, so a workbook can be serialised, cached or put on a
    /// queue without dragging a logo behind it.
    pub image: String,
    /// The cell its top-left corner hangs from. Zero-based, as merges are.
    pub row: u32,
    pub column: u32,
    /// How far into that cell to start, in points.
    #[serde(skip_serializing_if = "is_zero")]
    pub dx: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub dy: f64,
    /// How wide to draw it, in points. The height comes from the image's own
    /// pixels — there is no way to ask for one, because asking for both is
    /// how a logo gets squashed.
    pub width: f64,

    /// Where it sits inside the block it hangs from.
    ///
    /// Centring is the engine's job and cannot be the author's: it needs the
    /// height, and the height comes from the image's own pixels, which only
    /// the engine has read. A producer that worked out an offset itself would
    /// get it right for the logo it had in front of it and wrong for the next
    /// one — silently, because the picture is still on the page.
    #[serde(skip_serializing_if = "Placement::is_start")]
    pub align: Placement,
    #[serde(skip_serializing_if = "Placement::is_start")]
    pub valign: Placement,
}

/// Where a picture sits along one axis of the block it hangs from.
///
/// One enum for both axes because the three positions are the same three, and
/// naming them `left/center/right` and `top/middle/bottom` would be two words
/// for every idea and a conversion between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    /// Against the top-left corner, where a picture goes if nobody says.
    #[default]
    Start,
    Center,
    End,
}

impl Placement {
    pub fn is_start(&self) -> bool {
        matches!(self, Placement::Start)
    }

    /// How far in, given how much room there is and how much is taken.
    pub(crate) fn offset(self, room: f64, taken: f64) -> f64 {
        match self {
            Placement::Start => 0.0,
            Placement::Center => (room - taken) / 2.0,
            Placement::End => room - taken,
        }
        // A picture wider than the block it hangs from would otherwise be
        // pushed off the left edge of the sheet, where it cannot be seen or
        // dragged back.
        .max(0.0)
    }
}

/// Excel's own width unit, in points.
///
/// A column is measured in characters — the width of a `0` in the body font —
/// and everything else in this file is a length. The conversion is Excel's:
/// seven pixels a character plus five for the cell's own padding, at 96 dpi.
pub(crate) fn column_points(chars: f64) -> f64 {
    (chars * 7.0 + 5.0) * 0.75
}

/// What Excel makes a column that nobody described: 8.43 characters.
pub(crate) const DEFAULT_COLUMN: f64 = 8.43;

/// What Excel makes a row that nobody described, in points.
pub(crate) const DEFAULT_ROW: f64 = 15.0;

impl Sheet {
    /// How wide the columns `from..=to` are, in points.
    pub(crate) fn columns_points(&self, from: u32, to: u32) -> f64 {
        (from..=to)
            .map(|at| {
                column_points(
                    self.columns
                        .get(at as usize)
                        .and_then(|column| column.width)
                        .unwrap_or(DEFAULT_COLUMN),
                )
            })
            .sum()
    }

    /// How tall the rows `from..=to` are, in points.
    pub(crate) fn rows_points(&self, from: u32, to: u32) -> f64 {
        (from..=to)
            .map(|at| {
                self.rows
                    .get(at as usize)
                    .and_then(|row| row.height)
                    .unwrap_or(DEFAULT_ROW)
            })
            .sum()
    }

    /// The block a cell belongs to: itself, or the merge that swallowed it.
    ///
    /// A logo hangs off `A1` and the author combined `A1:B4` to make room for
    /// it. Centring in `A1` alone would put it in the top-left corner of the
    /// block the eye actually sees.
    pub(crate) fn block(&self, row: u32, column: u32) -> (u32, u32, u32, u32) {
        self.merges
            .iter()
            .find(|m| {
                m.from_row <= row
                    && row <= m.to_row
                    && m.from_column <= column
                    && column <= m.to_column
            })
            .map(|m| (m.from_row, m.from_column, m.to_row, m.to_column))
            .unwrap_or((row, column, row, column))
    }
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Sheet {
    pub fn new(name: impl Into<String>, rows: Vec<Row>) -> Self {
        Self {
            name: name.into(),
            rows,
            ..Self::default()
        }
    }
}

/// A column's width and the format its cells fall back on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Column {
    /// In Excel's own unit: roughly the width of one digit of the body font.
    /// Not points, not pixels — a column is measured in characters because a
    /// spreadsheet is a grid of text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
}

/// A block of cells shown as one.
///
/// Rows and columns are zero-based and both ends are included, so a merge
/// across two cells of the first row is `(0,0)` to `(0,1)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Merge {
    pub from_row: u32,
    pub from_column: u32,
    pub to_row: u32,
    pub to_column: u32,
}

/// How many rows and columns stay put when the sheet is scrolled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Freeze {
    pub rows: u32,
    pub columns: u32,
}

/// A row of cells.
///
/// Rows are ragged and that is normal: a row with three cells beside one with
/// ten is what every real export looks like. A cell that is not there is
/// **absent**, not empty — Excel tells the two apart and so does `COUNTA`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Row {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Cell>,
    /// In points, as everything vertical is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    /// Whether this row's cells are the labels of an autofilter.
    ///
    /// Marked on the row and not declared as a range, because the range ends
    /// at the last row of the sheet — and for a sheet whose rows are streamed
    /// that is the one thing the author cannot know. The engine works it out
    /// when the sheet closes, which is the only moment it is knowable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub filter: bool,
    /// The format for this row, including the cells it does not have.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
}

impl Row {
    pub fn new(cells: Vec<Cell>) -> Self {
        Self {
            cells,
            ..Self::default()
        }
    }

    /// The same row, formatted.
    pub fn styled(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

/// One cell: what is in it, and how it is formatted.
///
/// The most specific style wins **whole**: a cell's own replaces the row's,
/// which replaces the column's, and nothing is merged property by property.
/// That is how Excel's own format records work, and where an author does want
/// two sets of classes combined — a bold row and a red cell — the combining
/// happens in the producer, which is where `className` composition lives
/// anyway.
///
/// # Why the style is behind a box
///
/// This is the highest-volume type in the crate — a million-row export is ten
/// million of these — so its size is the crate's memory profile. A `Style` is
/// 128 bytes of mostly-absent options, and inline it made a cell 168 bytes
/// whether or not anything had been said about how it looks. Four cells a row
/// was then 672 bytes before a single character of data, and a growing vector
/// hands out about twice what it settles at.
///
/// Boxed, a cell is 48. The trade is one allocation for a cell that **is**
/// styled, and most are not — in a real ledger a handful of formats serve
/// every row, which is the same observation the style table is built on.
/// Measured: 2,380 bytes a row down to about 1,560. `tests/allocations.rs`
/// holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cell {
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<Box<Style>>,
}

impl Cell {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            style: None,
            value: Value::Text(text.into()),
        }
    }

    pub fn number(number: f64) -> Self {
        Self {
            style: None,
            value: Value::Number(number),
        }
    }

    pub fn boolean(yes: bool) -> Self {
        Self {
            style: None,
            value: Value::Bool(yes),
        }
    }

    /// A date, from an Excel serial. See [`crate::serial`] to get one.
    pub fn date(serial: f64) -> Self {
        Self {
            style: None,
            value: Value::Date(serial),
        }
    }

    pub fn formula(formula: impl Into<String>) -> Self {
        Self {
            style: None,
            value: Value::formula(formula),
        }
    }

    /// A formula, and the answer the producer already worked out.
    pub fn formula_worth(formula: impl Into<String>, cached: f64) -> Self {
        Self {
            style: None,
            value: Value::formula_worth(formula, cached),
        }
    }

    pub fn blank() -> Self {
        Self {
            style: None,
            value: Value::Blank,
        }
    }
}

/// What a cell holds, and therefore what Excel will let you do with it.
///
/// The tag is spelled out in JSON rather than inferred from the shape of the
/// value: a producer that means "the text 007" and a producer that means "the
/// number 7" must be able to say so, and a JSON number cannot express the
/// difference on its own.
///
/// # Adjacently tagged, and that turns out to matter
///
/// `{"t":"number","v":1200}` rather than `{"t":"number", …the fields…}`.
/// Internally tagged is what cost the PDF IR sixteen allocations a row, because
/// serde has to buffer the whole map before it knows which variant to build.
/// **Adjacent tagging does not**: with the tag read first, serde reads the
/// content straight into the variant it names.
///
/// That was worth checking rather than assuming. A hand-written `Deserialize`
/// was written here on the strength of the PDF crate's experience and measured
/// at 7.5 allocations a row against the derive's 7.3 — no difference at all —
/// so it was taken out again. The cost was somewhere else entirely; see
/// [`Cell`]. `tests/allocations.rs` is what settled it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum Value {
    /// Nothing in it. Distinct from the empty string, which is a value.
    #[default]
    Blank,
    Text(String),
    Number(f64),
    Bool(bool),

    /// A date, as the Excel serial underneath one. See [`crate::serial`].
    ///
    /// Stored and written exactly like a number, because underneath it is
    /// one. It is a separate variant because it is the only thing that can
    /// tell the writer this cell wants a date number format — without which
    /// it shows as 46237, which is not a date to anybody.
    Date(f64),

    Formula(Formula),
}

impl Value {
    pub fn formula(formula: impl Into<String>) -> Self {
        Self::Formula(Formula {
            formula: formula.into(),
            cached: None,
        })
    }

    /// A formula, and the answer the producer already worked out.
    pub fn formula_worth(formula: impl Into<String>, cached: f64) -> Self {
        Self::Formula(Formula {
            formula: formula.into(),
            cached: Some(cached),
        })
    }
}

/// A formula, and optionally what it comes to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Formula {
    /// Without the leading `=`, which the writer strips if it is there.
    pub formula: String,

    /// What it evaluates to, if the producer already knows.
    ///
    /// Excel recalculates on open and does not need this. Every reader that
    /// only reads — calamine, pandas, openpyxl in its default mode — does: to
    /// them a formula with no cached value is an empty cell. A total that
    /// vanishes when the file is read by a script is a bad surprise, so a
    /// producer that has the number should pass it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_cell_is_not_an_empty_string() {
        // The two look the same on screen and behave differently in every
        // formula that counts or averages.
        assert_ne!(Cell::blank().value, Value::Text(String::new()));
    }

    #[test]
    fn a_workbook_survives_a_json_round_trip() {
        // JSON is how this arrives from React, so a shape that cannot be read
        // back is a shape no producer can write.
        let book = Workbook::new(vec![Sheet::new(
            "Ventas",
            vec![Row::new(vec![Cell::text("Licencia"), Cell::blank()])],
        )]);

        let json = serde_json::to_string(&book).expect("a workbook should serialise");
        let back: Workbook = serde_json::from_str(&json).expect("and read back");

        assert_eq!(back, book);
    }
}
