/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::mem;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::ops::RangeInclusive;
use std::ptr;

/// Compact bitset of small revision numbers.
///
/// Optimized for size - only one pointer wide for common linelog
/// use-cases (rev <= MAX_INLINE_REV).
///
/// Supports `insert`, `remove`, and `contains`.
/// Sparse or large rev numbers (e.g. rev 10000) waste O(rev) memory
/// since every bit below is materialized.
///
/// Uses pointer tagging on a single machine word:
/// - Bit 0 = 1: inline. Remaining bits represent revs 0..=(N-2)
///   where N = `InlineInt::BITS`.
/// - Bit 0 = 0: heap. Stores a `Box<Vec<HeapInt>>` raw pointer.
///   `vec[i]` covers `HeapInt::BITS` revs starting at `i * HeapInt::BITS`.
pub struct SmallRevs {
    bits: NonZeroUsize,
}

pub trait SmallRevRange {
    /// None: empty set; Some((start, end)): both inclusive.
    fn into_inclusive_bounds(self) -> Option<(usize, usize)>;
}

impl SmallRevRange for Range<usize> {
    fn into_inclusive_bounds(self) -> Option<(usize, usize)> {
        if self.start >= self.end {
            None
        } else {
            Some((self.start, self.end - 1))
        }
    }
}

impl SmallRevRange for RangeInclusive<usize> {
    fn into_inclusive_bounds(self) -> Option<(usize, usize)> {
        let (start, end) = self.into_inner();
        if start > end {
            None
        } else {
            Some((start, end))
        }
    }
}

type InlineInt = usize;
type HeapInt = u64;

macro_rules! mask_inclusive {
    ($T:ty, $lo:expr, $hi:expr) => {{
        let lo: usize = $lo;
        let hi: usize = $hi;
        let top: $T = if hi >= <$T>::BITS as usize - 1 {
            !0
        } else {
            (1 << (hi + 1)) - 1
        };
        let bot: $T = if lo == 0 { 0 } else { (1 << lo) - 1 };
        top & !bot
    }};
}

const INLINE_TAG: InlineInt = 1;
const MAX_INLINE_REV: usize = InlineInt::BITS as usize - 2;

const _: () = assert!(
    mem::size_of::<InlineInt>() == mem::size_of::<*mut Vec<HeapInt>>(),
    "inline bits and heap pointers must be the same size"
);

impl SmallRevs {
    pub const fn empty() -> Self {
        Self {
            bits: Self::inline_bits(INLINE_TAG),
        }
    }

