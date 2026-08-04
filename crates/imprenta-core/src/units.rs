//! Absolute and relative lengths.

use serde::{Deserialize, Serialize};

/// An absolute length in PDF points (1 pt = 1/72 in).
///
/// Serialises as a bare number, not `{"0": 12}`: a producer writing the IR by
/// hand should be able to type `"size": 12`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pt(pub f32);

pub const PT_PER_IN: f32 = 72.0;
pub const PT_PER_MM: f32 = PT_PER_IN / 25.4;
pub const PT_PER_CM: f32 = PT_PER_MM * 10.0;
/// CSS reference pixel: 96 px to the inch.
pub const PT_PER_PX: f32 = PT_PER_IN / 96.0;

impl Pt {
    pub fn mm(mm: f32) -> Self {
        Self(mm * PT_PER_MM)
    }

    pub fn get(self) -> f32 {
        self.0
    }

    pub fn inch(inch: f32) -> Self {
        Self(inch * PT_PER_IN)
    }

    pub fn px(px: f32) -> Self {
        Self(px * PT_PER_PX)
    }

    pub fn cm(cm: f32) -> Self {
        Self(cm * PT_PER_CM)
    }
}

/// A length as authored: absolute, relative to the containing block, or left
/// for layout to decide.
///
/// Tagged so the three cases are unmistakable in JSON — `{"unit":"percent",
/// "value":0.5}` rather than a bare number whose meaning depends on context.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "value", rename_all = "camelCase")]
pub enum Length {
    /// Absolute, already normalised to points.
    Pt(Pt),
    /// Fraction of the containing block, where `50%` is `0.5`.
    Percent(f32),
    /// Sized by content or by the layout algorithm.
    Auto,
}

impl Default for Length {
    /// Undeclared means "let layout decide", never zero — a zero-width
    /// column would silently vanish instead of being sized.
    fn default() -> Self {
        Self::Auto
    }
}

impl Length {
    /// Resolves against a containing-block extent.
    ///
    /// Returns `None` for [`Length::Auto`]: "auto" has no absolute value, and
    /// what it means depends on the property and the layout algorithm, so the
    /// caller has to decide rather than receive a silent zero.
    pub fn resolve(self, basis: Pt) -> Option<Pt> {
        match self {
            Self::Pt(v) => Some(v),
            Self::Percent(f) => Some(Pt(basis.get() * f)),
            Self::Auto => None,
        }
    }
}

/// The four sides of a box, in CSS order.
///
/// Generic over the value type because the same shape carries lengths
/// (padding, margin, border width), colours (border colour) and styles
/// (border style) — the three always travel together and always per side.
/// `Eq` and `Hash` apply only where `T` has them, which rules out `Edges<Pt>`
/// and is meant to: a length is a float and floats are not keys. Where `T` is
/// a discrete thing — a border, a flag — the edges together make a perfectly
/// good one, and the spreadsheet writer interns them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T: Serialize",
    deserialize = "T: Deserialize<'de> + Default"
))]
pub struct Edges<T> {
    #[serde(default)]
    pub top: T,
    #[serde(default)]
    pub right: T,
    #[serde(default)]
    pub bottom: T,
    #[serde(default)]
    pub left: T,
}

impl<T: Copy> Edges<T> {
    /// CSS one-value shorthand: `padding: 4mm`.
    pub fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// CSS two-value shorthand: `padding: 4mm 8mm`.
    pub fn symmetric(vertical: T, horizontal: T) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

impl Edges<Pt> {
    /// Space below and nowhere else.
    pub fn bottom(value: Pt) -> Self {
        Self {
            bottom: value,
            ..Default::default()
        }
    }

    /// The same edges with more room underneath.
    ///
    /// How `spaceAfter` is honoured where there are no atoms to put a spacer
    /// between — inside a row, or inside anything composed as one piece.
    pub fn plus_bottom(self, extra: Pt) -> Self {
        Self {
            bottom: self.bottom + extra,
            ..self
        }
    }

    /// Total space consumed on the inline axis.
    pub fn horizontal(&self) -> Pt {
        self.left + self.right
    }

    /// Total space consumed on the block axis — this is the one the paginator
    /// subtracts from the page budget.
    pub fn vertical(&self) -> Pt {
        self.top + self.bottom
    }
}

impl std::ops::Add for Pt {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Pt {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts two lengths agree to well below a printer dot at 2400 dpi.
    fn assert_same(a: Pt, b: Pt) {
        assert!(
            (a.get() - b.get()).abs() < 1e-4,
            "expected {a:?} == {b:?} (differ by {})",
            (a.get() - b.get()).abs()
        );
    }

