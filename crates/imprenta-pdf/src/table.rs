//! Column geometry.
//!
//! # This is mechanism, not a component
//!
//! There is no styled `Table` here and there never will be. A table in this
//! engine is column widths, cell placement, and the rules for splitting rows
//! across pages — all of it invisible. Every colour, rule, padding and weight
//! arrives as a [`crate::decoration::Decoration`] the caller supplies per row
//! or per cell, so a table looks like whatever the author asked for and never
//! like something the engine chose.
//!
//! That separation is what lets the React layer offer unstyled `<Table>`,
//! `<Row>` and `<Cell>` components styled entirely through `className` or
//! `style`, and lets anyone who dislikes them build their own out of
//! [`crate::content::BoxContent::place`] — which is all this module does
//! anyway. Nothing here is privileged; it is a convenience over primitives
//! that stay public.

use crate::content::{BoxContent, Content};
use crate::decoration::Decoration;
use crate::measure::{Measured, TextStyle, measure_text_in};
use crate::shape::{Face, Shaper, report_missing};
use imprenta_core::color::Color;
use imprenta_core::diagnostic::{Diagnostic, Diagnostics};
use imprenta_core::units::{Edges, Length, Pt};

/// Where a cell's content sits within its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// The inline start — left in a left-to-right script, right in Arabic.
    #[default]
    Start,
    End,
    Center,
}

/// What a cell does when its content is wider than its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Break onto more lines and let the row grow. The sane default: a
    /// document that reads correctly beats one that fits.
    #[default]
    Wrap,
    /// Trim to the column width, measured, with an ellipsis.
    Ellipsis,
    /// Trim with no marker.
    Clip,
}

/// One column of a table.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Column {
    pub width: Length,
    pub align: Align,
    pub overflow: Overflow,
}

impl Column {
    pub fn new(width: Length) -> Self {
        Self {
            width,
            ..Default::default()
        }
    }

    pub fn aligned(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn overflowing(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }
}

/// A column's resolved position and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
    pub x: Pt,
    pub width: Pt,
}

/// Resolves column widths against the space available to the table.
///
/// Absolute widths are taken as given, percentages resolve against
/// `available`, and [`Length::Auto`] columns share whatever is left in equal
/// parts. Measured auto widths — sizing a column to its widest cell — are a
/// later addition: they need the whole table before deciding, which is at
/// odds with streaming, so §11.3.1 of the design notes settles them by
/// sampling rather than by measuring everything.
pub fn resolve(columns: &[Column], available: Pt) -> Vec<Track> {
    // Everything that declares a width takes it first; auto columns divide
    // what survives.
    let declared: f32 = columns
        .iter()
        .filter_map(|c| c.width.resolve(available))
        .map(Pt::get)
        .sum();

    let auto_count = columns
        .iter()
        .filter(|c| c.width.resolve(available).is_none())
        .count();

    // Clamped at zero: over-wide declared columns are the author's problem,
    // but a negative track would place later cells left of earlier ones.
    let each_auto = if auto_count == 0 {
        0.0
    } else {
        (available.get() - declared).max(0.0) / auto_count as f32
    };

    let mut x = Pt(0.0);
    columns
        .iter()
        .map(|column| {
            let width = column.width.resolve(available).unwrap_or(Pt(each_auto));
            let track = Track { x, width };
            x = x + width;
            track
        })
        .collect()
}

/// Where content of `content_width` starts inside a track.
pub fn offset_within(track: Track, content_width: Pt, align: Align) -> Pt {
    // Slack is clamped at zero so content wider than its column spills to the
    // right, into empty space, rather than to the left over its neighbour.
    let slack = (track.width.get() - content_width.get()).max(0.0);
    let shift = match align {
        Align::Start => 0.0,
        Align::End => slack,
        Align::Center => slack / 2.0,
    };
    track.x + Pt(shift)
}

/// One cell's text and the ink it is set in.
///
/// Style travels with the cell rather than with the column so that a single
/// row can be emphasised without redefining the table — a total line in bold
/// navy is the same table as the rows above it.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub size: Pt,
    pub color: Color,
    pub face: Face,
}

impl Cell {
    pub fn new(text: impl Into<String>, size: Pt) -> Self {
        Self {
            text: text.into(),
            size,
            color: Color::BLACK,
            face: Face::REGULAR,
        }
    }

    pub fn in_face(mut self, face: Face) -> Self {
        self.face = face;
        self
    }

