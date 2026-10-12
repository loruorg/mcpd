//! Bottom-k MinHash sketches over the wording of an aspect note.
//!
//! This is what a theme is built from, and it is the whole reason mcpd needs no
//! model and no embedding to group notes. A sketch is 64 integers derived from
//! a sentence; two sketches are compared by counting how many of those integers
//! they share; and **there is not one floating-point operation anywhere in
//! it**, so two replicas of one deployment reading the same notes cannot
//! disagree about where a cluster boundary falls.
//!
//! Why not embeddings. An embedding produced outside the daemon is opaque to
//! it: the daemon cannot tell which embedding space a vector is in, so two
//! differently configured workers, or one upgrade of an embedder, produce a
//! nonsense clustering that the daemon then reports with full confidence. A
//! sketch is computed here, from text this daemon already stores, so it is
//! always in the same space as every other sketch it is compared against.
//!
//! What this costs the reader, stated plainly because the documentation says it
//! too: a sketch compares wording and not meaning, so two notes that mean the
//! same thing in different words land in separate themes.

/// How many hashes a sketch keeps.
///
/// 64 is the width the whole design is quoted in: a similarity is reported to
/// callers as a count out of 64, `ThemeExample.similarity` is that count, and
/// the clustering threshold is stated in the same units. Changing it changes
/// the meaning of a published field, so it is not a tuning knob.
///
/// It is also 512 bytes per note on disk, which is the size the storage columns
/// were chosen for.
pub const SKETCH_LEN: usize = 64;

/// The bytes one sketch occupies, big-endian, in ascending order of hash.
pub const SKETCH_BYTES: usize = SKETCH_LEN * 8;

/// The seed the hash starts from.
///
/// Fixed, published in the source, and never derived from anything about the
/// deployment. A per-deployment seed would mean two replicas of one deployment
/// could not compare sketches at all, and a per-process seed would mean a
/// restart invalidated every sketch on disk.
const SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// The FNV-1a prime, for the same 64-bit variant the seed above comes from.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// A bottom-k MinHash sketch: the 64 smallest hashes of a note's shingles.
///
/// Held sorted ascending and padded with `u64::MAX`, so a sketch of a very
/// short sentence is still 64 wide and every comparison below is a merge of two
/// sorted runs rather than a set lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sketch {
    hashes: [u64; SKETCH_LEN],
    /// How many of the 64 slots hold a real hash rather than padding.
    filled: usize,
}

impl Sketch {
    /// Sketch one note's text.
    ///
    /// The pipeline is: fold to lowercase, treat everything that is not a
    /// letter or a digit as a separator, then take every word and every
    /// adjacent pair of words as a shingle. Pairs matter: on words alone,
    /// "refund the order" and "order the refund" are the same set, and those
    /// are two different things for a person reading a themes screen.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let words: Vec<&str> =
            text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();

