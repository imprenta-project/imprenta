//! Widow and orphan control.
//!
//! Both constraints reduce to `keep_with_next`, so the packer needs to know
//! nothing about them:
//!
//! * **orphans = n** — at least `n` lines must stay at the foot of a page.
//!   Marking the first `n - 1` lines `keep_with_next` makes a shorter tail
//!   impossible: the run cannot be split, so it moves whole.
//! * **widows = n** — at least `n` lines must carry to the top of the next
//!   page. Marking the last `n - 1` lines `keep_with_next` has the same
//!   effect from the other end.
//!
//! Keeping this out of the packer matters: the packer is the hardest code in
//! the engine, and every constraint expressed as an atom property instead of
//! a packer rule is one the packer never has to grow a branch for.

use crate::atom::Atom;

/// Marks the `keep_with_next` flags that enforce widow and orphan limits over
/// one breakable sequence — the lines of a paragraph, the rows of a group.
///
/// Values below 2 impose no constraint: a single line cannot be split.
pub fn apply_widows_orphans(lines: &mut [Atom], widows: u8, orphans: u8) {
    let n = lines.len();
    // The last line has nothing to keep with, so only the first `n - 1` are
    // ever candidates — which also makes a single line a no-op.
    let Some(breakable) = n.checked_sub(1).filter(|&b| b > 0) else {
        return;
    };

    let head = (orphans.saturating_sub(1) as usize).min(breakable);
    for line in &mut lines[..head] {
        line.keep_with_next = true;
    }

    let tail = (widows.saturating_sub(1) as usize).min(breakable);
    for line in &mut lines[breakable - tail..breakable] {
        line.keep_with_next = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Flow, pack};
    use imprenta_core::units::Pt;

    fn lines(count: usize) -> Vec<Atom> {
        (0..count).map(|_| Atom::new(Pt(10.0))).collect()
    }

    fn marked(lines: &[Atom]) -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, a)| a.keep_with_next)
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn orphans_of_two_pins_the_first_line_to_the_second() {
        let mut p = lines(5);
        apply_widows_orphans(&mut p, 1, 2);
        assert_eq!(marked(&p), vec![0]);
    }

    #[test]
    fn widows_of_two_pins_the_last_line_to_the_one_before() {
        let mut p = lines(5);
        apply_widows_orphans(&mut p, 2, 1);
        assert_eq!(marked(&p), vec![3]);
    }

    #[test]
    fn both_constraints_apply_from_their_own_end() {
        let mut p = lines(5);
        apply_widows_orphans(&mut p, 2, 2);
        assert_eq!(marked(&p), vec![0, 3]);
    }

    #[test]
    fn larger_limits_pin_proportionally_more_lines() {
        let mut p = lines(8);
        apply_widows_orphans(&mut p, 3, 3);
        assert_eq!(marked(&p), vec![0, 1, 5, 6]);
    }

    #[test]
    fn limits_below_two_impose_nothing() {
        for (w, o) in [(0, 0), (1, 1), (1, 0)] {
            let mut p = lines(5);
            apply_widows_orphans(&mut p, w, o);
            assert!(marked(&p).is_empty(), "widows={w} orphans={o}");
        }
    }

    #[test]
    fn a_paragraph_shorter_than_its_limits_becomes_unbreakable() {
        // Three lines with widows=2 and orphans=2 cannot be split anywhere,
        // so every line but the last is pinned.
        let mut p = lines(3);
        apply_widows_orphans(&mut p, 2, 2);
        assert_eq!(marked(&p), vec![0, 1]);
    }

    #[test]
    fn a_single_line_is_never_marked() {
        let mut p = lines(1);
        apply_widows_orphans(&mut p, 3, 3);
        assert!(marked(&p).is_empty());
    }

    #[test]
    fn an_empty_sequence_is_left_alone() {
        let mut p: Vec<Atom> = Vec::new();
        apply_widows_orphans(&mut p, 2, 2);
        assert!(p.is_empty());
    }

    // ── behaviour once packed ───────────────────────────────────────────

    #[test]
    fn a_lone_first_line_at_the_foot_of_a_page_moves_with_its_paragraph() {
        // Nine rows fill y=0..90, leaving room for exactly one more line.
        // Without orphan control line 0 would sit alone at y=90.
        let mut atoms = lines(9);
        let mut paragraph = lines(5);
        apply_widows_orphans(&mut paragraph, 2, 2);
        atoms.extend(paragraph);

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages[0].atoms().len(), 9, "the paragraph moved whole");
        assert_eq!(pages[1].atoms(), vec![9, 10, 11, 12, 13]);
    }

    #[test]
    fn a_lone_last_line_is_pulled_down_to_join_its_predecessor() {
        // Six rows, then a 5-line paragraph: lines 0..3 fit (y=60..100) and
        // line 4 would sit alone atop page 2. Widow control moves line 3 too.
        let mut atoms = lines(6);
        let mut paragraph = lines(5);
        apply_widows_orphans(&mut paragraph, 2, 2);
        atoms.extend(paragraph);

        let pages = pack(&Flow::new(&atoms), Pt(100.0));

        assert_eq!(pages[0].atoms(), vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(pages[1].atoms(), vec![9, 10], "two lines carried over");
    }
}
