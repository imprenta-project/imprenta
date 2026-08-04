//! Phase B — packing measured atoms onto pages.
//!
//! Pure arithmetic over heights: no shaping, no layout, no fonts. That is why
//! it is fast enough to run over the whole document in order (packing 9,231
//! pages measured ~10 ms in the prototype), and why the engine can make good
//! break decisions instead of guessing from a rows-per-page constant.
//!
//! Because it runs in document order it is also the only place that can carry
//! state across a page boundary — running totals, section context, "continued
//! from page 12".

use crate::atom::{Atom, Break};
use imprenta_core::units::Pt;

/// Slack allowed when deciding whether a run still fits on the page.
///
/// The vertical cursor is a running sum of f32 heights, so it drifts from the
/// exact total by an ULP per addition — a page of a hundred atoms accumulates
/// on the order of 1e-4 pt. Comparing strictly turns that drift into a page
/// break, so content that exactly fills its page spills a nearly empty one
/// after it.
///
/// 1e-3 pt is roughly 0.35 µm: a thirtieth of a single dot at 2400 dpi, which
/// is the finest commercial printing there is. Nothing this tolerance lets
/// through can be seen, printed, or measured.
const FIT_EPSILON: f32 = 1e-3;

/// An atom placed on a page, at an offset from the top of the content box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Index into the slice handed to [`pack`].
    pub atom: usize,
    pub y: Pt,
    /// The atom's height, restated so a placement describes its own box.
    /// The painter draws backgrounds and rules from this and should not have
    /// to cross-reference the atom slice to learn how tall a row is.
    pub height: Pt,
}

/// A run of atoms that belongs together and may repeat a prefix when split.
///
/// The packer has no idea what a table is. It knows that some atoms form a
/// group, and that a group may ask for space to be reserved at the top of
/// every page it continues onto. A table's repeated column header is one user
/// of that; a "(continued from page 12)" line and a carried-forward balance
/// are others, and so is whatever primitive gets written next year.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// Indices into the atom slice, as a half-open range.
    pub atoms: std::ops::Range<usize>,
    /// What to repeat at the top of each continuation page.
    ///
    /// `None` means the group simply continues with nothing repeated — this
    /// is how a primitive (or an author writing `repeatHeader={false}`) opts
    /// out. The engine provides the capability; the policy lives above it.
    pub repeat_prefix: Option<Repeat>,
}

/// The atom a group repeats when it carries onto a new page.
///
/// Names an atom rather than carrying content so the packer stays blind to
/// what anything looks like: it needs the height to reserve room, and the
/// painter looks up the atom when it comes to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Repeat {
    pub atom: usize,
    pub height: Pt,
}

/// A group carrying onto a page it did not start on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Continuation {
    /// Index into [`Flow::groups`].
    pub group: usize,
    /// The atom to draw — the same one the group named.
    pub atom: usize,
    /// Where it is painted, from the top of the content box.
    pub y: Pt,
    pub height: Pt,
}

/// A value an atom adds to a running total when it is placed.
///
/// Sparse on purpose: in a ledger of 700,000 atoms only the rows carrying an
/// amount contribute, so a per-atom field would be mostly zeroes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contribution {
    /// Index into [`Flow::atoms`]. Must be ascending across the slice.
    pub atom: usize,
    /// Which running total this feeds.
    pub accumulator: usize,
    pub value: f64,
}

/// Everything the packer needs to lay out a document.
#[derive(Debug, Clone, Default)]
pub struct Flow<'a> {
    pub atoms: &'a [Atom],
    pub groups: &'a [Group],
    /// How many running totals the document declares.
    pub accumulators: usize,
    /// Ascending by atom index.
    pub contributions: &'a [Contribution],
    /// What the running totals already stood at before this flow began.
    ///
    /// A document composed in segments packs each one separately; without
    /// this the totals would restart at zero every time and "carried
    /// forward" would be wrong from the second page on.
    pub opening: &'a [f64],
    /// Which groups had already placed atoms before this flow began.
    ///
    /// Parallel to `groups`; a shorter slice leaves the rest false. Without
    /// it a flow that resumes partway through a table treats its first page
    /// as the table's first page, and the repeated header goes missing on
    /// exactly the pages that most need it.
    pub started: &'a [bool],
}

impl<'a> Flow<'a> {
    pub fn new(atoms: &'a [Atom]) -> Self {
        Self {
            atoms,
            groups: &[],
            accumulators: 0,
            contributions: &[],
            opening: &[],
            started: &[],
        }
    }

    /// Starts the running totals from where a previous segment left off.
    pub fn continuing_from(mut self, opening: &'a [f64]) -> Self {
        self.opening = opening;
        self
    }