    pub fn bold(self) -> Self {
        self.in_face(Face::BOLD)
    }

    pub fn inked(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

/// A table's columns, resolved against the width they were offered.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub columns: Vec<Column>,
    pub tracks: Vec<Track>,
}

impl Layout {
    /// `content_width` is the space inside the row's padding, not the page.
    pub fn new(columns: Vec<Column>, content_width: Pt) -> Self {
        let tracks = resolve(&columns, content_width);
        Self { columns, tracks }
    }

    /// Lays `cells` into their columns and wraps them in a decorated box.
    ///
    /// The row is one atom: the box contains its cells, so the text is
    /// painted over its own background rather than under the next row's.
    pub fn row(
        &self,
        shaper: &mut Shaper,
        cells: &[Cell],
        decoration: Decoration,
        padding: Edges<Pt>,
    ) -> BoxContent {
        self.row_reporting(
            shaper,
            cells,
            decoration,
            padding,
            &mut Diagnostics::default(),
        )
    }

    /// As [`Self::row`], but reporting what went wrong.
    ///
    /// Clipped text and overflowing cells are defects the engine can see and
    /// the reader cannot: the page looks deliberate. Reported here, they
    /// reach the dev server and CI instead of a customer.
    pub fn row_reporting(
        &self,
        shaper: &mut Shaper,
        cells: &[Cell],
        decoration: Decoration,
        padding: Edges<Pt>,
        diagnostics: &mut Diagnostics,
    ) -> BoxContent {
        let mut row = BoxContent::new(decoration).with_padding(padding);

        // A cell with no column is dropped rather than piled onto its
        // neighbour: a missing cell is visible, an overlapping one is not.
        for ((cell, track), column) in cells.iter().zip(&self.tracks).zip(&self.columns) {
            row = row.place(
                track.x,
                self.cell(shaper, cell, *track, *column, diagnostics),
            );
        }

        if cells.len() > self.tracks.len() {
            diagnostics.report(
                Diagnostic::error(
                    "cell-without-column",
                    format!(
                        "{} cells were given but the table declares {} columns",
                        cells.len(),
                        self.tracks.len()
                    ),
                )
                .with_hint("the extra cells are dropped, not stacked"),
            );
        }
        row
    }