        let mut smallest = Bottom::new();
        for (i, w) in words.iter().enumerate() {
            smallest.offer(hash_word(w));
            if let Some(next) = words.get(i + 1) {
                smallest.offer(hash_pair(w, next));
            }
        }
        smallest.finish()
    }

    /// Whether this sketch came from text with no words in it at all.
    ///
    /// An empty sketch is like nothing, itself included: every comparison with
    /// it is zero, so a note whose text is punctuation cannot pull a theme
    /// together around it.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// How alike two sketches are, out of [`SKETCH_LEN`].
    ///
    /// This is the bottom-k estimator, not a naive intersection of the two
    /// stored lists. Take the smallest hashes of the two sketches together, up
    /// to 64 of them, and count how many of those appear in both. Counting the
    /// raw intersection instead would bias every comparison downwards whenever
    /// the two notes are different lengths, because the longer note's 64
    /// smallest hashes reach less far up the hash space than the shorter note's
    /// do.
    ///
    /// The count is then scaled to a denominator of 64. That step is what makes
    /// the number readable, and leaving it out is a real defect rather than a
    /// nicety: a note with fifteen shingles in it has fifteen slots filled, so
    /// two identical copies of it can share at most fifteen, and an unscaled
    /// answer would report two identical sentences as 15 of 64 and drop them
    /// either side of any threshold worth setting. Scaled, identical wording is
    /// 64 whatever the sentence's length.
    ///
    /// The scaling is `both * 64 / taken` in integers, truncating. **No
    /// floating-point arithmetic**, so every replica computes the same value
    /// and a cluster boundary cannot fall in two places.
    #[must_use]
    pub fn shared(&self, other: &Self) -> u32 {
        if self.is_empty() || other.is_empty() {
            return 0;
        }
        let (mut i, mut j, mut taken, mut both) = (0usize, 0usize, 0u32, 0u32);
        // One merge over the two sorted runs, stopping after 64 distinct values
        // have been taken from the union. Both padded tails are `u64::MAX`, and
        // the `taken` bound is reached long before them on any real sketch;
        // the explicit length checks are what stop a short sketch walking off
        // the end.
        while (taken as usize) < SKETCH_LEN && (i < self.filled || j < other.filled) {
            let a = if i < self.filled { self.hashes[i] } else { u64::MAX };
            let b = if j < other.filled { other.hashes[j] } else { u64::MAX };
            match a.cmp(&b) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                    both += 1;
                }
            }
            taken += 1;
        }
        if taken == 0 {
            return 0;
        }
        both * SKETCH_LEN as u32 / taken
    }

    /// The key for one band of the prefilter.
    ///
    /// The 64 positions are cut into [`BANDS`] runs of equal length, and two
    /// sketches are worth comparing in full when any one run matches exactly.
    /// That is what keeps a pass from comparing every note against every other
    /// note, which at the 20,000 note ceiling is two hundred million
    /// comparisons.
    ///
    /// Returns `None` for a band that is entirely padding, so a two-word note
    /// does not become a candidate for every other short note by agreeing on a
    /// run of `u64::MAX`.
    #[must_use]
    pub fn band(&self, band: usize) -> Option<u64> {
        let width = SKETCH_LEN / BANDS;
        let start = band * width;
        if start >= self.filled {
            return None;
        }
        let mut h = SEED;
        for slot in start..start + width {
            let v = if slot < self.filled { self.hashes[slot] } else { u64::MAX };
            h = mix(h, v);
        }
        Some(h)
    }

    /// The sketch as stored: 64 big-endian integers, ascending.
    ///
    /// Big-endian so the byte order matches the numeric order, which means a
    /// backend that has to break a tie on the raw bytes of a sketch breaks it
    /// the same way the arithmetic here would.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SKETCH_BYTES);
        for slot in 0..SKETCH_LEN {
            let v = if slot < self.filled { self.hashes[slot] } else { u64::MAX };
            out.extend_from_slice(&v.to_be_bytes());
        }
        out
    }

    /// Read a stored sketch back.
    ///
    /// `None` when the blob is not the right length or is not sorted. Both are
    /// treated as absent rather than as an error, so a row that predates this
    /// column, or one a backend truncated, makes the pass recompute the sketch
    /// from the text instead of failing the whole pass.
    #[must_use]
    pub fn from_bytes(raw: &[u8]) -> Option<Self> {
        if raw.len() != SKETCH_BYTES {
            return None;
        }
        let mut hashes = [u64::MAX; SKETCH_LEN];
        let mut filled = 0;
        let mut previous = 0u64;
        for (slot, chunk) in raw.chunks_exact(8).enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(chunk);
            let v = u64::from_be_bytes(b);
            if slot > 0 && v < previous {
                return None;
            }
            previous = v;
            hashes[slot] = v;
            if v != u64::MAX {
                filled = slot + 1;
            }
        }
        Some(Self { hashes, filled })
    }
}

/// How many runs the prefilter cuts a sketch into.
///
/// Eight runs of eight. More bands means more candidate pairs and a slower
/// pass; fewer means a pair that is genuinely similar can miss every band and
/// be scored as unrelated. Eight is the middle of that and it divides 64
/// exactly, which the band arithmetic above relies on.
pub const BANDS: usize = 8;

