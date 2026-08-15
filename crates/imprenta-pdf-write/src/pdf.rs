//! The finished file, still in the blocks it was written into.
//!
//! `Writer::finish` used to join the blocks into one `Vec<u8>` before anybody
//! had said they wanted it contiguous, and that join was the last place the
//! engine held a document twice — inside a WebAssembly module, whose linear
//! memory never shrinks, the peak *is* the footprint, and two copies of a
//! twenty-two megabyte file is what issue #7 measured. So the join moved from
//! the producer to the consumer: a [`Pdf`] hands out its blocks to whoever
//! can take them in pieces — the wasm boundary can, and so can a file — and
//! makes the contiguous copy only for a caller that dereferences it.
//!
//! For that caller nothing has changed. A `Pdf` compares, indexes, scans and
//! slices like the `Vec<u8>` it replaced, which is why the hundred-odd tests
//! that read the bytes did not have to.

use crate::blocks::Blocks;
use std::sync::OnceLock;

/// A finished PDF. The bytes are reachable three ways, from cheapest to
/// dearest: [`blocks`](Pdf::blocks) walks the pieces where they lie,
/// [`into_vec`](Pdf::into_vec) joins them draining, and dereferencing joins
/// them while keeping the pieces — the price of a `&self` that has to stay
/// valid.
#[derive(Default, Clone)]
pub struct Pdf {
    blocks: Blocks,
    /// The contiguous copy, made once and only on demand.
    joined: OnceLock<Vec<u8>>,
}

impl Pdf {
    /// No file. `const` so a thread-local on the far side of the wasm
    /// boundary can start with one without a lazy initialiser.
    pub const fn empty() -> Self {
        Self {
            blocks: Blocks::empty(),
            joined: OnceLock::new(),
        }
    }

    pub(crate) fn from_blocks(blocks: Blocks) -> Self {
        Self {
            blocks,
            joined: OnceLock::new(),
        }
    }

    /// The size of the file, however it is currently held.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match self.joined.get() {
            Some(joined) if self.blocks.count() == 0 => joined.len(),
            _ => self.blocks.len(),
        }
    }

    /// How many pieces [`block`](Pdf::block) can be asked for.
    pub fn block_count(&self) -> usize {
        if self.blocks.count() > 0 {
            self.blocks.count()
        } else {
            usize::from(self.joined.get().is_some_and(|j| !j.is_empty()))
        }
    }

    /// One piece of the file, by position. Concatenated in order, the pieces
    /// are the file. `None` past the end rather than a panic, because the
    /// index arrives over the wasm boundary where a panic is a trap.
    pub fn block(&self, index: usize) -> Option<&[u8]> {
        if self.blocks.count() > 0 {
            self.blocks.get(index)
        } else {
            match self.joined.get() {
                Some(joined) if !joined.is_empty() && index == 0 => Some(joined.as_slice()),
                _ => None,
            }
        }
    }

    /// The pieces in file order.
    pub fn blocks(&self) -> impl Iterator<Item = &[u8]> {
        (0..self.block_count()).filter_map(|i| self.block(i))
    }

    /// The whole file, contiguous, giving each block back as it drains — the
    /// path for a caller that wants to own the bytes and holds at most two
    /// copies for the length of one `memcpy`.
    pub fn into_vec(self) -> Vec<u8> {
        if self.blocks.count() > 0 {
            self.blocks.into_vec()
        } else {
            self.joined.into_inner().unwrap_or_default()
        }
    }
}

impl From<Vec<u8>> for Pdf {
    /// Bytes that are already contiguous — a merge result, a fixture — cost
    /// nothing to wrap and come back as one block.
    fn from(bytes: Vec<u8>) -> Self {
        let joined = OnceLock::new();
        let _ = joined.set(bytes);
        Self {
            blocks: Blocks::default(),
            joined,
        }
    }
}