    pub const fn empty_ref() -> &'static Self {
        static EMPTY: SmallRevs = SmallRevs::empty();
        &EMPTY
    }

    /// Construct from a finite range using bit manipulation. O(1) for
    /// inline, O(end / HeapInt::BITS) for heap — no per-element insertion.
    pub fn from_range(range: impl SmallRevRange) -> Self {
        let Some((start, last)) = range.into_inclusive_bounds() else {
            return Self::empty();
        };

        if last <= MAX_INLINE_REV {
            let lo = start + 1;
            let hi = last + 1;
            let mask = mask_inclusive!(InlineInt, lo, hi);
            return Self {
                bits: Self::inline_bits(INLINE_TAG | mask),
            };
        }

        let hb = HeapInt::BITS as usize;
        let start_word = start / hb;
        let end_word = last / hb;
        let mut vec: Vec<HeapInt> = vec![0; end_word + 1];

        if start_word == end_word {
            vec[start_word] = mask_inclusive!(HeapInt, start % hb, last % hb);
        } else {
            vec[start_word] = !0 << (start % hb);
            vec[start_word + 1..end_word].fill(!0);
            vec[end_word] = mask_inclusive!(HeapInt, 0, last % hb);
        }

        Self {
            bits: Self::heap_bits(vec),
        }
    }

    /// O(1) membership test.
    pub fn contains(&self, rev: usize) -> bool {
        let bits = HeapInt::BITS as usize;
        self.word(rev / bits) & (1 << (rev % bits)) != 0
    }

    /// O(1) for rev <= MAX_INLINE_REV,
    /// O(rev) otherwise due to `Vec` allocation.
    /// Not suitable for large rev values.
    pub fn insert(&mut self, rev: usize) {
        if self.is_inline() && rev <= MAX_INLINE_REV {
            self.bits = Self::inline_bits(self.inline_val() | (1 << (rev + 1)));
        } else {
            if self.is_inline() {
                self.promote_to_heap();
            }
            self.insert_heap(rev);
        }
    }

    /// Remove `rev` from the set.
    /// This does not demote heap storage back to inline storage.
    pub fn remove(&mut self, rev: usize) {
        if self.is_inline() {
            if rev <= MAX_INLINE_REV {
                self.set_inline_word(self.word(0) & !(1 << rev));
            }
            return;
        }

        let bits = HeapInt::BITS as usize;
        let (w, b) = (rev / bits, rev % bits);
        if let Some(word) = self.heap_mut().get_mut(w) {
            *word &= !(1 << b);
        }
    }

    /// Add all revisions from `other` to `self`.
    pub fn union_with(&mut self, other: &Self) {
        if self.is_inline() && other.is_inline() {
            self.set_inline_word(self.word(0) | other.word(0));
            return;
        }

        if self.is_inline() {
            self.promote_to_heap();
        }

        let n = other.word_count();
        let vec = self.heap_mut();
        if vec.len() < n {
            vec.resize(n, 0);
        }
        self.update_heap_words(other, n, |word, other_word| word | other_word);
    }

    /// Remove all revisions in `other` from `self`.
    pub fn difference_with(&mut self, other: &Self) {
        if self.is_inline() {
            self.set_inline_word(self.word(0) & !other.word(0));
            return;
        }

        let n = self.word_count().min(other.word_count());
        self.update_heap_words(other, n, |word, other_word| word & !other_word);
    }

    /// Keep only revisions that are also in `other`.
    /// This does not demote heap storage back to inline storage.
    pub fn intersect_with(&mut self, other: &Self) {
        if self.is_inline() {
            self.set_inline_word(self.word(0) & other.word(0));
            return;
        }

        let n = other.word_count();
        let vec = self.heap_mut();
        if vec.len() > n {
            vec[n..].fill(0);
        }
        self.update_heap_words(other, n, |word, other_word| word & other_word);
    }

    /// Iterate over set revisions in ascending order.
    /// Use `.rev()` to iterate in descending order.
    pub fn iter(&self) -> RevsIter<'_> {
        let word_count = self.word_count();
        let back_word_idx = word_count.saturating_sub(1);
        RevsIter {
            revs: self,
            front_word_idx: 0,
            front_remaining: self.word(0),
            back_word_idx,
            back_remaining: self.word(back_word_idx),
        }
    }

    pub fn is_empty(&self) -> bool {
        if self.is_inline() {
            return self.word(0) == 0;
        }

        self.heap_ref().iter().all(|&word| word == 0)
    }

    /// Count set revisions.
    pub fn len(&self) -> usize {
        if self.is_inline() {
            return self.word(0).count_ones() as usize;
        }

        self.heap_ref()
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    fn is_inline(&self) -> bool {
        const _: () = assert!(
            mem::align_of::<Vec<HeapInt>>() >= 2,
            "heap pointer must have low bit 0 for tag"
        );
        self.bits_word() & INLINE_TAG != 0
    }

    /// Read the inline value. Only valid when `is_inline()`.
    fn inline_val(&self) -> InlineInt {
        debug_assert!(self.is_inline());
        self.bits_word()
    }

    /// Read the heap pointer. Only valid when `!is_inline()`.
    fn heap_ptr(&self) -> *mut Vec<HeapInt> {
        debug_assert!(!self.is_inline());
        ptr::with_exposed_provenance_mut(self.bits.get())
    }

    fn heap_ref(&self) -> &[HeapInt] {
        let ptr = self.heap_ptr();
        // SAFETY: heap pointers are created by heap_bits from Box::leak and
        // remain owned by self until Drop. &self guarantees no mutable alias.
        unsafe { &*ptr }
    }

    fn heap_mut(&mut self) -> &mut Vec<HeapInt> {
        let ptr = self.heap_ptr();
        // SAFETY: heap pointers are created by heap_bits from Box::leak and
        // remain owned by self until Drop. &mut self guarantees exclusivity.
        unsafe { &mut *ptr }
    }

    fn bits_word(&self) -> usize {
        self.bits.get()
    }

    const fn inline_bits(bits: InlineInt) -> NonZeroUsize {
        debug_assert!(bits & INLINE_TAG != 0);
        // SAFETY: inline representations always have INLINE_TAG set, so the
        // stored word is non-zero. Inline tagged words are never dereferenced.
        unsafe { NonZeroUsize::new_unchecked(bits) }
    }

    fn heap_bits(vec: Vec<HeapInt>) -> NonZeroUsize {
        let ptr = Box::leak(Box::new(vec)) as *mut Vec<HeapInt>;
        let addr = ptr.expose_provenance();
        debug_assert_eq!(addr & INLINE_TAG, 0);
        // SAFETY: Box::leak never returns a null pointer, so its exposed
        // address is non-zero.
        unsafe { NonZeroUsize::new_unchecked(addr) }
    }

    fn set_inline_word(&mut self, word: HeapInt) {
        debug_assert!(self.is_inline());
        self.bits = Self::inline_bits(INLINE_TAG | ((word as InlineInt) << 1));
    }

    /// Get the `idx`-th HeapInt element of the logical bitset.
    fn word(&self, idx: usize) -> HeapInt {
        const _: () = assert!(
            HeapInt::BITS >= InlineInt::BITS - 1,
            "HeapInt must hold all inline data bits"
        );
        if self.is_inline() {
            if idx == 0 {
                (self.inline_val() >> 1) as HeapInt
            } else {
                0
            }
        } else {
            self.heap_ref().get(idx).copied().unwrap_or(0)
        }
    }

    fn word_count(&self) -> usize {
        if self.is_inline() {
            1
        } else {
            self.heap_ref().len()
        }
    }

    fn promote_to_heap(&mut self) {
        debug_assert!(self.is_inline());
        let w0 = (self.inline_val() >> 1) as HeapInt;
        let vec = if w0 == 0 { Vec::new() } else { vec![w0] };
        self.bits = Self::heap_bits(vec);
    }

    fn insert_heap(&mut self, rev: usize) {
        let bits = HeapInt::BITS as usize;
        let (w, b) = (rev / bits, rev % bits);
        let vec = self.heap_mut();
        if w >= vec.len() {
            vec.resize(w + 1, 0);
        }
        vec[w] |= 1 << b;
    }

    fn update_heap_words(
        &mut self,
        other: &Self,
        n: usize,
        f: impl Fn(HeapInt, HeapInt) -> HeapInt,
    ) {
        self.heap_mut()
            .iter_mut()
            .take(n)
            .enumerate()
            .for_each(|(i, word)| *word = f(*word, other.word(i)));
    }
}

