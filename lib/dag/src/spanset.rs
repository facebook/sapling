// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # spanset
//!
//! See [`SpanSet`] for the main structure.

use std::cmp::{
    Ordering::{self, Equal, Greater, Less},
    PartialOrd,
};
use std::fmt::{self, Debug};
use std::ops::{Bound, RangeBounds, RangeInclusive};

type Id = u64;

/// Range `low..=high`. `low` must be <= `high`.
#[derive(Copy, Clone, Debug, Eq)]
pub struct Span {
    pub(crate) low: Id,
    pub(crate) high: Id,
}

/// A set of integer spans.
#[derive(Clone)]
pub struct SpanSet {
    spans: Vec<Span>,
}

impl PartialOrd for Span {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reversed order.
        Some(match other.high.cmp(&self.high) {
            Less => Less,
            Greater => Greater,
            Equal => other.low.cmp(&self.low),
        })
    }
}

impl PartialEq for Span {
    fn eq(&self, other: &Self) -> bool {
        other.low == self.low && other.high == self.high
    }
}

impl Ord for Span {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed order.
        match other.high.cmp(&self.high) {
            Less => Less,
            Greater => Greater,
            Equal => other.low.cmp(&self.low),
        }
    }
}

impl Span {
    pub fn new(low: Id, high: Id) -> Self {
        assert!(low <= high);
        Self { low, high }
    }

    fn count(self) -> u64 {
        self.high - self.low + 1
    }

    fn contains(self, value: Id) -> bool {
        self.low <= value && value <= self.high
    }

    fn try_from_bounds(bounds: impl RangeBounds<Id>) -> Option<Self> {
        use Bound::{Excluded, Included};
        #[cfg(debug_assertions)]
        {
            use Bound::Unbounded;
            match (bounds.start_bound(), bounds.end_bound()) {
                (Excluded(_), _) | (Unbounded, _) | (_, Unbounded) => {
                    panic!("unsupported bound type")
                }
                _ => (),
            }
        }
        match (bounds.start_bound(), bounds.end_bound()) {
            (Included(&low), Included(&high)) if low <= high => Some(Span { low, high }),
            (Included(&low), Excluded(&high_plus_one)) if low < high_plus_one => {
                let high = high_plus_one - 1;
                Some(Span { low, high })
            }
            _ => None,
        }
    }
}

// This is for users who want shorter code than [`Span::new`].
// Internal logic here should use [`Span::new`], or [`Span::try_from_bounds`],
// or construct [`Span`] directly.
impl From<RangeInclusive<Id>> for Span {
    fn from(range: RangeInclusive<Id>) -> Span {
        Span::new(*range.start(), *range.end())
    }
}

impl From<Id> for Span {
    fn from(id: Id) -> Span {
        Span::new(id, id)
    }
}

impl<T: Into<Span>> From<T> for SpanSet {
    fn from(span: T) -> SpanSet {
        SpanSet {
            spans: vec![span.into()],
        }
    }
}

impl From<Span> for RangeInclusive<Id> {
    fn from(span: Span) -> RangeInclusive<Id> {
        span.low..=span.high
    }
}

// This is used by `gca(set)` where `set` usually contains 2 ids. The code
// can then be written as `gca((a, b))`.
impl From<(Id, Id)> for SpanSet {
    fn from(ids: (Id, Id)) -> SpanSet {
        SpanSet::from_spans([ids.0, ids.1].iter().cloned())
    }
}

impl SpanSet {
    /// Construct a [`SpanSet`] containing given spans.
    /// Panic if spans overlap.
    pub fn from_spans<T: Into<Span>, I: IntoIterator<Item = T>>(spans: I) -> Self {
        let mut spans: Vec<Span> = spans.into_iter().map(|span| span.into()).collect();
        spans.sort_unstable();
        let result = SpanSet { spans };
        assert!(result.is_valid());
        result
    }