    /// Marks groups that were already under way when this flow began.
    pub fn resuming(mut self, started: &'a [bool]) -> Self {
        self.started = started;
        self
    }

    pub fn with_groups(mut self, groups: &'a [Group]) -> Self {
        self.groups = groups;
        self
    }

    pub fn with_accumulators(mut self, count: usize, contributions: &'a [Contribution]) -> Self {
        self.accumulators = count;
        self.contributions = contributions;
        self
    }
}

/// One page's worth of placements, in document order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub placements: Vec<Placement>,
    /// Groups continuing onto this page from the previous one, in the order
    /// their prefixes are painted.
    pub continuations: Vec<Continuation>,
    /// Running totals as they stood *before* anything on this page was
    /// placed — the "brought forward" figure a header shows.
    pub opening: Vec<f64>,
    /// Running totals *after* this page's content — the "carried forward"
    /// figure a footer shows.
    pub closing: Vec<f64>,
}

impl Page {
    pub fn atoms(&self) -> Vec<usize> {
        self.placements.iter().map(|p| p.atom).collect()
    }
}

/// Places every atom onto a page, in order.
///
/// `budget` is the usable height of the page content box — margins and margin
/// boxes have already been subtracted by the caller.
pub fn pack(flow: &Flow, budget: Pt) -> Vec<Page> {
    let atoms = flow.atoms;
    let mut pages: Vec<Page> = Vec::new();
    let mut current = Page::default();
    let mut y = Pt(0.0);
    let mut index = 0;
    // Whether each group has placed at least one atom. A group whose *first*
    // atom happens to land on a fresh page has not been continued onto it —
    // it begins there — so it gets no repeated prefix.
    let mut started = vec![false; flow.groups.len()];
    for (g, was) in flow.started.iter().enumerate().take(started.len()) {
        started[g] = *was;
    }
    // Running totals, and their value when the page in hand opened.
    let mut running = if flow.opening.len() == flow.accumulators {
        flow.opening.to_vec()
    } else {
        vec![0.0; flow.accumulators]
    };
    let mut opening = running.clone();
    // Contributions arrive ascending by atom, and atoms are placed in
    // ascending order, so one forward-only cursor suffices.
    let mut contribution = 0;

    while index < atoms.len() {
        let run = run_starting_at(atoms, index);
        let run_height = atoms[run.clone()]
            .iter()
            .fold(Pt(0.0), |total, a| total + a.height);

        // A forced break is a property of the run, taken from the atom that
        // opens it — breaking inside a keep-with-next run would defeat it.
        //
        // A run that exactly fills the budget fits, hence the epsilon rather
        // than a bare `>`.
        //
        // The emptiness guard is what makes a run taller than a whole page
        // terminate: it is placed on the page it lands on and overflows,
        // rather than triggering an unbounded search for a page big enough.
        // It also stops a forced break at the very start of the document
        // from emitting a leading blank page.
        let forced = atoms[run.start].break_before;
        let overflows = (y + run_height).get() > budget.get() + FIT_EPSILON;

        if (forced != Break::Auto || overflows) && !current.placements.is_empty() {
            seal(&mut pages, std::mem::take(&mut current), &opening, &running);
            opening.clone_from(&running);
            y = Pt(0.0);
        }

        // Parity breaks may need a blank page to land on the right side of
        // the spread. `pages.len() + 1` is the 1-indexed number the next page
        // will have. A blank page contributes nothing, so the running totals
        // pass straight through it.
        while parity_unsatisfied(forced, pages.len() + 1) {
            seal(&mut pages, Page::default(), &opening, &running);
        }

        // At the top of a fresh page, a group that has already placed atoms
        // elsewhere is being *continued* onto this one, so it may reserve its
        // repeated prefix before any content goes down.
        if current.placements.is_empty()
            && let Some(g) = group_containing(flow.groups, run.start)
            && started[g]
            && let Some(repeat) = flow.groups[g].repeat_prefix
        {
            current.continuations.push(Continuation {
                group: g,
                atom: repeat.atom,
                y,
                height: repeat.height,
            });
            y = y + repeat.height;
        }

        for i in run.clone() {
            current.placements.push(Placement {
                atom: i,
                y,
                height: atoms[i].height,
            });
            y = y + atoms[i].height;
            if let Some(g) = group_containing(flow.groups, i) {
                started[g] = true;
            }
            // Totals accrue where the atom actually landed, not where it sat
            // in the input — a run pushed to the next page takes its
            // contributions with it.
            while let Some(c) = flow.contributions.get(contribution)
                && c.atom <= i
            {
                if let Some(total) = running.get_mut(c.accumulator) {
                    *total += c.value;
                }
                contribution += 1;
            }
        }
        index = run.end;
    }

    if !current.placements.is_empty() {
        seal(&mut pages, current, &opening, &running);
    }
    pages
}

