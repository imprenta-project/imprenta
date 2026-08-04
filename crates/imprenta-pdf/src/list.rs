//! Ordered and unordered lists.
//!
//! Like [`crate::table`], mechanism rather than component: a list is a marker
//! column and an indented content column, which is a two-track table by
//! another name. It exists because getting the marker to align with the first
//! line of a wrapped item — and to stay put when the item spans a page — is
//! fiddly enough to be worth doing once.
//!
//! Nothing about how it looks is decided here. The marker is a string the
//! caller supplies or a numbering scheme it picks; colour, weight and spacing
//! ride on the `Cell`s as they do everywhere else.

use crate::content::{BoxContent, Content};
use crate::decoration::Decoration;
use crate::shape::{Face, Shaper};
use crate::table::{Align, Cell, Column, Layout};
use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Length, Pt};

/// How an item is labelled.
#[derive(Debug, Clone, PartialEq)]
pub enum Marker {
    /// The same string before every item.
    Bullet(String),
    /// 1., 2., 3. …
    Decimal,
    /// a., b., c. … and then aa., ab. — never running out.
    LowerAlpha,
    /// i., ii., iii. …
    LowerRoman,
    /// No marker at all, only the indent.
    None,
}

impl Marker {
    /// The label for the item at `index`, counting from zero.
    pub fn label(&self, index: usize) -> String {
        match self {
            Self::Bullet(s) => s.clone(),
            Self::Decimal => format!("{}.", index + 1),
            Self::LowerAlpha => format!("{}.", alpha(index)),
            Self::LowerRoman => format!("{}.", roman(index + 1)),
            Self::None => String::new(),
        }
    }
}