    /// Construct a [`SpanSet`] containing given spans.
    /// The given spans must be already sorted (i.e. larger ids first), and do
    /// not have overlapped spans.
    pub fn from_sorted_spans<T: Into<Span>, I: IntoIterator<Item = T>>(spans: I) -> Self {
        let spans: Vec<Span> = spans.into_iter().map(Into::into).collect();
        let result = SpanSet { spans };
        assert!(result.is_valid());
        result
    }

    /// Construct an empty [`SpanSet`].
    pub fn empty() -> Self {
        let spans = Vec::new();
        SpanSet { spans }
    }

    /// Construct a full [`SpanSet`] that contains everything.
    pub fn full() -> Self {
        let spans = vec![(0..=u64::max_value()).into()];
        SpanSet { spans }
    }

    /// Check if the spans satisfies internal assumptions: sorted and not
    /// overlapped.
    fn is_valid(&self) -> bool {
        self.spans
            .iter()
            .rev()
            .cloned()
            .fold((-1, true), |(last_high, is_sorted), span| {
                (span.high as i64, is_sorted && last_high < span.low as i64)
            })
            .1
    }

    /// Count integers covered by this [`SpanSet`].
    pub fn count(&self) -> u64 {
        self.spans.iter().fold(0, |acc, span| acc + span.count())
    }

    /// Tests if a given [`Id`] or [`Span`] is covered by this set.
    pub fn contains(&self, value: impl Into<Span>) -> bool {
        let mut span = value.into();
        loop {
            let idx = match self
                .spans
                .binary_search_by(|probe| span.low.cmp(&probe.low))
            {
                Ok(idx) => idx,
                Err(idx) => idx,
            };
            if let Some(existing_span) = self.spans.get(idx) {
                debug_assert!(existing_span.low <= span.low);
                if existing_span.high < span.low {
                    return false;
                } else if existing_span.high >= span.high {
                    return true;
                } else {
                    span.low = existing_span.high + 1;
                    debug_assert!(span.low <= span.high);
                }
            } else {
                return false;
            }
        }
    }

    /// Calculates the union of two sets.
    pub fn union(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = Vec::with_capacity((self.spans.len() + rhs.spans.len()).min(32));
        let mut iter_left = self.spans.iter().cloned();
        let mut iter_right = rhs.spans.iter().cloned();
        let mut next_left = iter_left.next();
        let mut next_right = iter_right.next();
        let mut push = |span: Span| push_with_union(&mut spans, span);

        loop {
            match (next_left, next_right) {
                (Some(left), Some(right)) => {
                    if left.high < right.high {
                        push(right);
                        next_right = iter_right.next();
                    } else {
                        push(left);
                        next_left = iter_left.next();
                    }
                }
                (Some(span), None) => {
                    push(span);
                    next_left = iter_left.next();
                }
                (None, Some(span)) => {
                    push(span);
                    next_right = iter_right.next();
                }
                (None, None) => {
                    let result = SpanSet { spans };
                    debug_assert!(result.is_valid());
                    return result;
                }
            }
        }
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = Vec::with_capacity(self.spans.len().max(rhs.spans.len()).min(32));
        let mut iter_left = self.spans.iter().cloned();
        let mut iter_right = rhs.spans.iter().cloned();
        let mut next_left = iter_left.next();
        let mut next_right = iter_right.next();
        let mut push = |span: Span| push_with_union(&mut spans, span);

        loop {
            match (next_left, next_right) {
                (Some(left), Some(right)) => {
                    // current:
                    //   |------- A --------|
                    //         |------- B ------|
                    //         |--- span ---|
                    // next:
                    //   |- A -| (remaining part of A)
                    //           (next B)
                    // note: (A, B) can be either (left, right) or (right, left)
                    let span_low = left.low.max(right.low);
                    let span_high = left.high.min(right.high);
                    if let Some(span) = Span::try_from_bounds(span_low..=span_high) {
                        push(span);
                    }

                    next_right = Span::try_from_bounds(right.low..(right.high + 1).min(span_low))
                        .or_else(|| iter_right.next());
                    next_left = Span::try_from_bounds(left.low..(left.high + 1).min(span_low))
                        .or_else(|| iter_left.next());
                }
                (_, None) | (None, _) => {
                    let result = SpanSet { spans };
                    debug_assert!(result.is_valid());
                    return result;
                }
            }
        }
    }

