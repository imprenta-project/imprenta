//! What a box paints behind and around its content.
//!
//! Kept separate from the atom that carries it: the packer decides *where* a
//! box goes and must never learn what it looks like.

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};

/// One edge of a border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSide {
    pub width: Pt,
    pub color: Color,
}

/// The fill and border of a box.
///
/// Every side is independent because that is how real documents are ruled —
/// a table row with a line underneath and nothing else is the common case,
/// and the templates this engine has to replace use per-side borders 87
/// times over.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Decoration {
    pub background: Option<Color>,
    pub border: Edges<Option<BorderSide>>,
    /// Corner radius. Zero is a square box, which is nearly all of them.
    pub radius: Pt,
}

impl Decoration {
    /// Whether there is anything at all to paint.
    ///
    /// A radius does not count: it is the shape of the ink, not ink.
    pub fn is_empty(&self) -> bool {
        self.background.is_none()
            && self.border.top.is_none()
            && self.border.right.is_none()
            && self.border.bottom.is_none()
            && self.border.left.is_none()
    }

    /// The border, when all four sides are there and identical.
    ///
    /// Only such a border can follow a rounded outline. Where two sides
    /// differ the corner between them belongs to neither, and an arc through
    /// it would be the engine inventing a shape nobody asked for.
    pub fn uniform_border(&self) -> Option<BorderSide> {
        let top = self.border.top?;
        let same = [self.border.right, self.border.bottom, self.border.left]
            .into_iter()
            .all(|side| side == Some(top));
        same.then_some(top)
    }
}

/// A radius the box can actually hold.
///
/// Two corners share every side, so anything above half the shorter side
/// would have them overlap and the outline cross itself. CSS clamps the same
/// way, and an author who writes `rounded-full` on a short box means "as
/// round as it goes", not "draw it inside out".
pub fn fitted_radius(radius: Pt, width: Pt, height: Pt) -> Pt {
    Pt(radius
        .get()
        .max(0.0)
        .min(width.get() / 2.0)
        .min(height.get() / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use imprenta_core::color::Color;

    fn side() -> BorderSide {
        BorderSide {
            width: Pt(1.0),
            color: Color::BLACK,
        }
    }

    #[test]
    fn a_decoration_with_nothing_set_paints_nothing() {
        assert!(Decoration::default().is_empty());
    }

    #[test]
    fn a_background_alone_is_something_to_paint() {
        assert!(
            !Decoration {
                background: Some(Color::BLACK),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn any_single_side_counts_as_something_to_paint() {
        // Five branches, and forgetting one means a rule that silently never
        // appears. Each side gets its own assertion for that reason.
        let cases: [(&str, Edges<Option<BorderSide>>); 4] = [
            (
                "top",
                Edges {
                    top: Some(side()),
                    ..Default::default()
                },
            ),
            (
                "right",
                Edges {
                    right: Some(side()),
                    ..Default::default()
                },
            ),
            (
                "bottom",
                Edges {
                    bottom: Some(side()),
                    ..Default::default()
                },
            ),
            (
                "left",
                Edges {
                    left: Some(side()),
                    ..Default::default()
                },
            ),
        ];

        for (name, border) in cases {
            assert!(
                !Decoration {
                    border,
                    ..Default::default()
                }
                .is_empty(),
                "a {name} border was treated as nothing to paint"
            );
        }
    }

    #[test]
    fn a_zero_width_border_is_still_declared() {
        // Whether a zero-width rule should draw is the painter's decision,
        // not this predicate's: `is_empty` reports intent, not visibility.
        assert!(
            !Decoration {
                background: None,
                border: Edges {
                    bottom: Some(BorderSide {
                        width: Pt(0.0),
                        color: Color::BLACK
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }
            .is_empty()
        );
    }

    // ── rounded corners ─────────────────────────────────────────────────

    #[test]
    fn a_radius_no_bigger_than_the_box_is_left_alone() {
        assert_eq!(fitted_radius(Pt(6.0), Pt(100.0), Pt(40.0)), Pt(6.0));
    }

    #[test]
    fn a_radius_larger_than_the_box_is_brought_down_to_fit() {
        // Two corners share each side, so a radius above half the shorter
        // side would have them overlap and the outline cross itself.
        assert_eq!(fitted_radius(Pt(50.0), Pt(100.0), Pt(20.0)), Pt(10.0));
        assert_eq!(fitted_radius(Pt(50.0), Pt(12.0), Pt(80.0)), Pt(6.0));
    }

    #[test]
    fn a_radius_of_nothing_stays_nothing() {
        assert_eq!(fitted_radius(Pt(0.0), Pt(100.0), Pt(40.0)), Pt(0.0));
    }

    #[test]
    fn a_negative_radius_is_treated_as_none() {
        assert_eq!(fitted_radius(Pt(-4.0), Pt(100.0), Pt(40.0)), Pt(0.0));
    }

    #[test]
    fn a_box_with_only_a_radius_still_paints_nothing() {
        // A radius is a shape, not a thing to draw. Without a fill or a rule
        // there is no ink in it.
        let rounded = Decoration {
            radius: Pt(6.0),
            ..Default::default()
        };

        assert!(rounded.is_empty());
    }

    #[test]
    fn a_border_all_the_way_round_in_one_colour_can_be_rounded() {
        let uniform = Decoration {
            border: Edges::all(Some(side())),
            radius: Pt(4.0),
            ..Default::default()
        };

        assert_eq!(uniform.uniform_border(), Some(side()));
    }

    #[test]
    fn a_border_that_is_not_the_same_all_round_cannot_be() {
        // Where two sides differ, the corner between them belongs to neither
        // and an arc through it would be this engine inventing something.
        let thicker = BorderSide {
            width: Pt(3.0),
            color: Color::BLACK,
        };
        let mixed = Decoration {
            border: Edges {
                top: Some(side()),
                right: Some(thicker),
                bottom: Some(side()),
                left: Some(side()),
            },
            ..Default::default()
        };
        let partial = Decoration {
            border: Edges {
                bottom: Some(side()),
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(mixed.uniform_border(), None);
        assert_eq!(partial.uniform_border(), None);
        assert_eq!(Decoration::default().uniform_border(), None);
    }
}