/// Stamps the running totals onto a finished page and files it.
fn seal(pages: &mut Vec<Page>, mut page: Page, opening: &[f64], running: &[f64]) {
    page.opening = opening.to_vec();
    page.closing = running.to_vec();
    pages.push(page);
}

/// The group owning `index`, if any.
///
/// Groups are required to be sorted by start and non-overlapping, which lets
/// this be a binary search: a ledger with 40,000 grouped entries would make a
/// linear scan quadratic over the document.
fn group_containing(groups: &[Group], index: usize) -> Option<usize> {
    let candidate = groups.partition_point(|g| g.atoms.end <= index);
    groups
        .get(candidate)
        .filter(|g| g.atoms.contains(&index))
        .map(|_| candidate)
}

/// Whether `page_number` is the wrong parity for the requested break.
fn parity_unsatisfied(forced: Break, page_number: usize) -> bool {
    match forced {
        Break::Odd => page_number % 2 == 0,
        Break::Even => page_number % 2 == 1,
        Break::Auto | Break::Always => false,
    }
}

/// The maximal run of atoms that must stay together, starting at `start`.
///
/// A run is a chain of `keep_with_next` atoms plus the first atom that ends
/// the chain — a heading keeps with the column header, which keeps with the
/// first row, and all three travel as one. A trailing chain at the end of the
/// document has nothing to keep with, so it simply ends.
fn run_starting_at(atoms: &[Atom], start: usize) -> std::ops::Range<usize> {
    let mut end = start;
    while end < atoms.len() && atoms[end].keep_with_next {
        end += 1;
    }
    start..(end + 1).min(atoms.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Atoms of uniform height, which keeps the arithmetic in the assertions
    /// obvious: with a budget of 100 and a height of 10, ten fit per page.
    fn atoms_of_height(count: usize, height: f32) -> Vec<Atom> {
        (0..count).map(|_| Atom::new(Pt(height))).collect()
    }

    #[test]
    fn no_atoms_produce_no_pages() {
        assert_eq!(pack(&Flow::new(&[]), Pt(100.0)), vec![]);
    }

    #[test]
    fn atoms_that_fit_share_one_page() {
        let pages = pack(&Flow::new(&atoms_of_height(3, 10.0)), Pt(100.0));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].atoms(), vec![0, 1, 2]);
    }

    #[test]
    fn atoms_stack_from_the_top_of_the_content_box() {
        let pages = pack(&Flow::new(&atoms_of_height(3, 10.0)), Pt(100.0));

        assert_eq!(
            pages[0].placements,
            vec![
                Placement {
                    atom: 0,
                    y: Pt(0.0),
                    height: Pt(10.0)
                },
                Placement {
                    atom: 1,
                    y: Pt(10.0),
                    height: Pt(10.0)
                },
                Placement {
                    atom: 2,
                    y: Pt(20.0),
                    height: Pt(10.0)
                },
            ]
        );
    }

    #[test]
    fn an_atom_that_does_not_fit_starts_a_new_page() {
        // Ten atoms of 10 exactly fill a budget of 100; the eleventh spills.
        let pages = pack(&Flow::new(&atoms_of_height(11, 10.0)), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].atoms().len(), 10);
        assert_eq!(pages[1].atoms(), vec![10]);
    }

    #[test]
    fn a_new_page_restarts_the_vertical_cursor() {
        let pages = pack(&Flow::new(&atoms_of_height(11, 10.0)), Pt(100.0));

        assert_eq!(pages[1].placements[0].y, Pt(0.0));
    }

    #[test]
    fn an_atom_exactly_filling_the_budget_stays_on_the_page() {
        // Off-by-one guard: 100 into 100 fits. Getting this wrong is how a
        // document grows a blank page for every section.
        let pages = pack(&Flow::new(&atoms_of_height(1, 100.0)), Pt(100.0));

        assert_eq!(pages.len(), 1);
    }

    #[test]
    fn a_placement_states_the_height_of_its_atom() {
        let atoms = vec![Atom::new(Pt(10.0)), Atom::new(Pt(25.0))];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages[0].placements[0].height, Pt(10.0));
        assert_eq!(pages[0].placements[1].height, Pt(25.0));
    }

    #[test]
    fn accumulated_rounding_does_not_manufacture_an_extra_page() {
        // Twelve lines of 7pt text at 1.2 leading. In f32 the height is
        // 8.399999618530273, and adding it twelve times gives 100.80001,
        // while multiplying gives 100.79999 — the running cursor overshoots
        // a page that content exactly fills by 1.5e-5 pt, five nanometres.
        //
        // A strict comparison turns that into a whole extra page, and it
        // would happen on any document whose content lands flush with the
        // page box, which is most of them.
        let height = 7.0f32 * 1.2;
        let atoms: Vec<Atom> = (0..12).map(|_| Atom::new(Pt(height))).collect();

        let pages = pack(&Flow::new(&atoms), Pt(height * 12.0));

        assert_eq!(pages.len(), 1, "rounding conjured a page");
    }

    // ── keep-with-next ──────────────────────────────────────────────────
    // Replaces the hand-rolled "pre-emptive page break — prevents orphan
    // headers when page is full" that report templates otherwise grow.

    #[test]
    fn a_heading_is_never_left_alone_at_the_foot_of_a_page() {
        // Budget 100, atoms of 10: nine rows, then a heading that would sit at
        // y=90 as the last thing on the page. It must move to page 2 instead.
        let mut atoms = atoms_of_height(9, 10.0);
        atoms.push(Atom::new(Pt(10.0)).keep_with_next());
        atoms.push(Atom::new(Pt(10.0)));

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].atoms(), vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(pages[1].atoms(), vec![9, 10]);
    }

    #[test]
    fn a_run_of_keep_with_next_atoms_moves_as_a_block() {
        // Section heading + column header + first row must land together:
        // this is exactly "asiento header, then table header, then rows".
        let mut atoms = atoms_of_height(8, 10.0);
        atoms.push(Atom::new(Pt(10.0)).keep_with_next()); // 8 — section heading
        atoms.push(Atom::new(Pt(10.0)).keep_with_next()); // 9 — column header
        atoms.push(Atom::new(Pt(10.0))); //                  10 — first row

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages[0].atoms(), vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(pages[1].atoms(), vec![8, 9, 10]);
    }

    #[test]
    fn keep_with_next_does_not_move_an_atom_that_already_has_company() {
        // The heading fits at y=0 with rows after it — no reason to move.
        let mut atoms = vec![Atom::new(Pt(10.0)).keep_with_next()];
        atoms.extend(atoms_of_height(3, 10.0));

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].atoms(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn keep_with_next_on_the_last_atom_of_the_document_is_ignored() {
        // Nothing follows, so there is nothing to keep it with. It must not
        // loop, and must not emit a trailing blank page.
        let mut atoms = atoms_of_height(9, 10.0);
        atoms.push(Atom::new(Pt(10.0)).keep_with_next());

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].atoms().len(), 10);
    }

    // ── forced breaks ───────────────────────────────────────────────────

    #[test]
    fn an_always_break_starts_a_new_page_even_with_room_to_spare() {
        let atoms = vec![
            Atom::new(Pt(10.0)),
            Atom::new(Pt(10.0)).break_before(Break::Always),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].atoms(), vec![0]);
        assert_eq!(pages[1].atoms(), vec![1]);
    }

    #[test]
    fn a_break_on_the_very_first_atom_does_not_emit_a_leading_blank_page() {
        let atoms = vec![Atom::new(Pt(10.0)).break_before(Break::Always)];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].atoms(), vec![0]);
    }

    #[test]
    fn an_odd_break_inserts_a_blank_page_when_the_next_page_would_be_even() {
        // Page 1 holds the first atom, so the next page is 2 — even. A blank
        // page 2 is emitted so the chapter opens on the recto, page 3.
        let atoms = vec![
            Atom::new(Pt(10.0)),
            Atom::new(Pt(10.0)).break_before(Break::Odd),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 3);
        assert!(pages[1].placements.is_empty(), "page 2 must be blank");
        assert_eq!(pages[2].atoms(), vec![1]);
    }

    #[test]
    fn an_odd_break_needs_no_blank_page_when_the_next_page_is_already_odd() {
        // Two full pages of content, so the next page is 3 — already odd.
        let atoms = vec![
            Atom::new(Pt(100.0)),
            Atom::new(Pt(100.0)),
            Atom::new(Pt(10.0)).break_before(Break::Odd),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 3);
        assert_eq!(pages[2].atoms(), vec![2]);
    }

    #[test]
    fn an_even_break_inserts_a_blank_page_when_the_next_page_would_be_odd() {
        let atoms = vec![
            Atom::new(Pt(100.0)),
            Atom::new(Pt(100.0)),
            Atom::new(Pt(10.0)).break_before(Break::Even),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 4);
        assert!(pages[2].placements.is_empty(), "page 3 must be blank");
        assert_eq!(pages[3].atoms(), vec![2]);
    }

    #[test]
    fn a_parity_break_at_the_start_of_the_document_respects_the_parity() {
        // Page 1 is odd, so `Odd` on the first atom needs no blank page,
        // but `Even` does.
        let odd = pack(
            &Flow::new(&[Atom::new(Pt(10.0)).break_before(Break::Odd)]),
            Pt(100.0),
        );
        assert_eq!(odd.len(), 1);

        let even = pack(
            &Flow::new(&[Atom::new(Pt(10.0)).break_before(Break::Even)]),
            Pt(100.0),
        );
        assert_eq!(even.len(), 2);
        assert!(even[0].placements.is_empty());
        assert_eq!(even[1].atoms(), vec![0]);
    }

    #[test]
    fn a_forced_break_applies_to_the_whole_keep_with_next_run() {
        let atoms = vec![
            Atom::new(Pt(10.0)),
            Atom::new(Pt(10.0))
                .break_before(Break::Always)
                .keep_with_next(),
            Atom::new(Pt(10.0)),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].atoms(), vec![1, 2]);
    }

    // ── continuation groups ─────────────────────────────────────────────
    // The generic mechanism behind a repeated table header. The packer never
    // learns what a table is; it reserves space for a prefix the primitive
    // chose to declare.

    /// A group over `atoms`, repeating a prefix of `prefix` points.
    fn group(range: std::ops::Range<usize>, prefix: Option<f32>) -> Group {
        let atom = range.start;
        Group {
            atoms: range,
            repeat_prefix: prefix.map(|h| Repeat {
                atom,
                height: Pt(h),
            }),
        }
    }

    #[test]
    fn a_group_that_fits_on_one_page_never_continues() {
        let atoms = atoms_of_height(3, 10.0);
        let groups = [group(0..3, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages.len(), 1);
        assert!(pages[0].continuations.is_empty());
    }

    #[test]
    fn a_continuation_names_the_atom_the_group_asked_to_repeat() {
        // The painter looks the atom up; the packer never sees its content.
        let atoms = atoms_of_height(12, 10.0);
        let groups = [Group {
            atoms: 0..12,
            repeat_prefix: Some(Repeat {
                atom: 0,
                height: Pt(10.0),
            }),
        }];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages[1].continuations[0].atom, 0);
    }

    #[test]
    fn a_group_split_across_pages_records_a_continuation_on_the_second() {
        // Twelve rows of 10 into a budget of 100 spill onto a second page.
        let atoms = atoms_of_height(12, 10.0);
        let groups = [group(0..12, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert!(pages[0].continuations.is_empty(), "it starts here");
        assert_eq!(
            pages[1].continuations,
            vec![Continuation {
                group: 0,
                atom: 0,
                y: Pt(0.0),
                height: Pt(10.0)
            }]
        );
    }

    #[test]
    fn a_repeated_prefix_pushes_the_continued_content_down() {
        let atoms = atoms_of_height(12, 10.0);
        let groups = [group(0..12, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        // The prefix occupies y=0..10, so continued rows start at y=10.
        assert_eq!(pages[1].placements[0].y, Pt(10.0));
    }

    #[test]
    fn a_repeated_prefix_consumes_budget_so_fewer_rows_fit() {
        // Without a prefix, 10 rows fit per page and 12 rows need 2 pages.
        // A 10pt prefix leaves room for 9 rows on every continuation page.
        let atoms = atoms_of_height(21, 10.0);
        let groups = [group(0..21, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages[0].atoms().len(), 10, "no prefix on the first page");
        assert_eq!(pages[1].atoms().len(), 9, "prefix eats one row");
        assert_eq!(pages[2].atoms().len(), 2);
    }

    #[test]
    fn a_group_spanning_three_pages_repeats_its_prefix_on_both_continuations() {
        let atoms = atoms_of_height(21, 10.0);
        let groups = [group(0..21, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages.len(), 3);
        assert!(pages[0].continuations.is_empty());
        assert_eq!(pages[1].continuations.len(), 1);
        assert_eq!(pages[2].continuations.len(), 1);
    }

    #[test]
    fn a_group_opting_out_of_repetition_continues_without_a_prefix() {
        // This is `repeatHeader={false}`: the group still spans pages, it just
        // does not reserve anything at the top.
        let atoms = atoms_of_height(12, 10.0);
        let groups = [group(0..12, None)];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert!(pages[1].continuations.is_empty());
        assert_eq!(pages[1].placements[0].y, Pt(0.0), "nothing reserved");
    }

    #[test]
    fn atoms_outside_every_group_are_unaffected() {
        // Six loose rows, then a 12-row grouped table.
        let atoms = atoms_of_height(18, 10.0);
        let groups = [group(6..18, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert!(pages[0].continuations.is_empty());
        assert_eq!(pages[0].atoms().len(), 10);
        // The group started on page 1 and continues, so page 2 gets a prefix.
        assert_eq!(pages[1].continuations.len(), 1);
    }

    #[test]
    fn a_group_starting_exactly_at_a_page_boundary_is_not_a_continuation() {
        // Ten loose rows fill page 1; the group starts fresh on page 2 and
        // must not claim to be continued from anywhere.
        let atoms = atoms_of_height(15, 10.0);
        let groups = [group(10..15, Some(10.0))];

        let pages = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));

        assert_eq!(pages.len(), 2);
        assert!(
            pages[1].continuations.is_empty(),
            "the group begins on this page, it does not continue onto it"
        );
    }

    // ── geometric invariants ────────────────────────────────────────────
    // Properties that must hold for *any* document, checked over generated
    // ones rather than a handful of fixtures. An overlap of a third of a
    // point is invisible on screen and wrong on paper, so the eye is not a
    // sufficient check.

    /// Deterministic pseudo-random source. A seeded LCG rather than a crate:
    /// a failing case must be reproducible from its seed alone.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn range(&mut self, lo: u32, hi: u32) -> u32 {
            lo + (self.next() as u32) % (hi - lo)
        }
    }

    /// A document whose runs all fit on a page, so any overflow is a defect
    /// rather than the unavoidable oversized-run case.
    fn generated_document(seed: u64, count: usize, budget: Pt) -> Vec<Atom> {
        let mut rng = Rng(seed);
        let mut atoms = Vec::with_capacity(count);
        let mut run_height = 0.0f32;

        while atoms.len() < count {
            let height = rng.range(4, 40) as f32;
            // Keep runs comfortably inside the budget.
            let can_extend = run_height + height < budget.get() * 0.4;
            let keep = can_extend && rng.range(0, 100) < 30;

            let mut atom = Atom::new(Pt(height));
            atom.keep_with_next = keep;
            if rng.range(0, 100) < 3 {
                atom.break_before = match rng.range(0, 3) {
                    0 => Break::Always,
                    1 => Break::Odd,
                    _ => Break::Even,
                };
            }
            atoms.push(atom);

            run_height = if keep { run_height + height } else { 0.0 };
        }
        // A trailing keep-with-next has nothing to hold on to.
        if let Some(last) = atoms.last_mut() {
            last.keep_with_next = false;
        }
        atoms
    }

    #[test]
    fn no_two_atoms_on_a_page_ever_overlap() {
        let budget = Pt(700.0);
        for seed in 0..50 {
            let atoms = generated_document(seed, 300, budget);
            for page in pack(&Flow::new(&atoms), budget) {
                let mut previous_bottom = 0.0f32;
                for placement in &page.placements {
                    let top = placement.y.get();
                    assert!(
                        top >= previous_bottom - 1e-3,
                        "seed {seed}: atom {} starts at {top} but the one above ends at {previous_bottom}",
                        placement.atom
                    );
                    previous_bottom = top + atoms[placement.atom].height.get();
                }
            }
        }
    }

    #[test]
    fn no_page_of_ordinary_content_overflows_its_budget() {
        let budget = Pt(700.0);
        for seed in 0..50 {
            let atoms = generated_document(seed, 300, budget);
            for (i, page) in pack(&Flow::new(&atoms), budget).iter().enumerate() {
                let Some(last) = page.placements.last() else {
                    continue;
                };
                let bottom = last.y.get() + atoms[last.atom].height.get();
                assert!(
                    bottom <= budget.get() + 1e-3,
                    "seed {seed}: page {} ends at {bottom}, budget is {}",
                    i + 1,
                    budget.get()
                );
            }
        }
    }

    #[test]
    fn every_atom_is_placed_exactly_once_and_in_order() {
        let budget = Pt(700.0);
        for seed in 0..50 {
            let atoms = generated_document(seed, 300, budget);
            let placed: Vec<usize> = pack(&Flow::new(&atoms), budget)
                .iter()
                .flat_map(|p| p.atoms())
                .collect();

            assert_eq!(
                placed,
                (0..atoms.len()).collect::<Vec<_>>(),
                "seed {seed}: atoms lost, duplicated or reordered"
            );
        }
    }

    #[test]
    fn a_page_is_never_left_empty_unless_a_parity_break_asked_for_it() {
        let budget = Pt(700.0);
        for seed in 0..50 {
            let atoms = generated_document(seed, 300, budget);
            let pages = pack(&Flow::new(&atoms), budget);

            for (i, page) in pages.iter().enumerate() {
                if !page.placements.is_empty() {
                    continue;
                }
                let next_atom = pages[i + 1..]
                    .iter()
                    .find_map(|p| p.placements.first())
                    .map(|pl| atoms[pl.atom].break_before);
                assert!(
                    matches!(next_atom, Some(Break::Odd) | Some(Break::Even)),
                    "seed {seed}: page {} is blank for no reason",
                    i + 1
                );
            }
        }
    }

    // ── page accumulators ───────────────────────────────────────────────
    // Running totals evaluated in packing order. This can only live here:
    // the packer is the one pass that walks the document in order knowing
    // which page each atom landed on. Slice the document up and render the
    // pieces separately — as a chunk-and-merge pipeline must — and the
    // information needed to produce these numbers is gone.
    //
    // "Suma y sigue" / "carried forward" is the accounting instance; a
    // catalogue's "items 51–100 of 3,482" is the same mechanism.

    fn contributing(atoms: &[usize], value: f64) -> Vec<Contribution> {
        atoms
            .iter()
            .map(|&atom| Contribution {
                atom,
                accumulator: 0,
                value,
            })
            .collect()
    }

    #[test]
    fn a_flow_can_continue_the_totals_of_the_one_before_it() {
        // A document composed in segments packs each separately. Restarting
        // the totals would make "carried forward" wrong from page two on.
        let atoms = atoms_of_height(3, 10.0);
        let c = contributing(&[0, 1, 2], 5.0);

        let pages = pack(
            &Flow::new(&atoms)
                .with_accumulators(1, &c)
                .continuing_from(&[1000.0]),
            Pt(100.0),
        );

        assert_eq!(pages[0].opening, vec![1000.0]);
        assert_eq!(pages[0].closing, vec![1015.0]);
    }

    #[test]
    fn a_document_without_accumulators_reports_none() {
        let pages = pack(&Flow::new(&atoms_of_height(3, 10.0)), Pt(100.0));

        assert!(pages[0].opening.is_empty());
        assert!(pages[0].closing.is_empty());
    }

    #[test]
    fn the_first_page_opens_at_zero() {
        let atoms = atoms_of_height(3, 10.0);
        let c = contributing(&[0, 1, 2], 5.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert_eq!(pages[0].opening, vec![0.0]);
    }

    #[test]
    fn a_page_closes_with_the_sum_of_what_it_carried() {
        let atoms = atoms_of_height(3, 10.0);
        let c = contributing(&[0, 1, 2], 5.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert_eq!(pages[0].closing, vec![15.0]);
    }

    #[test]
    fn the_next_page_opens_where_the_previous_one_closed() {
        // This is "suma y sigue" / "brought forward": twelve rows of 5.00,
        // ten to a page.
        let atoms = atoms_of_height(12, 10.0);
        let c = contributing(&(0..12).collect::<Vec<_>>(), 5.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert_eq!(pages[0].closing, vec![50.0], "carried forward");
        assert_eq!(pages[1].opening, vec![50.0], "brought forward");
        assert_eq!(pages[1].closing, vec![60.0]);
    }

    #[test]
    fn a_page_that_contributes_nothing_closes_where_it_opened() {
        let atoms = atoms_of_height(12, 10.0);
        let c = contributing(&[0], 7.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert_eq!(pages[1].opening, vec![7.0]);
        assert_eq!(pages[1].closing, vec![7.0]);
    }

    #[test]
    fn accumulators_are_independent_of_each_other() {
        // Debit and credit columns, tracked side by side.
        let atoms = atoms_of_height(2, 10.0);
        let c = [
            Contribution {
                atom: 0,
                accumulator: 0,
                value: 100.0,
            },
            Contribution {
                atom: 1,
                accumulator: 1,
                value: 40.0,
            },
        ];

        let pages = pack(&Flow::new(&atoms).with_accumulators(2, &c), Pt(100.0));

        assert_eq!(pages[0].closing, vec![100.0, 40.0]);
    }

    #[test]
    fn a_declared_accumulator_with_no_contributions_still_reports_zero() {
        let atoms = atoms_of_height(2, 10.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(2, &[]), Pt(100.0));

        assert_eq!(pages[0].closing, vec![0.0, 0.0]);
    }

    #[test]
    fn totals_follow_atoms_that_a_break_rule_moved_to_another_page() {
        // The property that makes this belong in the packer rather than
        // anywhere upstream. Nine rows fill y=0..90; the heading and its row
        // form a 20pt run that will not fit, so both move to page 2 — and so
        // must their contributions. A pipeline that summed by index position
        // instead of by placement would close page 1 at 509.
        let mut atoms = atoms_of_height(9, 10.0);
        atoms.push(Atom::new(Pt(10.0)).keep_with_next()); // 9
        atoms.push(Atom::new(Pt(10.0))); //                  10

        let mut c = contributing(&(0..9).collect::<Vec<_>>(), 1.0);
        c.push(Contribution {
            atom: 9,
            accumulator: 0,
            value: 500.0,
        });
        c.push(Contribution {
            atom: 10,
            accumulator: 0,
            value: 1.0,
        });

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert_eq!(pages[0].atoms().len(), 9);
        assert_eq!(pages[0].closing, vec![9.0], "the moved atoms took theirs");
        assert_eq!(pages[1].opening, vec![9.0]);
        assert_eq!(pages[1].closing, vec![510.0]);
    }

    #[test]
    fn a_blank_parity_page_carries_the_running_total_through_unchanged() {
        let atoms = vec![
            Atom::new(Pt(10.0)),
            Atom::new(Pt(10.0)).break_before(Break::Odd),
        ];
        let c = contributing(&[0, 1], 3.0);

        let pages = pack(&Flow::new(&atoms).with_accumulators(1, &c), Pt(100.0));

        assert!(pages[1].placements.is_empty(), "the blank page");
        assert_eq!(pages[1].opening, vec![3.0]);
        assert_eq!(pages[1].closing, vec![3.0]);
        assert_eq!(pages[2].opening, vec![3.0]);
        assert_eq!(pages[2].closing, vec![6.0]);
    }

    #[test]
    fn one_atom_may_feed_several_accumulators() {
        let atoms = atoms_of_height(1, 10.0);
        let c = [
            Contribution {
                atom: 0,
                accumulator: 0,
                value: 12.5,
            },
            Contribution {
                atom: 0,
                accumulator: 1,
                value: 1.0,
            },
        ];

        let pages = pack(&Flow::new(&atoms).with_accumulators(2, &c), Pt(100.0));

        assert_eq!(pages[0].closing, vec![12.5, 1.0]);
    }

    #[test]
    fn a_keep_with_next_run_too_tall_for_any_page_is_still_placed() {
        // Pathological input must terminate, not spin forever looking for a
        // page big enough. Whether it looks good is the author's problem;
        // whether the engine hangs is ours.
        let atoms = vec![
            Atom::new(Pt(80.0)).keep_with_next(),
            Atom::new(Pt(80.0)).keep_with_next(),
            Atom::new(Pt(80.0)),
        ];

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        let placed: usize = pages.iter().map(|p| p.placements.len()).sum();
        assert_eq!(placed, 3, "every atom must end up somewhere");
    }

    #[test]
    fn a_group_already_under_way_repeats_on_the_first_page_of_a_resumed_flow() {
        // A document composed in segments packs each one on its own. The
        // second segment opens partway through a table, so its very first
        // page is a continuation page and needs the header — but nothing in
        // the atoms says so, because the rows that came before were painted
        // and dropped two hundred pages ago.
        let atoms = vec![Atom::new(Pt(10.0)); 30];
        let groups = [Group {
            atoms: 0..30,
            repeat_prefix: Some(Repeat {
                atom: 0,
                height: Pt(10.0),
            }),
        }];

        let cold = pack(&Flow::new(&atoms).with_groups(&groups), Pt(100.0));
        let resumed = pack(
            &Flow::new(&atoms).with_groups(&groups).resuming(&[true]),
            Pt(100.0),
        );

        assert!(
            cold[0].continuations.is_empty(),
            "a group starting here is not being continued"
        );
        assert_eq!(
            resumed[0].continuations.len(),
            1,
            "the first page of a resumed flow lost its repeated header"
        );
        assert_eq!(resumed[0].continuations[0].atom, 0);
    }

    #[test]
    fn resuming_says_nothing_about_groups_it_does_not_mention() {
        let atoms = vec![Atom::new(Pt(10.0)); 30];
        let groups = [Group {
            atoms: 0..30,
            repeat_prefix: Some(Repeat {
                atom: 0,
                height: Pt(10.0),
            }),
        }];

        let pages = pack(
            &Flow::new(&atoms).with_groups(&groups).resuming(&[false]),
            Pt(100.0),
        );

        assert!(pages[0].continuations.is_empty());
        assert!(
            !pages[1].continuations.is_empty(),
            "later pages still get it"
        );
    }
}