/// The contiguous view, for everything written against the `Vec<u8>` this
/// type replaced: indexing, `windows`, `starts_with`, `from_utf8_lossy`.
/// Made once, kept for the life of the `Pdf`, and paid for only by callers
/// that ask — the wasm path never does.
impl std::ops::Deref for Pdf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        if self.blocks.count() == 0 {
            return self.joined.get().map_or(&[], Vec::as_slice);
        }
        self.joined.get_or_init(|| {
            let mut out = Vec::with_capacity(self.blocks.len());
            for block in self.blocks.iter() {
                out.extend_from_slice(block);
            }
            out
        })
    }
}

impl AsRef<[u8]> for Pdf {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

/// Equality walks the blocks where they lie — joining two files to discover
/// they differ in the header would be the copy this type exists to avoid.
impl PartialEq for Pdf {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let mut a = self.blocks().flatten();
        let mut b = other.blocks().flatten();
        loop {
            match (a.next(), b.next()) {
                (Some(x), Some(y)) if x == y => continue,
                (None, None) => return true,
                _ => return false,
            }
        }
    }
}

impl Eq for Pdf {}

impl PartialEq<Vec<u8>> for Pdf {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.len() == other.len() && self.blocks().flatten().eq(other.iter())
    }
}

impl PartialEq<Pdf> for Vec<u8> {
    fn eq(&self, other: &Pdf) -> bool {
        other == self
    }
}

/// The length and the shape, never the bytes: a failed assertion that printed
/// twenty-two megabytes of PDF would bury the part of the message that says
/// where the two differed.
impl std::fmt::Debug for Pdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pdf")
            .field("len", &self.len())
            .field("blocks", &self.block_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_blocks(pieces: &[&[u8]]) -> Pdf {
        let mut blocks = Blocks::default();
        for piece in pieces {
            blocks.push(piece);
        }
        Pdf::from_blocks(blocks)
    }

    #[test]
    fn the_blocks_concatenated_are_the_file_the_deref_shows() {
        let pdf = in_blocks(&[b"%PDF-1.7\n", b"1 0 obj\n", b"%%EOF"]);

        let mut walked = Vec::new();
        for block in pdf.blocks() {
            walked.extend_from_slice(block);
        }

        assert_eq!(walked, &pdf[..]);
        assert_eq!(pdf.len(), walked.len());
    }

    #[test]
    fn dereferencing_does_not_change_what_the_blocks_report() {
        let pdf = in_blocks(&[b"abc", b"def"]);
        let before = pdf.block_count();

        let _ = &pdf[..];

        assert_eq!(pdf.block_count(), before);
        assert_eq!(pdf.into_vec(), b"abcdef");
    }

    #[test]
    fn two_files_compare_across_different_block_boundaries() {
        // The same bytes cut differently: a streamed document and a declared
        // one may block differently and must still be equal.
        let one = in_blocks(&[b"abcd", b"ef"]);
        let other = Pdf::from(b"abcdef".to_vec());

        assert_eq!(one, other);
        assert_eq!(one, b"abcdef".to_vec());
        assert_ne!(one, Pdf::from(b"abcdXf".to_vec()));
        assert_ne!(one, Pdf::from(b"abcde".to_vec()));
    }

    #[test]
    fn a_wrapped_vec_is_one_block_and_costs_no_copy() {
        let pdf = Pdf::from(b"whole".to_vec());

        assert_eq!(pdf.block_count(), 1);
        assert_eq!(pdf.block(0), Some(b"whole".as_slice()));
        assert_eq!(pdf.block(1), None);
        assert_eq!(pdf.into_vec(), b"whole");
    }

    #[test]
    fn empty_is_zero_blocks_and_an_empty_slice() {
        let pdf = Pdf::default();

        assert_eq!(pdf.block_count(), 0);
        assert_eq!(pdf.len(), 0);
        assert_eq!(&pdf[..], b"");
        assert_eq!(Pdf::from(Vec::new()).block_count(), 0);
    }
}