/// The 64 smallest hashes seen so far, held as a sorted array.
///
/// A binary heap would be the usual choice. This is an insertion into a sorted
/// 64-slot array instead, because the array has to come out sorted anyway, 64
/// is small enough that the memory move is cheaper than the heap's pointer
/// chasing, and duplicates have to be rejected, which a heap makes awkward.
struct Bottom {
    hashes: [u64; SKETCH_LEN],
    filled: usize,
}

impl Bottom {
    const fn new() -> Self {
        Self { hashes: [u64::MAX; SKETCH_LEN], filled: 0 }
    }

    /// Offer one shingle hash.
    fn offer(&mut self, h: u64) {
        // Full, and this is no smaller than the largest kept: nothing to do.
        if self.filled == SKETCH_LEN && h >= self.hashes[SKETCH_LEN - 1] {
            return;
        }
        // Where it belongs, and whether it is already there. A repeated shingle
        // must not take two of the 64 slots: a sentence that says one word
        // twice would otherwise sketch as if it were half as varied as it is.
        let at = self.hashes[..self.filled].partition_point(|&v| v < h);
        if at < self.filled && self.hashes[at] == h {
            return;
        }
        if self.filled < SKETCH_LEN {
            self.filled += 1;
        }
        self.hashes.copy_within(at..SKETCH_LEN - 1, at + 1);
        self.hashes[at] = h;
    }

    const fn finish(self) -> Sketch {
        Sketch { hashes: self.hashes, filled: self.filled }
    }
}

/// FNV-1a over one word, folded to lowercase as it goes.
///
/// Lowercasing here rather than allocating a lowercase copy of the whole text
/// first: the ASCII fold covers the case that matters and costs nothing, and a
/// note is hashed once per pass over tens of thousands of them.
fn hash_word(word: &str) -> u64 {
    let mut h = SEED;
    for b in word.bytes() {
        h = mix(h, u64::from(b.to_ascii_lowercase()));
    }
    h
}

/// The same, over a pair of adjacent words with a separator between them.
///
/// The separator is a byte no word can contain, so `hash_pair("ab", "c")` and
/// `hash_pair("a", "bc")` are different, which they must be.
fn hash_pair(first: &str, second: &str) -> u64 {
    let mut h = SEED;
    for b in first.bytes() {
        h = mix(h, u64::from(b.to_ascii_lowercase()));
    }
    h = mix(h, 0xff);
    for b in second.bytes() {
        h = mix(h, u64::from(b.to_ascii_lowercase()));
    }
    h
}