/// Spreadsheet-style letters: a…z, then aa, ab, … so a list can be any length.
fn alpha(index: usize) -> String {
    let mut n = index;
    let mut out = Vec::new();
    loop {
        out.push(b'a' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

fn roman(mut n: usize) -> String {
    const TABLE: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, sign) in TABLE {
        while n >= value {
            out.push_str(sign);
            n -= value;
        }
    }
    out
}

/// A list's geometry: a marker gutter and the content beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub marker: Marker,
    layout: Layout,
}

impl List {
    /// `gutter` is the width reserved for markers, `gap` the space between a
    /// marker and the text beside it.
    ///
    /// The gap is a column of its own rather than padding on the marker,
    /// because the marker is right-aligned: padding would move the marker,
    /// and what has to stay put is its right edge, so "9." and "10." line up
    /// on the full stop. Without a gap at all the two run together — `1.Pago`.
    pub fn new(marker: Marker, gutter: Pt, gap: Pt, width: Pt) -> Self {
        Self {
            marker,
            layout: Layout::new(
                vec![
                    Column::new(Length::Pt(gutter)).aligned(Align::End),
                    Column::new(Length::Pt(gap)),
                    Column::new(Length::Auto),
                ],
                width,
            ),
        }
    }

    /// One item, marker and all.
    ///
    /// The marker is right-aligned in its gutter and sits on the first line
    /// of the item — not centred against a wrapped block, which is what makes
    /// a hand-rolled list look wrong the moment an item runs to two lines.
    pub fn item(
        &self,
        shaper: &mut Shaper,
        index: usize,
        text: &str,
        size: Pt,
        color: Color,
    ) -> BoxContent {
        self.styled_item(shaper, index, text, size, color, Face::REGULAR)
    }

    /// As [`Self::item`], in a chosen face.
    pub fn styled_item(
        &self,
        shaper: &mut Shaper,
        index: usize,
        text: &str,
        size: Pt,
        color: Color,
        face: Face,
    ) -> BoxContent {
        self.layout.row(
            shaper,
            &[
                Cell::new(self.marker.label(index), size).inked(color),
                Cell::new("", size),
                Cell::new(text, size).inked(color).in_face(face),
            ],
            Decoration::default(),
            Edges::symmetric(Pt(0.0), Pt(0.0)),
        )
    }

    /// Where the content column begins, for nesting one list inside another.
    pub fn content_x(&self) -> Pt {
        self.layout.tracks[2].x
    }
}

/// Convenience: an entire list as stacked content.
pub fn simple(
    shaper: &mut Shaper,
    marker: Marker,
    items: &[&str],
    size: Pt,
    color: Color,
    width: Pt,
) -> Vec<Content> {
    let list = List::new(marker, Pt(size.get() * 2.0), Pt(size.get() * 0.4), width);
    items
        .iter()
        .enumerate()
        .map(|(i, text)| Content::Box(list.item(shaper, i, text, size, color)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

    fn shaper() -> Shaper {
        Shaper::new(ROBOTO.to_vec())
    }

    /// The text of every line inside cell `i` of an item.
    fn cell_lines(item: &BoxContent, i: usize) -> Vec<String> {
        match &item.children[i].content {
            Content::Box(b) => b
                .children
                .iter()
                .filter_map(|c| match &c.content {
                    Content::Text(l) => Some(l.text.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn decimal_markers_count_from_one() {
        assert_eq!(Marker::Decimal.label(0), "1.");
        assert_eq!(Marker::Decimal.label(41), "42.");
    }

    #[test]
    fn alphabetic_markers_carry_past_z() {
        // A 30-item list must not run out of letters or repeat one.
        assert_eq!(Marker::LowerAlpha.label(0), "a.");
        assert_eq!(Marker::LowerAlpha.label(25), "z.");
        assert_eq!(Marker::LowerAlpha.label(26), "aa.");
        assert_eq!(Marker::LowerAlpha.label(27), "ab.");
    }

    #[test]
    fn roman_markers_follow_the_subtractive_rules() {
        assert_eq!(Marker::LowerRoman.label(0), "i.");
        assert_eq!(Marker::LowerRoman.label(3), "iv.");
        assert_eq!(Marker::LowerRoman.label(8), "ix.");
        assert_eq!(Marker::LowerRoman.label(39), "xl.");
        assert_eq!(Marker::LowerRoman.label(1943), "mcmxliv.");
    }

    #[test]
    fn a_bullet_is_the_same_for_every_item() {
        let m = Marker::Bullet("·".into());

        assert_eq!(m.label(0), "·");
        assert_eq!(m.label(99), "·");
    }

    #[test]
    fn no_marker_means_an_indent_and_nothing_else() {
        assert_eq!(Marker::None.label(3), "");
    }

    #[test]
    fn an_item_has_a_marker_beside_its_text() {
        let list = List::new(Marker::Decimal, Pt(20.0), Pt(4.0), Pt(300.0));

        let item = list.item(&mut shaper(), 0, "Licencia anual", Pt(9.0), Color::BLACK);

        assert_eq!(cell_lines(&item, 0), vec!["1.".to_string()]);
        assert_eq!(cell_lines(&item, 2), vec!["Licencia anual".to_string()]);
    }

    #[test]
    fn the_marker_is_right_aligned_against_the_content() {
        // So "9." and "10." line up on the full stop rather than the digit.
        let list = List::new(Marker::Decimal, Pt(30.0), Pt(4.0), Pt(300.0));
        let mut s = shaper();

        let ninth = list.item(&mut s, 8, "x", Pt(9.0), Color::BLACK);
        let tenth = list.item(&mut s, 9, "x", Pt(9.0), Color::BLACK);

        let end = |item: &BoxContent| match &item.children[0].content {
            Content::Box(b) => match &b.children[0].content {
                Content::Text(l) => b.children[0].x.get() + l.width.get(),
                _ => 0.0,
            },
            _ => 0.0,
        };
        assert!(
            (end(&ninth) - end(&tenth)).abs() < 0.01,
            "markers do not share a right edge"
        );
    }

    #[test]
    fn a_wrapped_item_keeps_its_marker_on_the_first_line() {
        // The defect a hand-rolled list has: the marker drifting to the
        // middle of a two-line item.
        let list = List::new(Marker::Decimal, Pt(20.0), Pt(4.0), Pt(120.0));

        let item = list.item(
            &mut shaper(),
            0,
            "Prestación de servicios profesionales durante el periodo indicado",
            Pt(9.0),
            Color::BLACK,
        );

        assert!(cell_lines(&item, 2).len() > 1, "the sample must wrap");
        let marker_y = item.children[0].y;
        let text_y = item.children[2].y;
        assert!(
            (marker_y.get() - text_y.get()).abs() < 0.01,
            "the marker sits at {marker_y:?} and the text at {text_y:?}"
        );
    }

    #[test]
    fn an_item_is_as_tall_as_its_wrapped_text() {
        let list = List::new(Marker::Decimal, Pt(20.0), Pt(4.0), Pt(120.0));
        let mut s = shaper();

        let short = list.item(&mut s, 0, "Uno", Pt(9.0), Color::BLACK);
        let long = list.item(
            &mut s,
            1,
            "Prestación de servicios profesionales durante el periodo indicado",
            Pt(9.0),
            Color::BLACK,
        );

        assert!(long.height().get() > short.height().get());
    }

    #[test]
    fn a_whole_list_comes_back_one_item_per_entry() {
        let items = ["Uno", "Dos", "Tres"];

        let contents = simple(
            &mut shaper(),
            Marker::Bullet("•".into()),
            &items,
            Pt(9.0),
            Color::BLACK,
            Pt(300.0),
        );

        assert_eq!(contents.len(), 3);
    }

    #[test]
    fn nesting_starts_where_the_parent_content_starts() {
        let outer = List::new(Marker::Decimal, Pt(24.0), Pt(4.0), Pt(300.0));

        assert_eq!(outer.content_x(), Pt(28.0), "gutter plus gap");
    }

    #[test]
    fn a_marker_never_touches_the_text_beside_it() {
        // `1.Pago` instead of `1. Pago` is what happens without a gap column.
        let gap = 5.0;
        let list = List::new(Marker::Decimal, Pt(20.0), Pt(gap), Pt(300.0));

        let item = list.item(&mut shaper(), 0, "Pago", Pt(9.0), Color::BLACK);

        let marker_end = match &item.children[0].content {
            Content::Box(b) => match &b.children[0].content {
                Content::Text(l) => {
                    item.children[0].x.get() + b.children[0].x.get() + l.width.get()
                }
                _ => 0.0,
            },
            _ => 0.0,
        };
        let text_start = item.children[2].x.get();

        assert!(
            text_start - marker_end >= gap - 0.01,
            "only {:.1}pt between the marker and the text",
            text_start - marker_end
        );
    }
}