impl Default for SmallRevs {
    fn default() -> Self {
        Self::empty()
    }
}

impl Default for &SmallRevs {
    fn default() -> Self {
        SmallRevs::empty_ref()
    }
}

impl Drop for SmallRevs {
    fn drop(&mut self) {
        if !self.is_inline() {
            // SAFETY: heap_ptr() was created from Box::leak in heap_bits and is
            // still uniquely owned by self. Drop runs exactly once.
            unsafe { drop(Box::from_raw(self.heap_ptr())) };
        }
    }
}

impl Clone for SmallRevs {
    fn clone(&self) -> Self {
        if self.is_inline() {
            Self {
                bits: Self::inline_bits(self.inline_val()),
            }
        } else {
            Self {
                bits: Self::heap_bits(self.heap_ref().to_vec()),
            }
        }
    }
}

impl PartialEq for SmallRevs {
    fn eq(&self, other: &Self) -> bool {
        if self.is_inline() && other.is_inline() {
            return self.inline_val() == other.inline_val();
        }
        let n = self.word_count().max(other.word_count());
        (0..n).all(|i| self.word(i) == other.word(i))
    }
}

impl Eq for SmallRevs {}

impl fmt::Debug for SmallRevs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl FromIterator<usize> for SmallRevs {
    fn from_iter<I: IntoIterator<Item = usize>>(iter: I) -> Self {
        let mut revs = SmallRevs::empty();
        for rev in iter {
            revs.insert(rev);
        }
        revs
    }
}