    /// One cell as its own box.
    ///
    /// A box rather than bare lines because a cell that wraps has several of
    /// them, and they must stack inside the cell rather than spread across
    /// the row. It also means an empty cell still holds its column instead of
    /// vanishing and shunting every cell after it one place to the left.
    fn cell(
        &self,
        shaper: &mut Shaper,
        cell: &Cell,
        track: Track,
        column: Column,
        diagnostics: &mut Diagnostics,
    ) -> Content {
        report_missing(shaper, &cell.text, cell.face, diagnostics);

        let natural = shaper.shape_in(&cell.text, cell.face).width_at(cell.size);
        let overflows = natural.get() > track.width.get() + 0.01;

        let measured = match column.overflow {
            Overflow::Wrap => measure_text_in(
                shaper,
                &cell.text,
                TextStyle::new(cell.size),
                track.width,
                cell.face,
            ),
            Overflow::Ellipsis => truncate(shaper, cell, track.width, "…"),
            Overflow::Clip => truncate(shaper, cell, track.width, ""),
        };

        if overflows {
            match column.overflow {
                Overflow::Ellipsis | Overflow::Clip => diagnostics.report(
                    Diagnostic::warning(
                        "text-clipped",
                        format!("{:?} was cut to fit its column", short(&cell.text)),
                    )
                    .with_hint("widen the column or use overflow: wrap"),
                ),
                // A single word with nowhere to break spills over its
                // neighbour; wrapped text that merely took extra lines has
                // not gone wrong.
                Overflow::Wrap => {
                    let widest = measured
                        .lines
                        .iter()
                        .map(|l| l.width.get())
                        .fold(0.0f32, f32::max);
                    if widest > track.width.get() + 0.01 {
                        diagnostics.report(
                            Diagnostic::warning(
                                "cell-overflow",
                                format!(
                                    "{:?} overflows its column by {:.1}pt with nowhere to break",
                                    short(&cell.text),
                                    widest - track.width.get()
                                ),
                            )
                            .with_hint("widen the column or shorten the value"),
                        );
                    }
                }
            }
        }
        if measured.lines.is_empty() {
            return Content::Empty;
        }

        // Offsets inside the cell are relative to the cell's own corner, so
        // alignment resolves against a track anchored at zero.
        let local = Track {
            x: Pt(0.0),
            width: track.width,
        };
        let mut boxed = BoxContent::default().with_width(track.width);
        for line in measured.lines {
            let x = offset_within(local, line.width, column.align);
            boxed = boxed.stack_at(x, Content::Text(line.with_color(cell.color)));
        }
        Content::Box(boxed)
    }
}

/// The first few characters of a value, for an error message.
fn short(text: &str) -> String {
    let cut: String = text.chars().take(24).collect();
    if cut.chars().count() < text.chars().count() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// Cuts `cell` down to one line that fits `width`, ending with `marker`.
///
/// Measured, not counted. Trimming to a fixed number of characters — which is
/// what a template does when the engine cannot measure — cuts "IIIIIIIIII"
/// and "WWWWWWWWWW" at the same place, though one is half the width of the
/// other. The result is text lost early in some rows and overflowing in
/// others, and no way to tell which.
fn truncate(shaper: &mut Shaper, cell: &Cell, width: Pt, marker: &str) -> Measured {
    let style = TextStyle::new(cell.size);
    let whole = shaper.shape_in(&cell.text, cell.face);
    if whole.width_at(cell.size).get() <= width.get() {
        return measure_text_in(shaper, &cell.text, style, width, cell.face);
    }

    let marker_width = shaper.shape_in(marker, cell.face).width_at(cell.size).get();
    let budget = width.get() - marker_width;

    // Walk the glyphs, keeping the source offset of the last one that still
    // fits. Glyph advances rather than characters, so a wide letter costs
    // what it actually costs.
    let mut used = 0.0f32;
    let mut cut = 0usize;
    for glyph in &whole.glyphs {
        let next = used + glyph.x_advance * cell.size.get();
        if next > budget {
            break;
        }
        used = next;
        cut = glyph.text_range.end as usize;
    }

    let mut text = cell.text[..cut.min(cell.text.len())].trim_end().to_string();
    text.push_str(marker);

    // Re-shaped rather than spliced: the marker kerns against whatever
    // letter it now follows.
    measure_text_in(shaper, &text, style, Pt(f32::INFINITY), cell.face)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_close(actual: Pt, expected: Pt) {
        assert!(
            (actual.get() - expected.get()).abs() < 0.01,
            "expected {expected:?}, got {actual:?}"
        );
    }

    fn fixed(pt: f32) -> Column {
        Column::new(Length::Pt(Pt(pt)))
    }

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn shaper() -> Shaper {
        Shaper::new(ROBOTO.to_vec())
    }

    /// The x of each cell, in the order they were added.
    fn xs(row: &BoxContent) -> Vec<f32> {
        row.children.iter().map(|c| c.x.get()).collect()
    }

    /// Absolute x of the first line of text inside cell `i`.
    ///
    /// A cell is a box of its own, so alignment shows up as an offset inside
    /// that box rather than on the row. What matters is where the ink lands.
    fn ink_x(row: &BoxContent, i: usize) -> Pt {
        let cell = &row.children[i];
        match &cell.content {
            Content::Box(b) => cell.x + b.children[0].x,
            _ => cell.x,
        }
    }

    fn first_line(row: &BoxContent, i: usize) -> &crate::shape::Line {
        match &row.children[i].content {
            Content::Box(b) => match &b.children[0].content {
                Content::Text(l) => l,
                other => panic!("expected text in the cell, got {other:?}"),
            },
            other => panic!("expected a cell box, got {other:?}"),
        }
    }

    #[test]
    fn no_columns_resolve_to_no_tracks() {
        assert!(resolve(&[], Pt(500.0)).is_empty());
    }

    #[test]
    fn absolute_widths_are_taken_as_given() {
        let tracks = resolve(&[fixed(100.0), fixed(60.0)], Pt(500.0));

        assert_close(tracks[0].width, Pt(100.0));
        assert_close(tracks[1].width, Pt(60.0));
    }

    #[test]
    fn tracks_are_laid_left_to_right_from_zero() {
        let tracks = resolve(&[fixed(100.0), fixed(60.0), fixed(40.0)], Pt(500.0));

        assert_close(tracks[0].x, Pt(0.0));
        assert_close(tracks[1].x, Pt(100.0));
        assert_close(tracks[2].x, Pt(160.0));
    }

    #[test]
    fn percentages_resolve_against_the_space_offered() {
        // The ledger's column widths are written as percentages.
        let columns = [
            Column::new(Length::Percent(0.11)),
            Column::new(Length::Percent(0.21)),
        ];

        let tracks = resolve(&columns, Pt(500.0));

        assert_close(tracks[0].width, Pt(55.0));
        assert_close(tracks[1].width, Pt(105.0));
    }

    #[test]
    fn auto_columns_share_what_the_others_leave() {
        let columns = [
            fixed(100.0),
            Column::new(Length::Auto),
            Column::new(Length::Auto),
        ];

        let tracks = resolve(&columns, Pt(500.0));

        assert_close(tracks[1].width, Pt(200.0));
        assert_close(tracks[2].width, Pt(200.0));
    }

    #[test]
    fn a_lone_auto_column_takes_the_whole_remainder() {
        let tracks = resolve(&[fixed(120.0), Column::new(Length::Auto)], Pt(500.0));

        assert_close(tracks[1].width, Pt(380.0));
    }

    #[test]
    fn an_auto_column_never_goes_negative_when_the_others_overrun() {
        // Over-wide fixed columns are the author's problem, but a negative
        // track would place later cells to the left of earlier ones.
        let tracks = resolve(
            &[fixed(400.0), fixed(400.0), Column::new(Length::Auto)],
            Pt(500.0),
        );

        assert_close(tracks[2].width, Pt(0.0));
    }

    #[test]
    fn mixed_units_resolve_together() {
        let columns = [
            fixed(100.0),
            Column::new(Length::Percent(0.2)),
            Column::new(Length::Auto),
        ];

        let tracks = resolve(&columns, Pt(500.0));

        assert_close(tracks[0].width, Pt(100.0));
        assert_close(tracks[1].width, Pt(100.0));
        assert_close(tracks[2].width, Pt(300.0));
    }

    // ── alignment ───────────────────────────────────────────────────────

    #[test]
    fn start_aligned_content_begins_at_the_track() {
        let track = Track {
            x: Pt(80.0),
            width: Pt(100.0),
        };

        assert_close(offset_within(track, Pt(30.0), Align::Start), Pt(80.0));
    }

    #[test]
    fn end_aligned_content_finishes_at_the_track_edge() {
        // Every amount column in every ledger.
        let track = Track {
            x: Pt(80.0),
            width: Pt(100.0),
        };

        assert_close(offset_within(track, Pt(30.0), Align::End), Pt(150.0));
    }

    #[test]
    fn centred_content_leaves_equal_room_on_both_sides() {
        let track = Track {
            x: Pt(80.0),
            width: Pt(100.0),
        };

        assert_close(offset_within(track, Pt(30.0), Align::Center), Pt(115.0));
    }

    #[test]
    fn content_wider_than_its_track_still_starts_inside_it() {
        // Overflow spills to the right rather than to the left, where it
        // would collide with the previous column.
        let track = Track {
            x: Pt(80.0),
            width: Pt(20.0),
        };

        for align in [Align::Start, Align::End, Align::Center] {
            let x = offset_within(track, Pt(100.0), align);
            assert!(
                x.get() >= track.x.get() - 0.01,
                "{align:?} pushed content left of its column"
            );
        }
    }

    // ── rows ────────────────────────────────────────────────────────────

    #[test]
    fn a_row_places_each_cell_in_its_own_column() {
        let layout = Layout::new(vec![fixed(100.0), fixed(100.0), fixed(100.0)], Pt(300.0));
        let cells = [
            Cell::new("430000", Pt(8.0)),
            Cell::new("Cliente", Pt(8.0)),
            Cell::new("FV-1", Pt(8.0)),
        ];

        let row = layout.row(
            &mut shaper(),
            &cells,
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(xs(&row), vec![0.0, 100.0, 200.0]);
    }

    #[test]
    fn a_row_keeps_its_cells_in_the_order_they_were_given() {
        let layout = Layout::new(vec![fixed(100.0), fixed(100.0)], Pt(200.0));
        let cells = [Cell::new("primero", Pt(8.0)), Cell::new("segundo", Pt(8.0))];

        let row = layout.row(
            &mut shaper(),
            &cells,
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(&*first_line(&row, 0).text, "primero");
        assert_eq!(&*first_line(&row, 1).text, "segundo");
    }

    #[test]
    fn an_end_aligned_cell_finishes_at_its_column_edge() {
        // Every amount column in every ledger.
        let layout = Layout::new(
            vec![fixed(100.0), fixed(100.0).aligned(Align::End)],
            Pt(200.0),
        );
        let mut s = shaper();
        let width = s.shape("1.234,56").width_at(Pt(8.0));

        let row = layout.row(
            &mut s,
            &[Cell::new("", Pt(8.0)), Cell::new("1.234,56", Pt(8.0))],
            Decoration::default(),
            Edges::default(),
        );

        assert_close(ink_x(&row, 1), Pt(200.0) - width);
    }

    #[test]
    fn a_centred_cell_sits_in_the_middle_of_its_column() {
        let layout = Layout::new(vec![fixed(100.0).aligned(Align::Center)], Pt(100.0));
        let mut s = shaper();
        let width = s.shape("x").width_at(Pt(8.0));

        let row = layout.row(
            &mut s,
            &[Cell::new("x", Pt(8.0))],
            Decoration::default(),
            Edges::default(),
        );

        assert_close(ink_x(&row, 0), Pt((100.0 - width.get()) / 2.0));
    }

    #[test]
    fn a_row_is_as_tall_as_its_tallest_cell() {
        let layout = Layout::new(vec![fixed(200.0), fixed(200.0)], Pt(400.0));
        let mut s = shaper();
        let tall = s.break_lines("grande", Pt(16.0), Pt(200.0))[0].height;

        let row = layout.row(
            &mut s,
            &[Cell::new("pequeño", Pt(8.0)), Cell::new("grande", Pt(16.0))],
            Decoration::default(),
            Edges::default(),
        );

        assert_close(row.height(), tall);
    }

    #[test]
    fn a_cell_wider_than_its_column_wraps_and_the_row_grows() {
        // The behaviour that replaces truncating by character count.
        let narrow = Layout::new(vec![fixed(60.0)], Pt(60.0));
        let wide = Layout::new(vec![fixed(400.0)], Pt(400.0));
        let text = "Prestación de servicios profesionales periodo tres";

        let mut s = shaper();
        let tall = narrow.row(
            &mut s,
            &[Cell::new(text, Pt(8.0))],
            Decoration::default(),
            Edges::default(),
        );
        let short = wide.row(
            &mut s,
            &[Cell::new(text, Pt(8.0))],
            Decoration::default(),
            Edges::default(),
        );

        assert!(
            tall.height().get() > short.height().get(),
            "the narrow column did not wrap: {} vs {}",
            tall.height().get(),
            short.height().get()
        );
    }

    #[test]
    fn padding_insets_every_cell() {
        let layout = Layout::new(vec![fixed(100.0), fixed(100.0)], Pt(200.0));
        let cells = [Cell::new("a", Pt(8.0)), Cell::new("b", Pt(8.0))];

        let row = layout.row(
            &mut shaper(),
            &cells,
            Decoration::default(),
            Edges::all(Pt(4.0)),
        );

        assert_eq!(xs(&row), vec![4.0, 104.0]);
        assert_close(row.children[0].y, Pt(4.0));
    }

    #[test]
    fn a_row_carries_the_decoration_it_was_given() {
        let navy = Color::parse_hex("#1F4E79").unwrap();
        let decoration = Decoration {
            background: Some(navy),
            ..Default::default()
        };
        let layout = Layout::new(vec![fixed(100.0)], Pt(100.0));

        let row = layout.row(
            &mut shaper(),
            &[Cell::new("x", Pt(8.0))],
            decoration,
            Edges::default(),
        );

        assert_eq!(row.decoration, decoration);
    }

    #[test]
    fn a_cell_is_inked_in_its_own_colour() {
        let white = Color::parse_hex("#FFFFFF").unwrap();
        let layout = Layout::new(vec![fixed(100.0)], Pt(100.0));

        let row = layout.row(
            &mut shaper(),
            &[Cell::new("Cuenta", Pt(8.0)).inked(white)],
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(first_line(&row, 0).color(), white);
    }

    #[test]
    fn an_empty_row_is_just_its_padding() {
        let layout = Layout::new(vec![fixed(100.0)], Pt(100.0));

        let row = layout.row(
            &mut shaper(),
            &[],
            Decoration::default(),
            Edges::all(Pt(3.0)),
        );

        assert_close(row.height(), Pt(6.0));
    }

    #[test]
    fn cells_beyond_the_declared_columns_are_dropped_not_stacked() {
        // Better a missing cell than one silently piled on top of another.
        let layout = Layout::new(vec![fixed(100.0)], Pt(100.0));
        let cells = [Cell::new("a", Pt(8.0)), Cell::new("b", Pt(8.0))];

        let row = layout.row(
            &mut shaper(),
            &cells,
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(row.children.len(), 1);
    }

    #[test]
    fn an_empty_cell_occupies_its_column_without_drawing() {
        let layout = Layout::new(vec![fixed(100.0), fixed(100.0)], Pt(200.0));
        let cells = [Cell::new("", Pt(8.0)), Cell::new("b", Pt(8.0))];

        let row = layout.row(
            &mut shaper(),
            &cells,
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(
            xs(&row),
            vec![0.0, 100.0],
            "the empty cell still holds its place"
        );
    }

    // ── a whole table ───────────────────────────────────────────────────

    use crate::atom::Atom;
    use crate::pack::{Flow, Group, Repeat, pack};

    /// Builds a table: a header row, `count` body rows, and the group that
    /// makes the header repeat when the table crosses a page.
    fn ledger(count: usize) -> (Vec<Atom>, Vec<Content>, Vec<Group>) {
        let layout = Layout::new(
            vec![
                fixed(80.0),
                Column::new(Length::Auto),
                fixed(90.0).aligned(Align::End),
            ],
            Pt(500.0),
        );
        let mut s = shaper();
        let mut atoms = Vec::new();
        let mut contents = Vec::new();

        let header = layout.row(
            &mut s,
            &[
                Cell::new("Cuenta", Pt(8.0)),
                Cell::new("Nombre", Pt(8.0)),
                Cell::new("Importe", Pt(8.0)),
            ],
            Decoration::default(),
            Edges::all(Pt(2.0)),
        );
        let header_height = header.height();
        atoms.push(Atom::new(header_height).keep_with_next());
        contents.push(Content::Box(header));

        for i in 0..count {
            let row = layout.row(
                &mut s,
                &[
                    Cell::new(format!("4300{i:02}"), Pt(8.0)),
                    Cell::new(format!("Cliente {i}"), Pt(8.0)),
                    Cell::new(format!("{}.00", 100 + i), Pt(8.0)),
                ],
                Decoration::default(),
                Edges::all(Pt(2.0)),
            );
            atoms.push(Atom::new(row.height()));
            contents.push(Content::Box(row));
        }

        // The header repeats on every page the table continues onto. The
        // policy is the caller's: passing `None` here is `repeatHeader={false}`.
        let groups = vec![Group {
            atoms: 0..atoms.len(),
            repeat_prefix: Some(Repeat {
                atom: 0,
                height: header_height,
            }),
        }];
        (atoms, contents, groups)
    }

    #[test]
    fn a_table_that_fits_needs_no_repeated_header() {
        let (atoms, _, groups) = ledger(5);

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(400.0));

        assert_eq!(pages.len(), 1);
        assert!(pages[0].continuations.is_empty());
    }

    #[test]
    fn a_table_crossing_a_page_repeats_its_header() {
        let (atoms, _, groups) = ledger(200);

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(400.0));

        assert!(pages.len() > 1, "the table must span pages");
        assert!(pages[0].continuations.is_empty(), "it starts here");
        for (i, page) in pages.iter().enumerate().skip(1) {
            assert_eq!(
                page.continuations.len(),
                1,
                "page {} lost its header",
                i + 1
            );
        }
    }

    #[test]
    fn the_repeated_header_reserves_room_so_fewer_rows_fit_after_the_first_page() {
        let (atoms, _, groups) = ledger(200);

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(400.0));

        assert!(
            pages[1].placements.len() < pages[0].placements.len(),
            "the repeated header did not consume any budget"
        );
    }

    #[test]
    fn a_heading_row_never_ends_a_page_alone() {
        // Contrived so it would: filler fills the page to within one header
        // of the bottom, leaving room for the header and nothing after it.
        let (table, _, _) = ledger(5);
        let header_height = table[0].height.get();
        let row_height = table[1].height.get();
        let budget = Pt(200.0);

        let filler_count = ((budget.get() - header_height) / row_height).floor() as usize;
        let mut atoms: Vec<Atom> = (0..filler_count)
            .map(|_| Atom::new(Pt(row_height)))
            .collect();
        atoms.extend(table);

        let pages = pack(&Flow::new(&atoms), budget);

        let header_index = filler_count;
        let header_page = pages
            .iter()
            .position(|p| p.atoms().contains(&header_index))
            .expect("the header was never placed");
        let first_row_page = pages
            .iter()
            .position(|p| p.atoms().contains(&(header_index + 1)))
            .expect("the first row was never placed");

        assert_eq!(
            header_page,
            first_row_page,
            "the header was stranded on page {} with its rows on page {}",
            header_page + 1,
            first_row_page + 1
        );
    }

    // ── overflow policies ───────────────────────────────────────────────

    /// A cell rendered under `policy` in a `width`-point column.
    fn under(policy: Overflow, text: &str, width: f32) -> (Vec<String>, Pt) {
        let layout = Layout::new(vec![fixed(width).overflowing(policy)], Pt(width));
        let row = layout.row(
            &mut shaper(),
            &[Cell::new(text, Pt(8.0))],
            Decoration::default(),
            Edges::default(),
        );
        let lines = match &row.children[0].content {
            Content::Box(b) => b
                .children
                .iter()
                .map(|c| match &c.content {
                    Content::Text(l) => l.text.to_string(),
                    _ => String::new(),
                })
                .collect(),
            _ => Vec::new(),
        };
        let widest = match &row.children[0].content {
            Content::Box(b) => b
                .children
                .iter()
                .filter_map(|c| match &c.content {
                    Content::Text(l) => Some(l.width.get()),
                    _ => None,
                })
                .fold(0.0f32, f32::max),
            _ => 0.0,
        };
        (lines, Pt(widest))
    }

    const LONG: &str = "Prestación de servicios profesionales prestados durante el periodo";

    #[test]
    fn wrap_is_the_default_and_uses_as_many_lines_as_it_needs() {
        let (lines, _) = under(Overflow::Wrap, LONG, 90.0);

        assert!(lines.len() > 1, "wrap produced {} line(s)", lines.len());
    }

    #[test]
    fn ellipsis_cuts_to_a_single_line_that_fits() {
        let (lines, widest) = under(Overflow::Ellipsis, LONG, 90.0);

        assert_eq!(lines.len(), 1);
        assert!(
            widest.get() <= 90.0,
            "the cut line is {}pt wide",
            widest.get()
        );
    }

    #[test]
    fn ellipsis_marks_the_cut() {
        let (lines, _) = under(Overflow::Ellipsis, LONG, 90.0);

        assert!(lines[0].ends_with('…'), "no marker on {:?}", lines[0]);
    }

    #[test]
    fn clip_cuts_without_a_marker() {
        let (lines, widest) = under(Overflow::Clip, LONG, 90.0);

        assert_eq!(lines.len(), 1);
        assert!(!lines[0].ends_with('…'));
        assert!(widest.get() <= 90.0);
    }

    #[test]
    fn text_that_already_fits_is_left_alone_by_every_policy() {
        for policy in [Overflow::Wrap, Overflow::Ellipsis, Overflow::Clip] {
            let (lines, _) = under(policy, "Total", 200.0);

            assert_eq!(lines, vec!["Total".to_string()], "{policy:?} altered it");
        }
    }

    #[test]
    fn the_cut_is_measured_rather_than_counted() {
        // The defect this replaces: trimming to a fixed character count cuts
        // wide and narrow letters at the same place, though one string is
        // twice the width of the other.
        let (wide, _) = under(Overflow::Ellipsis, &"W".repeat(60), 90.0);
        let (narrow, _) = under(Overflow::Ellipsis, &"i".repeat(60), 90.0);

        assert!(
            narrow[0].chars().count() > wide[0].chars().count() * 2,
            "kept {} narrow letters against {} wide ones — this is counting, not measuring",
            narrow[0].chars().count(),
            wide[0].chars().count()
        );
    }

    #[test]
    fn a_column_too_narrow_for_even_the_marker_still_produces_something() {
        let (lines, _) = under(Overflow::Ellipsis, LONG, 3.0);

        assert_eq!(lines.len(), 1, "a hopeless column must not hang or vanish");
    }

    // ── diagnostics ─────────────────────────────────────────────────────

    fn report(policy: Overflow, text: &str, width: f32) -> Diagnostics {
        let layout = Layout::new(vec![fixed(width).overflowing(policy)], Pt(width));
        let mut diagnostics = Diagnostics::default();
        layout.row_reporting(
            &mut shaper(),
            &[Cell::new(text, Pt(8.0))],
            Decoration::default(),
            Edges::default(),
            &mut diagnostics,
        );
        diagnostics
    }

    #[test]
    fn clipping_text_is_reported() {
        let d = report(Overflow::Ellipsis, LONG, 90.0);

        assert_eq!(d.len(), 1);
        let message = d.iter().next().unwrap().to_string();
        assert!(message.contains("text-clipped"), "{message}");
        assert!(message.contains("overflow: wrap"), "no hint: {message}");
    }

    #[test]
    fn text_that_fits_reports_nothing() {
        for policy in [Overflow::Wrap, Overflow::Ellipsis, Overflow::Clip] {
            assert!(
                report(policy, "Total", 200.0).is_empty(),
                "{policy:?} complained"
            );
        }
    }

    #[test]
    fn wrapping_is_not_a_defect_and_is_not_reported() {
        // Taking three lines instead of one is the policy working.
        assert!(report(Overflow::Wrap, LONG, 90.0).is_empty());
    }

    #[test]
    fn a_word_with_nowhere_to_break_is_reported_even_under_wrap() {
        let d = report(Overflow::Wrap, "Contabilización", 20.0);

        assert_eq!(d.len(), 1);
        let message = d.iter().next().unwrap().to_string();
        assert!(message.contains("cell-overflow"), "{message}");
        assert!(
            message.contains("pt"),
            "the message should say by how much: {message}"
        );
    }

    #[test]
    fn more_cells_than_columns_is_an_error_not_a_warning() {
        // Silently dropping a value is worse than failing loudly.
        let layout = Layout::new(vec![fixed(100.0)], Pt(100.0));
        let mut d = Diagnostics::default();
        layout.row_reporting(
            &mut shaper(),
            &[Cell::new("a", Pt(8.0)), Cell::new("b", Pt(8.0))],
            Decoration::default(),
            Edges::default(),
            &mut d,
        );

        assert!(
            d.should_fail(false),
            "a dropped cell did not fail the build"
        );
    }

    #[test]
    fn the_same_clipped_column_across_many_rows_is_one_diagnostic() {
        // A ledger of 9,000 rows must not produce 9,000 identical lines.
        let layout = Layout::new(vec![fixed(90.0).overflowing(Overflow::Ellipsis)], Pt(90.0));
        let mut d = Diagnostics::default();
        let mut s = shaper();
        for i in 0..50 {
            layout.row_reporting(
                &mut s,
                &[Cell::new(format!("{LONG} {i}"), Pt(8.0))],
                Decoration::default(),
                Edges::default(),
                &mut d,
            );
        }

        assert_eq!(d.len(), 1, "50 rows produced {} diagnostics", d.len());
    }

    #[test]
    fn a_character_the_font_cannot_draw_is_reported() {
        // The failure that looks like a design choice: an empty box on the
        // page and nothing in the log.
        let layout = Layout::new(vec![fixed(200.0)], Pt(200.0));
        let mut d = Diagnostics::default();
        layout.row_reporting(
            &mut shaper(),
            &[Cell::new("▲ subida", Pt(8.0))],
            Decoration::default(),
            Edges::default(),
            &mut d,
        );

        let message = d.iter().next().expect("nothing reported").to_string();
        assert!(message.contains("missing-glyph"), "{message}");
        assert!(
            message.contains('▲'),
            "the message must name the character: {message}"
        );
    }

    #[test]
    fn a_bold_cell_is_shaped_in_the_bold_face() {
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
        let mut s = Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ]);
        let layout = Layout::new(vec![fixed(200.0)], Pt(200.0));

        let row = layout.row(
            &mut s,
            &[Cell::new("TOTAL", Pt(9.0)).bold()],
            Decoration::default(),
            Edges::default(),
        );

        assert_eq!(first_line(&row, 0).face(), Face::BOLD);
    }

    #[test]
    fn a_bold_cell_measures_wider_than_the_same_text_regular() {
        const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
        let mut s = Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ]);
        let layout = Layout::new(vec![fixed(300.0)], Pt(300.0));
        let plain = layout.row(
            &mut s,
            &[Cell::new("TOTAL A PAGAR", Pt(9.0))],
            Decoration::default(),
            Edges::default(),
        );
        let heavy = layout.row(
            &mut s,
            &[Cell::new("TOTAL A PAGAR", Pt(9.0)).bold()],
            Decoration::default(),
            Edges::default(),
        );

        assert!(
            first_line(&heavy, 0).width.get() > first_line(&plain, 0).width.get(),
            "the bold face was not used for measurement"
        );
    }
}