    #[test]
    fn a4_width_is_595_28_points() {
        // A4 is 210 mm wide. Every PDF tool reports that as 595.28 pt.
        assert!((Pt::mm(210.0).get() - 595.2756).abs() < 0.001);
    }

    #[test]
    fn one_inch_is_the_same_length_in_every_unit() {
        // 1 in == 25.4 mm == 2.54 cm == 96 CSS px == 72 pt. A mismatch here
        // means a template authored in mm renders at the wrong scale.
        assert_same(Pt::inch(1.0), Pt::mm(25.4));
        assert_same(Pt::inch(1.0), Pt::cm(2.54));
        assert_same(Pt::inch(1.0), Pt::px(96.0));
        assert_same(Pt::inch(1.0), Pt(72.0));
    }

    #[test]
    fn absolute_length_ignores_the_containing_block() {
        assert_eq!(Length::Pt(Pt(30.0)).resolve(Pt(500.0)), Some(Pt(30.0)));
    }

    #[test]
    fn percentage_length_resolves_against_the_containing_block() {
        assert_eq!(Length::Percent(0.5).resolve(Pt(200.0)), Some(Pt(100.0)));
        assert_eq!(Length::Percent(0.11).resolve(Pt(100.0)), Some(Pt(11.0)));
    }

    #[test]
    fn auto_length_has_no_absolute_value() {
        // Deliberately not `Some(ZERO)`: collapsing auto to zero is how a
        // column silently disappears instead of erroring.
        assert_eq!(Length::Auto.resolve(Pt(200.0)), None);
    }

    #[test]
    fn one_value_shorthand_sets_every_side() {
        let e = Edges::all(Pt(4.0));
        assert_eq!(
            e,
            Edges {
                top: Pt(4.0),
                right: Pt(4.0),
                bottom: Pt(4.0),
                left: Pt(4.0)
            }
        );
    }

    #[test]
    fn two_value_shorthand_is_vertical_then_horizontal() {
        // CSS order, not top-left: `padding: 4mm 8mm` is 4 above and below,
        // 8 left and right. Getting this backwards is a classic silent bug.
        let e = Edges::symmetric(Pt(4.0), Pt(8.0));
        assert_eq!(e.top, Pt(4.0));
        assert_eq!(e.bottom, Pt(4.0));
        assert_eq!(e.left, Pt(8.0));
        assert_eq!(e.right, Pt(8.0));
    }

    #[test]
    fn edges_sum_per_axis() {
        let e = Edges {
            top: Pt(1.0),
            right: Pt(2.0),
            bottom: Pt(4.0),
            left: Pt(8.0),
        };
        assert_eq!(e.horizontal(), Pt(10.0)); // left + right
        assert_eq!(e.vertical(), Pt(5.0)); // top + bottom
    }

    #[test]
    fn edges_are_generic_over_the_value_type() {
        // Border colour and border style ride the same shape as border width.
        let e = Edges::all("solid");
        assert_eq!(e.left, "solid");
    }

    #[test]
    fn a_length_defaults_to_auto() {
        // A column with no declared width is sized by layout, not zero-wide.
        // Defaulting to zero would make an undeclared column silently vanish.
        assert_eq!(Length::default(), Length::Auto);
    }

    #[test]
    fn a_length_in_points_serialises_as_a_bare_number() {
        // So a producer writing the IR by hand types `"size": 12`, not
        // `"size": {"0": 12}`.
        assert_eq!(serde_json::to_string(&Pt(12.0)).unwrap(), "12.0");
        assert_eq!(serde_json::from_str::<Pt>("12").unwrap(), Pt(12.0));
    }

    #[test]
    fn a_length_says_which_unit_it_is_in() {
        // A bare number would leave percent and points indistinguishable.
        let json = serde_json::to_string(&Length::Percent(0.5)).unwrap();

        assert!(json.contains("percent"), "{json}");
        assert_eq!(
            serde_json::from_str::<Length>(&json).unwrap(),
            Length::Percent(0.5)
        );
    }

    #[test]
    fn auto_needs_no_value() {
        assert_eq!(
            serde_json::to_string(&Length::Auto).unwrap(),
            r#"{"unit":"auto"}"#
        );
    }

    #[test]
    fn edges_can_be_given_one_side_at_a_time() {
        // Spelling out four sides to set one is the sort of friction that
        // makes a format unpleasant to write.
        let edges: Edges<Pt> = serde_json::from_str(r#"{"bottom":4}"#).expect("parse");

        assert_eq!(edges.bottom, Pt(4.0));
        assert_eq!(edges.top, Pt(0.0));
    }

    #[test]
    fn lengths_subtract() {
        // The page budget is the page height minus its margins.
        assert_eq!(Pt(297.0) - Pt(24.0), Pt(273.0));
    }
}