    /// Calculates spans that are included only by this set, not `rhs`.
    pub fn difference(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = Vec::with_capacity(self.spans.len().max(rhs.spans.len()).min(32));
        let mut iter_left = self.spans.iter().cloned();
        let mut iter_right = rhs.spans.iter().cloned();
        let mut next_left = iter_left.next();
        let mut next_right = iter_right.next();
        let mut push = |span: Span| push_with_union(&mut spans, span);

        loop {
            match (next_left, next_right) {
                (Some(left), Some(right)) => {
                    if right.low > left.high {
                        next_right = iter_right.next();
                    } else {
                        next_left = if right.high < left.low {
                            push(left);
                            iter_left.next()
                        } else {
                            // |----------------- left ------------------|
                            // |--- span1 ---|--- right ---|--- span2 ---|
                            if let Some(span2) = Span::try_from_bounds(right.high + 1..=left.high) {
                                push(span2);
                            }

                            Span::try_from_bounds(left.low..right.low).or_else(|| iter_left.next())
                        };
                    }
                }
                (Some(left), None) => {
                    push(left);
                    next_left = iter_left.next();
                }
                (None, _) => {
                    let result = SpanSet { spans };
                    debug_assert!(result.is_valid());
                    return result;
                }
            }
        }
    }

    /// Get an iterator for integers in this [`SpanSet`].
    /// By default, the iteration is in descending order.
    pub fn iter(&self) -> SpanSetIter {
        SpanSetIter {
            span_set: self,
            front: (0, 0),
            back: (
                self.spans.len() as isize - 1,
                self.spans
                    .last()
                    .map(|span| span.high - span.low)
                    .unwrap_or(0),
            ),
        }
    }

    /// Internal use only. Append a span, which must have lower boundaries
    /// than existing spans.
    pub(crate) fn push_span(&mut self, span: Span) {
        push_with_union(&mut self.spans, span);
    }

    /// Internal use only. Append a [`SpanSet`], which must have lower
    /// boundaries than the existing spans.
    ///
    /// This is faster than [`SpanSet::union`]. used when it's known
    /// that the all ids in `set` being added is below the minimal id
    /// in the `self` set.
    pub(crate) fn push_set(&mut self, set: &SpanSet) {
        for span in &set.spans {
            self.push_span(*span);
        }
    }

    /// Internal use only. Iterate through the spans.
    pub(crate) fn as_spans(&self) -> &Vec<Span> {
        &self.spans
    }
}

/// Push a span to `Vec<Span>`. Try to union them in-place.
fn push_with_union(spans: &mut Vec<Span>, span: Span) {
    match spans.last_mut() {
        None => spans.push(span),
        Some(mut last) => {
            debug_assert!(last.high >= span.high);
            if last.low <= span.high + 1 {
                // Union spans in-place.
                last.low = last.low.min(span.low);
            } else {
                spans.push(span)
            }
        }
    }
}

impl Debug for SpanSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let ranges: Vec<String> = self
            .spans
            .iter()
            .rev()
            .flat_map(|s| {
                if s.low + 2 >= s.high {
                    // "low..=high" form is not shorter.
                    (s.low..=s.high).map(|i| format!("{}", i)).collect()
                } else {
                    vec![format!("{}..={}", s.low, s.high)]
                }
            })
            .collect();
        write!(f, "{}", ranges.join(" "))
    }
}

/// Iterator of integers in a [`SpanSet`].
pub struct SpanSetIter<'a> {
    span_set: &'a SpanSet,
    // (index of span_set.spans, index of span_set.spans[i])
    front: (isize, Id),
    back: (isize, Id),
}

