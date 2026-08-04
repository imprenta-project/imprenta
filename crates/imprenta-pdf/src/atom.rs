//! The unit of pagination.
//!
//! An `Atom` is one indivisible slice of measured content: a single line of a
//! paragraph, one table row, a repeated table header. The measure phase emits
//! them; the pack phase places them on pages.
//!
//! **The packer sees nothing else.** It does not know whether an atom came
//! from a paragraph, a table or a future primitive nobody has written yet.
//! That is what keeps the hardest code in the engine untouched every time a
//! primitive is added.

use imprenta_core::units::Pt;

/// Where a forced page break puts the atom that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Break {
    /// Break only when the page runs out. The normal case.
    #[default]
    Auto,
    /// Start a new page.
    Always,
    /// Start on an odd (recto, right-hand) page, leaving a blank if needed.
    /// Chapters in a bound document open on the recto.
    Odd,
    /// Start on an even (verso, left-hand) page.
    Even,
}

/// One measured, indivisible slice of content.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub height: Pt,
    /// This atom must not be the last on its page. A section heading or a
    /// table's column header stranded at the foot of a page, with its content
    /// overleaf, is the defect this prevents.
    pub keep_with_next: bool,
    pub break_before: Break,
}

impl Atom {
    pub fn new(height: Pt) -> Self {
        Self {
            height,
            keep_with_next: false,
            break_before: Break::Auto,
        }
    }

    pub fn break_before(mut self, kind: Break) -> Self {
        self.break_before = kind;
        self
    }

    pub fn keep_with_next(mut self) -> Self {
        self.keep_with_next = true;
        self
    }
}