/// One FNV-1a step, then a final avalanche so the low bits move.
///
/// FNV-1a on its own leaves the low bits of a short input poorly mixed, and a
/// bottom-k sketch is exactly a selection on the low end of the hash space, so
/// unmixed low bits would make short words far more likely to be kept than long
/// ones. `wrapping_*` throughout: this is a hash, and overflow is the point.
const fn mix(h: u64, v: u64) -> u64 {
    let mut x = (h ^ v).wrapping_mul(PRIME);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_sentence_sketches_identically_every_time() {
        let a = Sketch::of("the customer wants a refund for order 4471");
        let b = Sketch::of("the customer wants a refund for order 4471");
        assert_eq!(a, b);
        assert_eq!(a.shared(&b), SKETCH_LEN as u32);
    }

    #[test]
    fn case_and_punctuation_do_not_change_a_sketch() {
        let a = Sketch::of("The customer wants a refund.");
        let b = Sketch::of("the  customer, wants a REFUND!");
        assert_eq!(a, b);
    }

    #[test]
    fn word_order_changes_a_sketch() {
        // Word bags alone cannot tell these apart, which is why pairs are
        // sketched as well as words.
        let a = Sketch::of("refund the order");
        let b = Sketch::of("order the refund");
        assert_ne!(a, b);
        assert!(a.shared(&b) < SKETCH_LEN as u32);
    }

    #[test]
    fn unrelated_sentences_share_almost_nothing() {
        let a = Sketch::of("the customer wants a refund for a late delivery");
        let b = Sketch::of("please book me a flight to lisbon on tuesday morning");
        assert!(a.shared(&b) < 8, "shared {} of 64", a.shared(&b));
    }

    #[test]
    fn a_rewording_still_scores_well_above_an_unrelated_sentence() {
        let a = Sketch::of("the customer wants a refund for a late delivery");
        let b = Sketch::of("the customer wants a refund for a delivery that was late");
        let unrelated = Sketch::of("please book me a flight to lisbon on tuesday morning");
        assert!(
            a.shared(&b) > a.shared(&unrelated) * 3,
            "{} vs {}",
            a.shared(&b),
            a.shared(&unrelated)
        );
    }

    #[test]
    fn an_empty_sketch_is_like_nothing_including_itself() {
        let empty = Sketch::of("!!! ...");
        assert!(empty.is_empty());
        assert_eq!(empty.shared(&empty), 0);
        assert_eq!(empty.shared(&Sketch::of("a real sentence")), 0);
    }

    #[test]
    fn a_sketch_survives_a_round_trip_through_its_bytes() {
        let s = Sketch::of("the customer wants a refund for order 4471");
        let bytes = s.to_bytes();
        assert_eq!(bytes.len(), SKETCH_BYTES);
        assert_eq!(Sketch::from_bytes(&bytes), Some(s));
    }

    #[test]
    fn a_blob_of_the_wrong_shape_reads_as_absent_rather_than_failing() {
        assert_eq!(Sketch::from_bytes(&[]), None);
        assert_eq!(Sketch::from_bytes(&[0u8; 8]), None);
        // The right length, out of order: a sketch is held ascending, and
        // reading an unsorted one would make every comparison wrong in a way
        // no caller could see.
        let mut wrong = vec![0u8; SKETCH_BYTES];
        wrong[7] = 9;
        assert_eq!(Sketch::from_bytes(&wrong), None);
    }

    #[test]
    fn a_repeated_word_does_not_take_two_slots() {
        let once = Sketch::of("refund refund refund refund");
        let twice = Sketch::of("refund refund");
        // "refund" and the pair "refund refund" are the only two shingles in
        // either sentence, however many times it is repeated.
        assert_eq!(once, twice);
        assert_eq!(once.shared(&twice), SKETCH_LEN as u32);
    }

    #[test]
    fn a_short_sentence_that_matches_exactly_still_scores_full_marks() {
        // The defect the scaling exists to prevent: this sentence fills far
        // fewer than 64 slots, and without scaling two identical copies of it
        // would be reported as being about a quarter alike.
        let a = Sketch::of("cancel my subscription");
        let b = Sketch::of("cancel my subscription");
        assert_eq!(a.shared(&b), SKETCH_LEN as u32);
    }

    #[test]
    fn similarity_never_exceeds_the_width_it_is_quoted_in() {
        for (x, y) in [
            ("one", "one"),
            ("a much longer sentence with a good many words in it indeed", "a"),
            ("", ""),
        ] {
            let s = Sketch::of(x).shared(&Sketch::of(y));
            assert!(s <= SKETCH_LEN as u32, "{x:?} vs {y:?} gave {s}");
        }
    }

    #[test]
    fn bands_are_absent_where_a_sketch_is_only_padding() {
        let tiny = Sketch::of("refund");
        assert!(tiny.band(0).is_some());
        assert!(tiny.band(BANDS - 1).is_none(), "a band of pure padding must not be a key");
    }

    #[test]
    fn identical_sketches_agree_on_every_band() {
        let a = Sketch::of("the customer wants a refund for order 4471 placed last tuesday");
        let b = Sketch::of("THE CUSTOMER WANTS A REFUND FOR ORDER 4471 PLACED LAST TUESDAY");
        for band in 0..BANDS {
            assert_eq!(a.band(band), b.band(band), "band {band}");
        }
    }
}