impl From<usize> for SmallRevs {
    fn from(value: usize) -> Self {
        let mut revs = SmallRevs::empty();
        revs.insert(value);
        revs
    }
}

pub struct RevsIter<'a> {
    revs: &'a SmallRevs,
    front_word_idx: usize,
    front_remaining: HeapInt,
    back_word_idx: usize,
    back_remaining: HeapInt,
}

impl Iterator for RevsIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        loop {
            if self.front_remaining != 0 {
                let word_idx = self.front_word_idx;
                let bit = self.front_remaining.trailing_zeros() as usize;
                self.front_remaining &= self.front_remaining - 1;
                if self.front_word_idx == self.back_word_idx {
                    self.back_remaining = self.front_remaining;
                }
                return Some(word_idx * HeapInt::BITS as usize + bit);
            }
            if self.front_word_idx >= self.back_word_idx {
                return None;
            }
            self.front_word_idx += 1;
            self.front_remaining = if self.front_word_idx == self.back_word_idx {
                self.back_remaining
            } else {
                self.revs.word(self.front_word_idx)
            };
        }
    }
}

impl DoubleEndedIterator for RevsIter<'_> {
    fn next_back(&mut self) -> Option<usize> {
        loop {
            if self.back_remaining != 0 {
                let word_idx = self.back_word_idx;
                let bit = HeapInt::BITS as usize - 1 - self.back_remaining.leading_zeros() as usize;
                self.back_remaining &= !((1 as HeapInt) << bit);
                if self.front_word_idx == self.back_word_idx {
                    self.front_remaining = self.back_remaining;
                }
                return Some(word_idx * HeapInt::BITS as usize + bit);
            }
            if self.back_word_idx <= self.front_word_idx {
                return None;
            }
            self.back_word_idx -= 1;
            self.back_remaining = if self.front_word_idx == self.back_word_idx {
                self.front_remaining
            } else {
                self.revs.word(self.back_word_idx)
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use quickcheck::quickcheck;

    use super::*;

    fn check_set_operations(
        left: &[usize],
        right: &[usize],
        is_inline: (Option<bool>, Option<bool>, Option<bool>),
    ) -> bool {
        let left_revs: SmallRevs = left.iter().copied().collect();
        let right_revs: SmallRevs = right.iter().copied().collect();
        let left_set: BTreeSet<usize> = left.iter().copied().collect();
        let right_set: BTreeSet<usize> = right.iter().copied().collect();

        let union_want: SmallRevs = left_set.union(&right_set).copied().collect();
        let difference_want: SmallRevs = left_set.difference(&right_set).copied().collect();
        let intersection_want: SmallRevs = left_set.intersection(&right_set).copied().collect();

        let mut union_got = left_revs.clone();
        union_got.union_with(&right_revs);

        let mut difference_got = left_revs.clone();
        difference_got.difference_with(&right_revs);

        let mut intersection_got = left_revs;
        intersection_got.intersect_with(&right_revs);

        let representation_matches = [
            (union_got.is_inline(), is_inline.0),
            (difference_got.is_inline(), is_inline.1),
            (intersection_got.is_inline(), is_inline.2),
        ]
        .into_iter()
        .all(|(got, want)| want.is_none_or(|want| got == want));

        representation_matches
            && union_got == union_want
            && difference_got == difference_want
            && intersection_got == intersection_want
    }

    #[test]
    fn test_size() {
        assert_eq!(mem::size_of::<SmallRevs>(), mem::size_of::<usize>());
    }

    #[test]
    fn test_option_size() {
        assert_eq!(mem::size_of::<Option<SmallRevs>>(), mem::size_of::<usize>());
    }

    #[test]
    fn test_inline() {
        let mut r = SmallRevs::empty();
        assert!(!r.contains(0));
        assert_eq!(r.len(), 0);
        r.insert(0);
        r.insert(5);
        r.insert(5);
        r.insert(MAX_INLINE_REV);
        assert_eq!(r.len(), 3);
        assert!(r.contains(0));
        assert!(r.contains(5));
        assert!(r.contains(MAX_INLINE_REV));
        assert!(!r.contains(MAX_INLINE_REV + 1));
        assert!(r.is_inline());
    }

    #[test]
    fn test_promote_to_heap() {
        let mut r = SmallRevs::empty();
        r.insert(0);
        r.insert(MAX_INLINE_REV);
        assert!(r.is_inline());

        r.insert(MAX_INLINE_REV + 1);
        assert!(!r.is_inline());
        assert!(r.contains(0));
        assert!(r.contains(MAX_INLINE_REV));
        assert!(r.contains(MAX_INLINE_REV + 1));
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn test_remove_inline() {
        let mut r: SmallRevs = [0, 5, MAX_INLINE_REV].into_iter().collect();
        for _ in 0..=1 {
            r.remove(5);
            r.remove(MAX_INLINE_REV + 1);

            assert_eq!(r.iter().collect::<Vec<_>>(), vec![0, MAX_INLINE_REV]);
            assert!(r.is_inline());
        }
    }

    #[test]
    fn test_remove_heap_does_not_demote() {
        let heap_rev = MAX_INLINE_REV + 1;
        let mut r: SmallRevs = [0, heap_rev].into_iter().collect();
        assert!(!r.is_inline());

        for _ in 0..=1 {
            r.remove(heap_rev);
            assert_eq!(r.iter().collect::<Vec<_>>(), vec![0]);
            assert!(!r.is_inline());
        }

        for _ in 0..=1 {
            r.remove(0);
            assert!(r.is_empty());
            assert!(!r.is_inline());
        }
    }

    #[test]
    fn test_iter_heap() {
        let r: SmallRevs = [0, 63, 64, 127, 128].into_iter().collect();
        let v: Vec<usize> = r.iter().collect();
        assert_eq!(v, vec![0, 63, 64, 127, 128]);
    }

    #[test]
    fn test_iter_double_ended() {
        let r: SmallRevs = [0, 2, 63, 64, 65, 127, 128].into_iter().collect();
        let v: Vec<usize> = r.iter().rev().collect();
        assert_eq!(v, vec![128, 127, 65, 64, 63, 2, 0]);
    }

    #[test]
    fn test_iter_double_ended_mixed_same_word() {
        let r: SmallRevs = [0, 2, 5].into_iter().collect();
        let mut iter = r.iter();
        assert_eq!(iter.next(), Some(0));
        assert_eq!(iter.next_back(), Some(5));
        assert_eq!(iter.next_back(), Some(2));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_iter_double_ended_mixed_across_empty_words() {
        let r: SmallRevs = [0, 130, 255].into_iter().collect();
        let mut iter = r.iter();
        assert_eq!(iter.next_back(), Some(255));
        assert_eq!(iter.next(), Some(0));
        assert_eq!(iter.next_back(), Some(130));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_empty() {
        let check = |r: &SmallRevs| {
            assert!(!r.contains(0));
            assert!(r.is_empty());
            assert_eq!(r.iter().count(), 0);
        };

        check(&SmallRevs::empty());
        check(SmallRevs::empty_ref());
        check(&Default::default());
        check(Default::default());
    }

    #[test]
    fn test_is_empty_after_heap_clear() {
        let rev = MAX_INLINE_REV + 1;
        let mut r: SmallRevs = [rev].into_iter().collect();
        assert!(!r.is_empty());

        r.difference_with(&[rev].into_iter().collect());
        assert!(!r.is_inline());
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert_eq!(r.iter().count(), 0);
    }

    #[test]
    fn test_insert_large_rev_into_empty() {
        let mut r = SmallRevs::empty();
        r.insert(MAX_INLINE_REV + 1);
        assert!(r.contains(MAX_INLINE_REV + 1));
        assert!(!r.contains(0));
        assert_eq!(r.iter().collect::<Vec<_>>(), vec![MAX_INLINE_REV + 1]);
    }

    #[test]
    fn test_set_operations() {
        let heap_rev = MAX_INLINE_REV + 2;
        let later_heap_rev = MAX_INLINE_REV + HeapInt::BITS as usize + 2;

        assert!(check_set_operations(
            &[0, 5],
            &[5, MAX_INLINE_REV],
            (Some(true), Some(true), Some(true)),
        ));
        assert!(check_set_operations(
            &[0, heap_rev, later_heap_rev],
            &[1, heap_rev, later_heap_rev + 1],
            (Some(false), Some(false), Some(false)),
        ));
        assert!(check_set_operations(
            &[0, MAX_INLINE_REV],
            &[1, heap_rev],
            (Some(false), Some(true), Some(true)),
        ));
        assert!(check_set_operations(
            &[1, heap_rev],
            &[0, MAX_INLINE_REV],
            (Some(false), Some(false), Some(false)),
        ));
        assert!(check_set_operations(
            &[0, heap_rev],
            &[0, MAX_INLINE_REV],
            (Some(false), Some(false), Some(false)),
        ));
    }

    // u8 keeps revs in 0..=255: covers inline (0..=62) and heap, avoids large allocs.
    quickcheck! {
        fn check_insert_contains(revs: Vec<u8>) -> bool {
            let mut r = SmallRevs::empty();
            for &v in &revs {
                r.insert(v as usize);
            }
            let set: BTreeSet<usize> = revs.iter().map(|&v| v as usize).collect();
            (0..=255).all(|i| r.contains(i) == set.contains(&i))
        }

        fn check_remove_contains(revs: Vec<u8>, remove: Vec<u8>) -> bool {
            let mut r: SmallRevs = revs.iter().map(|&v| v as usize).collect();
            let mut set: BTreeSet<usize> = revs.iter().map(|&v| v as usize).collect();
            for &v in &remove {
                r.remove(v as usize);
                set.remove(&(v as usize));
            }
            (0..=255).all(|i| r.contains(i) == set.contains(&i))
        }

        fn check_iter_sorted(revs: Vec<u8>) -> bool {
            let r: SmallRevs = revs.iter().map(|&v| v as usize).collect();
            let v: Vec<usize> = r.iter().collect();
            v.windows(2).all(|w| w[0] < w[1])
        }

        fn check_iter_complete(revs: Vec<u8>) -> bool {
            let set: BTreeSet<usize> = revs.iter().map(|&v| v as usize).collect();
            let r: SmallRevs = set.iter().copied().collect();
            let got: Vec<usize> = r.iter().collect();
            let want: Vec<usize> = set.into_iter().collect();
            got == want
        }

        fn check_iter_rev_complete(revs: Vec<u8>) -> bool {
            let set: BTreeSet<usize> = revs.iter().map(|&v| v as usize).collect();
            let r: SmallRevs = set.iter().copied().collect();
            let got: Vec<usize> = r.iter().rev().collect();
            let want: Vec<usize> = set.into_iter().rev().collect();
            got == want
        }

        fn check_len(revs: Vec<u8>) -> bool {
            let set: BTreeSet<usize> = revs.iter().map(|&v| v as usize).collect();
            let r: SmallRevs = revs.iter().map(|&v| v as usize).collect();
            r.len() == set.len()
        }

        fn check_clone_eq(revs: Vec<u8>) -> bool {
            let r: SmallRevs = revs.iter().map(|&v| v as usize).collect();
            r.clone() == r
        }

        fn check_from_range(start: u8, end: u8) -> bool {
            let (s, e) = (start as usize, end as usize);
            let inclusive_got = SmallRevs::from_range(s..=e);
            let inclusive_want: SmallRevs = (s..=e).collect();
            SmallRevs::from_range(s..e) == (s..e).collect::<SmallRevs>()
                && inclusive_got == inclusive_want
        }

        fn check_random_set_operations(left: Vec<u8>, right: Vec<u8>) -> bool {
            let left: Vec<usize> = left.into_iter().map(|v| v as usize).collect();
            let right: Vec<usize> = right.into_iter().map(|v| v as usize).collect();
            check_set_operations(&left, &right, (None, None, None))
        }
    }
}