impl<'a> Iterator for SpanSetIter<'a> {
    type Item = Id;

    fn next(&mut self) -> Option<Id> {
        if self.front > self.back {
            None
        } else {
            let (vec_id, span_id) = self.front;
            let span = &self.span_set.spans[vec_id as usize];
            self.front = if span_id == span.high - span.low {
                (vec_id + 1, 0)
            } else {
                (vec_id, span_id + 1)
            };
            Some(span.high - span_id)
        }
    }
}

impl<'a> DoubleEndedIterator for SpanSetIter<'a> {
    fn next_back(&mut self) -> Option<Id> {
        if self.front > self.back {
            None
        } else {
            let (vec_id, span_id) = self.back;
            let span = &self.span_set.spans[vec_id as usize];
            self.back = if span_id == 0 {
                let span_len = if vec_id > 0 {
                    let span = self.span_set.spans[(vec_id - 1) as usize];
                    span.high - span.low
                } else {
                    0
                };
                (vec_id - 1, span_len)
            } else {
                (vec_id, span_id - 1)
            };
            Some(span.high - span_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_overlapped_spans() {
        SpanSet::from_spans(vec![1..=3, 3..=4]);
    }

    #[test]
    fn test_valid_spans() {
        SpanSet::empty();
        SpanSet::from_spans(vec![4..=4, 3..=3, 1..=2]);
    }

    #[test]
    fn test_count() {
        let set = SpanSet::empty();
        assert_eq!(set.count(), 0);

        let set = SpanSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        assert_eq!(set.count(), 10 + 1 + 10);
    }

    #[test]
    fn test_contains() {
        let set = SpanSet::empty();
        assert!(!set.contains(0));
        assert!(!set.contains(10));

        let set = SpanSet::from_spans(vec![1..=1, 2..=9, 10..=10, 20..=20, 31..=35, 36..=40]);
        assert!(!set.contains(0));
        assert!(set.contains(1));
        assert!(set.contains(5));
        assert!(set.contains(10));
        assert!(!set.contains(11));

        assert!(set.contains(1..=10));
        assert!(set.contains(1..=8));
        assert!(set.contains(3..=10));
        assert!(set.contains(3..=7));
        assert!(!set.contains(1..=11));
        assert!(!set.contains(0..=10));

        assert!(!set.contains(19));
        assert!(!set.contains(19..=20));
        assert!(set.contains(20));
        assert!(!set.contains(20..=21));
        assert!(!set.contains(21));

        assert!(!set.contains(30));
        assert!(set.contains(31));
        assert!(set.contains(32));
        assert!(set.contains(39));
        assert!(set.contains(40));
        assert!(!set.contains(41));

        assert!(set.contains(31..=40));
        assert!(set.contains(32..=40));
        assert!(set.contains(31..=39));
        assert!(set.contains(31..=39));
        assert!(!set.contains(31..=41));
        assert!(!set.contains(30..=40));
        assert!(!set.contains(30..=41));
    }

    fn union(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<Id>> {
        let a = SpanSet::from_spans(a);
        let b = SpanSet::from_spans(b);
        let spans1 = a.union(&b).spans;
        let spans2 = b.union(&a).spans;
        assert_eq!(spans1, spans2);
        spans1.into_iter().map(|span| span.into()).collect()
    }

    #[test]
    fn test_union() {
        assert_eq!(union(vec![1..=10], vec![10..=20]), vec![1..=20]);
        assert_eq!(union(vec![1..=30], vec![10..=20]), vec![1..=30]);
        assert_eq!(union(vec![6, 8, 10], vec![5, 7, 9]), vec![5..=10]);
        assert_eq!(
            union(vec![6..=6, 8..=9, 10..=10], vec![5]),
            vec![8..=10, 5..=6]
        );
    }

    fn intersect(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<Id>> {
        let a = SpanSet::from_spans(a);
        let b = SpanSet::from_spans(b);
        let spans1 = a.intersection(&b).spans;
        let spans2 = b.intersection(&a).spans;
        assert_eq!(spans1, spans2);
        spans1.into_iter().map(|span| span.into()).collect()
    }

    #[test]
    fn test_intersection() {
        assert_eq!(intersect(vec![1..=10], vec![11..=20]), vec![]);
        assert_eq!(intersect(vec![1..=10], vec![10..=20]), vec![10..=10]);
        assert_eq!(intersect(vec![1..=30], vec![10..=20]), vec![10..=20]);
        assert_eq!(
            intersect(vec![0..=10, 15..=20], vec![0..=30]),
            vec![15..=20, 0..=10]
        );
        assert_eq!(
            intersect(vec![0..=10, 15..=20], vec![5..=19]),
            vec![15..=19, 5..=10]
        );
        assert_eq!(intersect(vec![10, 9, 8, 7], vec![8..=11]), vec![8..=10]);
        assert_eq!(intersect(vec![10, 9, 8, 7], vec![5..=8]), vec![7..=8]);
    }

    fn difference(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<Id>> {
        let a = SpanSet::from_spans(a);
        let b = SpanSet::from_spans(b);
        let spans1 = a.difference(&b).spans;
        let spans2 = b.difference(&a).spans;

        // |------------- a -------------------|
        // |--- spans1 ---|--- intersection ---|--- spans2 ---|
        //                |------------------- b -------------|
        let intersected = intersect(a.spans.clone(), b.spans.clone());
        let unioned = union(a.spans.clone(), b.spans.clone());
        assert_eq!(
            union(intersected.clone(), spans1.clone()),
            union(a.spans.clone(), Vec::<Span>::new())
        );
        assert_eq!(
            union(intersected.clone(), spans2.clone()),
            union(b.spans.clone(), Vec::<Span>::new())
        );
        assert_eq!(
            union(spans1.clone(), union(intersected.clone(), spans2.clone())),
            unioned.clone(),
        );

        assert!(intersect(spans1.clone(), spans2.clone()).is_empty());
        assert!(intersect(spans1.clone(), intersected.clone()).is_empty());
        assert!(intersect(spans2.clone(), intersected.clone()).is_empty());

        spans1.into_iter().map(|span| span.into()).collect()
    }

    #[test]
    fn test_difference() {
        assert_eq!(difference(vec![0..=5], Vec::<Span>::new()), vec![0..=5]);
        assert_eq!(difference(Vec::<Span>::new(), vec![0..=5]), vec![]);
        assert_eq!(difference(vec![0..=0], vec![1..=1]), vec![0..=0]);
        assert_eq!(difference(vec![0..=0], vec![0..=1]), vec![]);
        assert_eq!(difference(vec![0..=10], vec![0..=5]), vec![6..=10]);

        assert_eq!(
            difference(vec![0..=10], vec![3..=4, 7..=8]),
            vec![9..=10, 5..=6, 0..=2]
        );
        assert_eq!(
            difference(vec![3..=4, 7..=8, 10..=12], vec![4..=11]),
            vec![12..=12, 3..=3]
        );
    }

    #[test]
    fn test_iter() {
        let set = SpanSet::empty();
        assert!(set.iter().next().is_none());
        assert!(set.iter().rev().next().is_none());

        let set = SpanSet::from(0..=1);
        assert_eq!(set.iter().collect::<Vec<Id>>(), vec![1, 0]);
        assert_eq!(set.iter().rev().collect::<Vec<Id>>(), vec![0, 1]);

        let mut iter = set.iter();
        assert!(iter.next().is_some());
        assert!(iter.next_back().is_some());
        assert!(iter.next_back().is_none());

        let set = SpanSet::from_spans(vec![3..=5, 7..=8]);
        assert_eq!(set.iter().collect::<Vec<Id>>(), vec![8, 7, 5, 4, 3]);
        assert_eq!(set.iter().rev().collect::<Vec<Id>>(), vec![3, 4, 5, 7, 8]);
    }
}
