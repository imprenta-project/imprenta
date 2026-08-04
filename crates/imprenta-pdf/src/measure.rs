//! Phase A — turning content into atoms the packer can place.
//!
//! This is the seam between shaping and pagination. Everything downstream of
//! here is arithmetic: the packer sees heights and break flags, never text.

use crate::atom::Atom;
use crate::shape::{Face, Line, Shaper};
use crate::widows::apply_widows_orphans;
use imprenta_core::units::Pt;

/// How a run of text should be set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: Pt,
    /// Minimum lines carried to the top of a page. See [`crate::widows`].
    pub widows: u8,
    /// Minimum lines left at the foot of a page.
    pub orphans: u8,
}

impl TextStyle {
    /// The typographic convention: never one line alone at either end.
    pub fn new(size: Pt) -> Self {
        Self {
            size,
            widows: 2,
            orphans: 2,
        }
    }

    pub fn with_widows_orphans(mut self, widows: u8, orphans: u8) -> Self {
        self.widows = widows;
        self.orphans = orphans;
        self
    }
}

/// A measured paragraph: atoms for the packer, lines for the painter.
///
/// The two vectors are parallel — `atoms[i]` is placed where `lines[i]` will
/// be drawn — and travel together so they cannot drift apart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Measured {
    pub atoms: Vec<Atom>,
    pub lines: Vec<Line>,
}

impl Measured {
    pub fn len(&self) -> usize {
        self.atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.atoms.is_empty()
    }
}

/// Breaks `text` to `width` and turns each resulting line into an atom.
pub fn measure_text(shaper: &mut Shaper, text: &str, style: TextStyle, width: Pt) -> Measured {
    measure_text_in(shaper, text, style, width, Face::REGULAR)
}

/// As [`measure_text`], in a chosen face.
pub fn measure_text_in(
    shaper: &mut Shaper,
    text: &str,
    style: TextStyle,
    width: Pt,
    face: Face,
) -> Measured {
    let lines = shaper.break_lines_in(text, style.size, width, face);

    let mut atoms: Vec<Atom> = lines.iter().map(|l| Atom::new(l.height)).collect();
    apply_widows_orphans(&mut atoms, style.widows, style.orphans);

    Measured { atoms, lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Flow, pack};

    const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
    const PROSE: &str = "Prestación de servicios profesionales periodo 3 para el \
                         cliente comercial, según contrato marco vigente";

    fn shaper() -> Shaper {
        Shaper::new(ROBOTO.to_vec())
    }

    /// No widow or orphan control, so line-to-atom mapping is visible plainly.
    fn loose(size: f32) -> TextStyle {
        TextStyle::new(Pt(size)).with_widows_orphans(1, 1)
    }

    #[test]
    fn every_line_becomes_one_atom() {
        let mut s = shaper();
        let m = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));

        assert!(m.len() > 1, "the sample must break");
        assert_eq!(m.atoms.len(), m.lines.len());
    }

    #[test]
    fn an_atom_takes_the_height_of_its_line() {
        let mut s = shaper();
        let m = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));

        for (atom, line) in m.atoms.iter().zip(&m.lines) {
            assert_eq!(atom.height, line.height);
        }
    }

    #[test]
    fn empty_text_measures_to_nothing() {
        let mut s = shaper();
        let m = measure_text(&mut s, "", loose(7.0), Pt(60.0));

        assert!(m.is_empty());
    }

    #[test]
    fn text_that_fits_measures_to_a_single_atom() {
        let mut s = shaper();
        let m = measure_text(&mut s, "Total", loose(7.0), Pt(400.0));

        assert_eq!(m.len(), 1);
    }

    #[test]
    fn widow_and_orphan_limits_reach_the_atoms() {
        let mut s = shaper();

        let free = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));
        let guarded = measure_text(&mut s, PROSE, TextStyle::new(Pt(7.0)), Pt(60.0));

        assert!(
            free.atoms.iter().all(|a| !a.keep_with_next),
            "nothing pinned without limits"
        );
        assert!(
            guarded.atoms.iter().any(|a| a.keep_with_next),
            "the default 2/2 must pin something"
        );
    }

    // ── text all the way to pages ───────────────────────────────────────

    /// A budget that fits exactly `lines` lines of the measured paragraph.
    fn budget_for(m: &Measured, lines: usize) -> Pt {
        Pt(m.lines[0].height.get() * lines as f32)
    }

    #[test]
    fn a_paragraph_that_fits_stays_on_one_page() {
        let mut s = shaper();
        let m = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));
        let budget = budget_for(&m, m.len());

        let pages = pack(&Flow::new(&m.atoms), budget);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].atoms().len(), m.len());
    }

    #[test]
    fn without_widow_control_a_single_line_is_left_stranded() {
        // The defect, demonstrated: one line alone atop the second page.
        let mut s = shaper();
        let m = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));
        let budget = budget_for(&m, m.len() - 1);

        let pages = pack(&Flow::new(&m.atoms), budget);

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].atoms().len(), 1, "a widow");
    }

    #[test]
    fn widow_control_carries_a_second_line_over_instead() {
        // Same paragraph, same budget, default 2/2 limits: the engine gives
        // up a line on the first page so the second is not left with one.
        let mut s = shaper();
        let loose_m = measure_text(&mut s, PROSE, loose(7.0), Pt(60.0));
        let budget = budget_for(&loose_m, loose_m.len() - 1);

        let m = measure_text(&mut s, PROSE, TextStyle::new(Pt(7.0)), Pt(60.0));
        let pages = pack(&Flow::new(&m.atoms), budget);

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].atoms().len(), 2, "two lines carried over");
        assert_eq!(pages[0].atoms().len(), m.len() - 2);
    }

    #[test]
    fn no_line_is_lost_between_measuring_and_paging() {
        let mut s = shaper();
        let m = measure_text(&mut s, PROSE, TextStyle::new(Pt(7.0)), Pt(45.0));
        let budget = budget_for(&m, 3);

        let pages = pack(&Flow::new(&m.atoms), budget);

        let placed: Vec<usize> = pages.iter().flat_map(|p| p.atoms()).collect();
        assert_eq!(
            placed,
            (0..m.len()).collect::<Vec<_>>(),
            "every line, once, in order"
        );
    }

    #[test]
    fn a_narrower_column_needs_more_pages_for_the_same_budget() {
        let mut s = shaper();
        let budget = Pt(40.0);

        let wide = measure_text(&mut s, PROSE, TextStyle::new(Pt(7.0)), Pt(200.0));
        let narrow = measure_text(&mut s, PROSE, TextStyle::new(Pt(7.0)), Pt(45.0));

        let wide_pages = pack(&Flow::new(&wide.atoms), budget).len();
        let narrow_pages = pack(&Flow::new(&narrow.atoms), budget).len();

        assert!(narrow_pages > wide_pages, "{narrow_pages} vs {wide_pages}");
    }
}
