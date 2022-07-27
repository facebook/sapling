/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # spanset
//!
//! See [`SpanSet`] for the main structure.

use std::cmp::Ordering;
use std::cmp::Ordering::Equal;
use std::cmp::Ordering::Greater;
use std::cmp::Ordering::Less;
use std::collections::BinaryHeap;
use std::collections::VecDeque;
use std::fmt;
use std::fmt::Debug;
use std::iter::Rev;
use std::ops::Bound;
use std::ops::RangeBounds;
use std::ops::RangeInclusive;

use dag_types::FlatSegment;
use serde::Deserialize;
use serde::Serialize;

use crate::bsearch::BinarySearchBy;
use crate::id::Id;

/// Range `low..=high`. `low` must be <= `high`.
#[derive(Copy, Clone, Debug, Eq, Serialize, Deserialize)]
pub struct Span {
    #[serde(with = "flat_id")]
    pub(crate) low: Id,
    #[serde(with = "flat_id")]
    pub(crate) high: Id,
}

/// A set of integer spans.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct SpanSet {
    /// `spans` are sorted in DESC order.
    spans: VecDeque<Span>,
}

impl PartialOrd for Span {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(match self.high.cmp(&other.high) {
            Less => Less,
            Greater => Greater,
            Equal => self.low.cmp(&other.low),
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
        match self.high.cmp(&other.high) {
            Less => Less,
            Greater => Greater,
            Equal => self.low.cmp(&other.low),
        }
    }
}

impl Span {
    pub fn new(low: Id, high: Id) -> Self {
        assert!(low <= high, "low {:?} <= high {:?}", low, high);
        Self { low, high }
    }

    pub fn count(self) -> u64 {
        self.high.0 - self.low.0 + 1
    }

    /// Get the n-th [`Id`] in this [`Span`].
    ///
    /// Similar to [`SpanSet`], ids are sorted in descending order.
    /// The 0-th Id is `high`.
    pub fn nth(self, n: u64) -> Option<Id> {
        if n >= self.count() {
            None
        } else {
            Some(self.high - n)
        }
    }

    pub(crate) fn contains(self, value: Id) -> bool {
        self.low <= value && value <= self.high
    }

    /// Construct a full [`Span`] that contains everything.
    /// Warning: The [`Id`] in this span might be unknown to an actual storage.
    pub fn full() -> Self {
        (Id::MIN..=Id::MAX).into()
    }

    pub(crate) fn try_from_bounds(bounds: impl RangeBounds<Id>) -> Option<Self> {
        use Bound::Excluded;
        use Bound::Included;
        #[cfg(debug_assertions)]
        {
            use Bound::Unbounded;
            match (bounds.start_bound(), bounds.end_bound()) {
                (Excluded(_), _) | (Unbounded, _) | (_, Unbounded) => {
                    panic!("unsupported bound type")
                }
                _ => {}
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

/// Subspan is a trait for an object that
/// (a) Can be mapped into Span
/// (b) Can return 'subspan' for any given non-empty subset of it's span
pub trait Subspan {
    fn span(&self) -> Span;

    /// Provided span should be subset of T::span(), otherwise this method behavior is undefined
    fn subspan(&self, span: Span) -> Self;

    /// Overlaps two objects and returns result of overlap and remainders for left and right object
    /// This method is generally defined for any two types that implement Subspan
    /// Type of an overlap if the same as type of Self
    ///
    /// Returns:
    ///  - overlap: overlap between two objects
    ///  - rem_left: remaining non-overlapping part of left object
    ///  - rem_right: remaining non-overlapping part of right object
    ///
    /// L: [123456]
    /// R:    [456789]
    /// overlap(L, R) = ([456], [123], None)
    fn intersect<R: Subspan>(&self, r: &R) -> (Option<Self>, Option<Self>, Option<R>)
    where
        Self: Sized,
    {
        let left = self.span();
        let right = r.span();
        let span_low = left.low.max(right.low);
        let span_high = left.high.min(right.high);
        let overlap = Span::try_from_bounds(span_low..=span_high);

        let rem_left = Span::try_from_bounds(left.low..(left.high + 1).min(span_low));
        let rem_right = Span::try_from_bounds(right.low..(right.high + 1).min(span_low));

        let overlap = overlap.map(|s| self.subspan(s));
        let rem_left = rem_left.map(|s| self.subspan(s));
        let rem_right = rem_right.map(|s| r.subspan(s));
        (overlap, rem_left, rem_right)
    }
}

/// Calculates the intersection of two ordered iterators of span-like objects.
pub fn intersect_iter<
    L: Subspan,
    R: Subspan,
    LI: Iterator<Item = L>,
    RI: Iterator<Item = R>,
    P: FnMut(L),
>(
    mut lhs: LI,
    mut rhs: RI,
    mut push: P,
) {
    let mut next_left = lhs.next();
    let mut next_right = rhs.next();

    while let (Some(left), Some(right)) = (next_left, next_right) {
        // current:
        //   |------- A --------|
        //         |------- B ------|
        //         |--- span ---|
        // next:
        //   |- A -| (remaining part of A)
        //           (next B)
        // note: (A, B) can be either (left, right) or (right, left)
        let (span, rem_left, rem_right) = left.intersect(&right);

        if let Some(span) = span {
            push(span);
        }

        next_right = rem_right.or_else(|| rhs.next());
        next_left = rem_left.or_else(|| lhs.next());
    }
}

impl Subspan for Span {
    fn span(&self) -> Span {
        *self
    }

    fn subspan(&self, span: Span) -> Self {
        assert!(self.low <= span.low);
        assert!(self.high >= span.high);
        span
    }
}

impl Subspan for FlatSegment {
    fn span(&self) -> Span {
        Span::new(self.low, self.high)
    }

    fn subspan(&self, span: Span) -> Self {
        assert!(self.low <= span.low);
        assert!(self.high >= span.high);
        if span.low == self.low {
            FlatSegment {
                low: span.low,
                high: span.high,
                parents: self.parents.clone(),
            }
        } else {
            FlatSegment {
                low: span.low,
                high: span.high,
                parents: vec![span.low - 1],
            }
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
        SpanSet::from_sorted_spans(std::iter::once(span.into()))
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
    /// Overlapped or adjacent spans will be merged automatically.
    pub fn from_spans<T: Into<Span>, I: IntoIterator<Item = T>>(spans: I) -> Self {
        let mut heap: BinaryHeap<Span> = spans.into_iter().map(|span| span.into()).collect();
        let mut spans = VecDeque::with_capacity(heap.len().min(64));
        while let Some(span) = heap.pop() {
            push_with_union(&mut spans, span);
        }
        let result = SpanSet { spans };
        // `result` should be valid because the use of `push_with_union`.
        #[cfg(debug_assertions)]
        result.validate();
        result
    }

    /// Construct a [`SpanSet`] containing given spans.
    /// The given spans must be already sorted (i.e. larger ids first), and do
    /// not have overlapped spans.
    /// Adjacent spans will be merged automatically.
    pub fn from_sorted_spans<T: Into<Span>, I: IntoIterator<Item = T>>(span_iter: I) -> Self {
        let mut spans = VecDeque::<Span>::new();
        for span in span_iter {
            let span = span.into();
            push_with_union(&mut spans, span);
        }
        let result = Self { spans };
        #[cfg(debug_assertions)]
        result.validate();
        result
    }

    /// Construct an empty [`SpanSet`].
    pub fn empty() -> Self {
        let spans = VecDeque::new();
        SpanSet { spans }
    }

    /// Construct a full [`SpanSet`] that contains everything.
    /// Warning: The [`Id`] in this set might be unknown to an actual storage.
    pub fn full() -> Self {
        Span::full().into()
    }

    /// Check if this [`SpanSet`] contains nothing.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Validate the spans are in the expected order and there are no mergable
    /// adjacent spans.
    #[cfg(debug_assertions)]
    fn validate(&self) {
        for (i, span) in self.spans.iter().enumerate() {
            assert!(span.low <= span.high);
            if i > 0 {
                assert!(
                    span.high + 1 < self.spans[i - 1].low,
                    "{:?} is not in DESC order or has mergable adjacent spans (around #{})",
                    &self.spans,
                    i
                );
            }
        }
    }

    /// Count integers covered by this [`SpanSet`].
    pub fn count(&self) -> u64 {
        self.spans.iter().fold(0, |acc, span| acc + span.count())
    }

    /// Tests if a given [`Id`] or [`Span`] is covered by this set.
    pub fn contains(&self, value: impl Into<Span>) -> bool {
        self.span_contains(value).is_some()
    }

    /// Find the [`Span`] that covers the given `value`.
    pub fn span_contains(&self, value: impl Into<Span>) -> Option<&Span> {
        let span = value.into();
        let idx = match self.spans.bsearch_by(|probe| span.low.cmp(&probe.low)) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };
        if let Some(existing_span) = self.spans.get(idx) {
            debug_assert!(existing_span.low <= span.low);
            if existing_span.high >= span.high {
                return Some(existing_span);
            }
        }
        None
    }

    /// Skip the first `n` items.
    pub fn skip(&self, mut n: u64) -> Self {
        #[cfg(test)]
        let expected = n.max(self.count()) - n;
        let mut result = SpanSet::empty();
        for span in self.as_spans() {
            if n == 0 {
                result.push_span(*span);
            } else {
                let count = span.count();
                if count <= n {
                    // This span is skipped entirely.
                    n -= count;
                } else {
                    // This span is skipped partially.
                    debug_assert!(n > 0);
                    debug_assert!(n < count);
                    let high = span.high - n;
                    n = 0;
                    result.push_span((span.low..=high).into());
                }
            }
        }
        #[cfg(test)]
        assert_eq!(result.count(), expected);
        result
    }

    /// Only take the first `n` items.
    pub fn take(&self, mut n: u64) -> Self {
        #[cfg(test)]
        let expected = n.min(self.count());
        let mut result = SpanSet::empty();
        for span in self.as_spans() {
            if n == 0 {
                break;
            } else {
                let count = span.count();
                if count <= n {
                    // This span is taken entirely.
                    n -= count;
                    result.push_span(*span);
                } else {
                    // Part of the span is the last to be taken.
                    debug_assert!(n > 0);
                    debug_assert!(n < count);
                    let low = span.high - (n - 1);
                    n = 0;
                    result.push_span((low..=span.high).into());
                }
            }
        }
        #[cfg(test)]
        assert_eq!(result.count(), expected);
        result
    }

    /// Calculates the union of two sets.
    pub fn union(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = VecDeque::with_capacity((self.spans.len() + rhs.spans.len()).min(32));
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
                    #[cfg(debug_assertions)]
                    result.validate();
                    return result;
                }
            }
        }
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = VecDeque::with_capacity(self.spans.len().max(rhs.spans.len()).min(32));
        let push = |span: Span| push_with_union(&mut spans, span);
        intersect_iter(self.spans.iter().cloned(), rhs.spans.iter().cloned(), push);

        let result = SpanSet { spans };
        #[cfg(debug_assertions)]
        result.validate();
        result
    }

    /// Calculates spans that are included only by this set, not `rhs`.
    pub fn difference(&self, rhs: &SpanSet) -> SpanSet {
        let mut spans = VecDeque::with_capacity(self.spans.len().max(rhs.spans.len()).min(32));
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
                    #[cfg(debug_assertions)]
                    result.validate();
                    return result;
                }
            }
        }
    }

    /// Iterate `Id`s in descending order.
    pub fn iter_desc(&self) -> SpanSetIter<&SpanSet> {
        let len = self.spans.len();
        let back = (
            len as isize - 1,
            if len == 0 {
                0
            } else {
                let span = self.spans[len - 1];
                span.high.0 - span.low.0
            },
        );
        SpanSetIter {
            span_set: self,
            front: (0, 0),
            back,
        }
    }

    /// Iterate `Id`s in ascending order.
    pub fn iter_asc(&self) -> Rev<SpanSetIter<&Self>> {
        self.iter_desc().rev()
    }

    /// Iterate `Span`s in descending order.
    pub fn iter_span_desc(&self) -> impl Iterator<Item = &Span> {
        self.as_spans().iter()
    }

    /// Iterate `Span`s in ascending order.
    pub fn iter_span_asc(&self) -> impl Iterator<Item = &Span> {
        self.as_spans().iter().rev()
    }

    /// Get the maximum id in this set.
    pub fn max(&self) -> Option<Id> {
        self.spans.get(0).map(|span| span.high)
    }

    /// Get the minimal id in this set.
    pub fn min(&self) -> Option<Id> {
        self.spans
            .get(self.spans.len().max(1) - 1)
            .map(|span| span.low)
    }

    /// Internal use only. Append a span, which must have lower boundaries
    /// than existing spans.
    pub(crate) fn push_span(&mut self, span: Span) {
        push_with_union(&mut self.spans, span);
    }

    /// Internal use only. Append a span, which must have high boundaries
    /// than existing spans. In other words, spans passed to this function
    /// should be in ascending order.
    pub(crate) fn push_span_asc(&mut self, span: Span) {
        if self.spans.is_empty() {
            self.spans.push_back(span);
        } else {
            let mut last = &mut self.spans[0];
            // | last |
            //     | span |  | span |
            debug_assert!(span.low >= last.low);
            if last.high + 1 >= span.low {
                // Update in-place.
                last.high = span.high.max(last.high);
            } else {
                self.spans.push_front(span);
            }
        }
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

    /// Get a reference to the spans.
    pub fn as_spans(&self) -> &VecDeque<Span> {
        &self.spans
    }

    /// Make this [`SpanSet`] contain the specified `span`.
    ///
    /// The current implementation works best when spans are pushed in
    /// ascending or descending order.
    pub fn push(&mut self, span: impl Into<Span>) {
        let span = span.into();
        if self.spans.is_empty() {
            self.spans.push_back(span)
        } else {
            let len = self.spans.len();
            {
                // Fast path: pushing to the last span.
                // 30->22 20->12 last H->L
                //               span H------>L union [Case 1]
                //                         H->L new   [Case 2]
                let mut last = &mut self.spans[len - 1];
                if last.high >= span.high {
                    if last.low <= span.high + 1 {
                        // Union spans in-place [Case 1]
                        last.low = last.low.min(span.low);
                    } else {
                        // New back span [Case 2]
                        self.spans.push_back(span)
                    }
                    return;
                }
            }
            {
                // Fast path: pushing to the last span.
                // first      H->L  20->12 10->12
                // span  H------>L union [Case 3]
                //       H->L      new   [Case 4]
                // Fast path: pushing to the first span.
                let mut first = &mut self.spans[0];
                if span.low >= first.low {
                    if span.low <= first.high + 1 {
                        // Union [Case 3]
                        first.high = first.high.max(span.high);
                    } else {
                        // New front span [Case 4]
                        self.spans.push_front(span);
                    }
                    return;
                }
            }
            {
                // Fast path: modify a span in-place.
                // higher H1---->L1     cur H2---->L2     lower H3---->L3
                // safe range        L1-2---------------->H3+2
                // Exceeding the safe range would cause spans to overlap and this path cannot
                // handle that.
                let idx = match self
                    .spans
                    .bsearch_by(|probe| (span.high + 1).cmp(&probe.low))
                {
                    Ok(idx) => idx,
                    Err(idx) => idx,
                };
                for idx in [idx] {
                    if let Some(cur) = self.spans.get(idx) {
                        // Not overlap with span?
                        if span.high + 1 < cur.low || cur.high + 1 < span.low {
                            continue;
                        }
                        // Might merge with a higher span? (Not in safe range)
                        if idx > 0 {
                            if let Some(higher) = self.spans.get(idx - 1) {
                                if span.high + 1 >= higher.low {
                                    continue;
                                }
                            }
                        }
                        // Might merge with a lower span? (Not in safe range)
                        if let Some(lower) = self.spans.get(idx + 1) {
                            if lower.high + 1 >= span.low {
                                continue;
                            }
                        }
                        // Passed all checks. Merge the span.
                        let mut cur = &mut self.spans[idx];
                        cur.high = cur.high.max(span.high);
                        cur.low = cur.low.min(span.low);
                        return;
                    }
                }
            }
            {
                // PERF: There might be a better way to do this by bisecting
                // spans and insert or delete in-place.  For now, this code
                // path remains not optimized since it is rarely used.
                *self = self.union(&SpanSet::from(span))
            }
        }
    }

    /// Intersection with a span. Return the min Id.
    ///
    /// This is not a general purpose API, but useful for internal logic
    /// like DAG descendant calculation.
    pub(crate) fn intersection_span_min(&self, rhs: Span) -> Option<Id> {
        let i = match self.spans.bsearch_by(|probe| rhs.low.cmp(&probe.high)) {
            Ok(idx) => idx,
            Err(idx) => idx.max(1) - 1,
        };
        // Prefer small index so we get the span that might overlap:
        // |----spans[1]----|      |----spans[0]----|
        //                     |----rhs-----|
        //                     (want spans[0], not spans[1])
        if i < self.spans.len() {
            let lhs = self.spans[i];
            if lhs.low <= rhs.high && lhs.high >= rhs.low {
                Some(lhs.low.max(rhs.low))
            } else {
                None
            }
        } else {
            // Happens if the set is empty.
            None
        }
    }
}

/// Push a span to `VecDeque<Span>`. Try to union them in-place.
fn push_with_union(spans: &mut VecDeque<Span>, span: Span) {
    if spans.is_empty() {
        spans.push_back(span);
    } else {
        let len = spans.len();
        let mut last = &mut spans[len - 1];
        debug_assert!(last.high >= span.high);
        if last.low <= span.high + 1 {
            // Union spans in-place.
            last.low = last.low.min(span.low);
        } else {
            spans.push_back(span)
        }
    }
}

impl Debug for SpanSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        // Limit spans to show.
        let limit = f.width().unwrap_or(12);
        let mut ranges: Vec<String> = self
            .spans
            .iter()
            .rev()
            .take(limit)
            .flat_map(|s| {
                if s.low + 2 >= s.high {
                    // "low..=high" form is not shorter.
                    (s.low.to(s.high)).map(|i| format!("{}", i)).collect()
                } else {
                    vec![format!("{}..={}", s.low, s.high)]
                }
            })
            .collect();
        let total = self.spans.len();
        if total == limit + 1 {
            ranges.push("and 1 span".into());
        } else if total > limit {
            ranges.push(format!("and {} spans", total - limit));
        }
        write!(f, "{}", ranges.join(" "))
    }
}

/// Iterator of integers in a [`SpanSet`].
#[derive(Clone)]
pub struct SpanSetIter<T> {
    span_set: T,
    // (index of span_set.spans, index of span_set.spans[i])
    front: (isize, u64),
    back: (isize, u64),
}

impl<T: AsRef<SpanSet>> SpanSetIter<T> {
    fn count_remaining(&self) -> u64 {
        let mut front = self.front;
        let back = self.back;
        let mut count = 0;
        while front <= back {
            let (vec_id, span_id) = front;
            let (back_vec_id, back_span_id) = back;
            if vec_id < back_vec_id {
                let len = self.span_set.as_ref().spans[vec_id as usize].count();
                count += len.max(span_id) - span_id;
                front = (vec_id + 1, 0);
            } else {
                count += back_span_id - span_id + 1;
                front = (vec_id + 1, 0);
            }
        }
        count
    }
}

impl<T: AsRef<SpanSet>> Iterator for SpanSetIter<T> {
    type Item = Id;

    fn next(&mut self) -> Option<Id> {
        if self.front > self.back {
            #[cfg(test)]
            assert_eq!(self.size_hint().0, 0);
            None
        } else {
            #[cfg(test)]
            let old_size = self.size_hint().0;
            let (vec_id, span_id) = self.front;
            let span = &self.span_set.as_ref().spans[vec_id as usize];
            self.front = if span_id == span.high.0 - span.low.0 {
                (vec_id + 1, 0)
            } else {
                (vec_id, span_id + 1)
            };
            #[cfg(test)]
            assert_eq!(self.size_hint().0 + 1, old_size);
            Some(span.high - span_id)
        }
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        #[cfg(test)]
        let expected_size = self.size_hint().0.max(n + 1) - n - 1;
        let mut n = n as u64;
        while self.front <= self.back {
            let (vec_id, span_id) = self.front;
            let span = &self.span_set.as_ref().spans[vec_id as usize];
            let span_remaining = span.count() - span_id;
            if n >= span_remaining {
                n -= span_remaining;
                self.front = (vec_id + 1, 0)
            } else {
                let span_id = span_id + n;
                self.front = (vec_id, span_id);
                let result = if self.front <= self.back {
                    self.front.1 += 1;
                    Some(span.high - span_id)
                } else {
                    None
                };
                #[cfg(test)]
                assert_eq!(self.size_hint().0, expected_size);
                return result;
            };
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.count_remaining() as _;
        (size, Some(size))
    }

    fn count(self) -> usize {
        self.count_remaining() as _
    }

    fn last(mut self) -> Option<Self::Item> {
        self.next_back()
    }
}

impl<T: AsRef<SpanSet>> DoubleEndedIterator for SpanSetIter<T> {
    fn next_back(&mut self) -> Option<Id> {
        if self.front > self.back {
            #[cfg(test)]
            assert_eq!(self.size_hint().0, 0);
            None
        } else {
            #[cfg(test)]
            let old_size = self.size_hint().0;
            let (vec_id, span_id) = self.back;
            let span = &self.span_set.as_ref().spans[vec_id as usize];
            self.back = if span_id == 0 {
                let span_len = if vec_id > 0 {
                    let span = self.span_set.as_ref().spans[(vec_id - 1) as usize];
                    span.high.0 - span.low.0
                } else {
                    0
                };
                (vec_id - 1, span_len)
            } else {
                (vec_id, span_id - 1)
            };
            #[cfg(test)]
            assert_eq!(self.size_hint().0 + 1, old_size);
            Some(span.high - span_id)
        }
    }

    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        #[cfg(test)]
        let expected_size = self.size_hint().0.max(n + 1) - n - 1;
        let mut n = n as u64;
        while self.front <= self.back {
            let (vec_id, span_id) = self.back;
            let span = &self.span_set.as_ref().spans[vec_id as usize];
            let span_remaining = span_id + 1;
            if n >= span_remaining {
                n -= span_remaining;
                let span_end = if vec_id > 0 {
                    self.span_set.as_ref().spans[(vec_id - 1) as usize].count() - 1
                } else {
                    0
                };
                self.back = (vec_id - 1, span_end);
            } else {
                let span_id = span_id - n;
                self.back = (vec_id, span_id);
                let result = if self.front <= self.back {
                    if span_id == 0 {
                        let span_end = if vec_id > 0 {
                            self.span_set.as_ref().spans[(vec_id - 1) as usize].count() - 1
                        } else {
                            0
                        };
                        self.back = (vec_id - 1, span_end);
                    } else {
                        self.back.1 -= 1;
                    }
                    Some(span.high - span_id)
                } else {
                    None
                };
                #[cfg(test)]
                assert_eq!(self.size_hint().0, expected_size);
                return result;
            }
        }
        None
    }
}

impl<T: AsRef<SpanSet>> ExactSizeIterator for SpanSetIter<T> {
    fn len(&self) -> usize {
        self.count_remaining() as _
    }
}

impl IntoIterator for SpanSet {
    type Item = Id;
    type IntoIter = SpanSetIter<SpanSet>;

    /// Get an iterator for integers in this [`SpanSet`].
    fn into_iter(self) -> SpanSetIter<SpanSet> {
        let len = self.spans.len();
        let back = (
            len as isize - 1,
            if len == 0 {
                0
            } else {
                let span = self.spans[len - 1];
                span.high.0 - span.low.0
            },
        );
        SpanSetIter {
            span_set: self,
            front: (0, 0),
            back,
        }
    }
}

impl AsRef<SpanSet> for SpanSet {
    fn as_ref(&self) -> &SpanSet {
        self
    }
}

// `#[serde(transparent)]` on the `Id` struct.
// This would be easier if `Id` has `#[serde(transparent)]`.
// But that might be a breaking change now...
mod flat_id {
    use serde::de;
    use serde::de::Visitor;
    use serde::Deserializer;
    use serde::Serializer;

    use super::*;

    pub fn serialize<S: Serializer>(id: &Id, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(id.0)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Id, D::Error> {
        struct IdVisitor;
        impl<'de> Visitor<'de> for IdVisitor {
            type Value = Id;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("u64")
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Id(value))
            }
        }
        deserializer.deserialize_u64(IdVisitor)
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod tests {
    use super::*;

    impl From<RangeInclusive<u64>> for Span {
        fn from(range: RangeInclusive<u64>) -> Span {
            Span::new(Id(*range.start()), Id(*range.end()))
        }
    }

    impl From<u64> for Span {
        fn from(id: u64) -> Span {
            let id = Id(id);
            Span::new(id, id)
        }
    }

    impl From<(u64, u64)> for SpanSet {
        fn from(ids: (u64, u64)) -> SpanSet {
            SpanSet::from_spans([ids.0, ids.1].iter().cloned().map(Id))
        }
    }

    impl From<Span> for RangeInclusive<u64> {
        fn from(span: Span) -> RangeInclusive<u64> {
            span.low.0..=span.high.0
        }
    }

    #[test]
    fn test_overlapped_spans() {
        let span = SpanSet::from_spans(vec![1..=3, 3..=4]);
        assert_eq!(span.as_spans(), &[Span::from(1..=4)]);
    }

    #[test]
    fn test_valid_spans() {
        SpanSet::empty();
        SpanSet::from_spans(vec![4..=4, 3..=3, 1..=2]);
    }

    #[test]
    fn test_from_sorted_spans_merge() {
        let s = SpanSet::from_sorted_spans(vec![4..=4, 3..=3, 1..=2]);
        assert_eq!(format!("{:?}", s), "1..=4");
    }

    #[test]
    fn test_count() {
        let set = SpanSet::empty();
        assert_eq!(set.count(), 0);

        let set = SpanSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        assert_eq!(set.count(), 10 + 1 + 10);
    }

    #[test]
    fn test_skip() {
        let set = SpanSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        let skip = |n| format!("{:?}", set.skip(n));
        assert_eq!(skip(0), "1..=10 20 31..=40");
        assert_eq!(skip(1), "1..=10 20 31..=39");
        assert_eq!(skip(9), "1..=10 20 31");
        assert_eq!(skip(10), "1..=10 20");
        assert_eq!(skip(11), "1..=10");
        assert_eq!(skip(12), "1..=9");
        assert_eq!(skip(20), "1");
        assert_eq!(skip(21), "");
        assert_eq!(skip(22), "");
        assert_eq!(skip(50), "");
    }

    #[test]
    fn test_take() {
        let set = SpanSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        let take = |n| format!("{:?}", set.take(n));
        assert_eq!(take(0), "");
        assert_eq!(take(1), "40");
        assert_eq!(take(9), "32..=40");
        assert_eq!(take(10), "31..=40");
        assert_eq!(take(11), "20 31..=40");
        assert_eq!(take(12), "10 20 31..=40");
        assert_eq!(take(20), "2..=10 20 31..=40");
        assert_eq!(take(21), "1..=10 20 31..=40");
        assert_eq!(take(22), "1..=10 20 31..=40");
        assert_eq!(take(50), "1..=10 20 31..=40");
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

    fn union(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<u64>> {
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

    fn intersect(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<u64>> {
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

    fn difference(a: Vec<impl Into<Span>>, b: Vec<impl Into<Span>>) -> Vec<RangeInclusive<u64>> {
        let a = SpanSet::from_spans(a);
        let b = SpanSet::from_spans(b);
        let spans1 = a.difference(&b).spans;
        let spans2 = b.difference(&a).spans;

        // |------------- a -------------------|
        // |--- spans1 ---|--- intersection ---|--- spans2 ---|
        //                |------------------- b -------------|
        let intersected = intersect(
            a.spans.iter().cloned().collect(),
            b.spans.iter().cloned().collect(),
        );
        let unioned = union(
            a.spans.iter().cloned().collect(),
            b.spans.iter().cloned().collect(),
        );
        assert_eq!(
            union(intersected.clone(), spans1.iter().cloned().collect()),
            union(a.spans.iter().cloned().collect(), Vec::<Span>::new())
        );
        assert_eq!(
            union(intersected.clone(), spans2.iter().cloned().collect()),
            union(b.spans.iter().cloned().collect(), Vec::<Span>::new())
        );
        assert_eq!(
            union(
                spans1.iter().cloned().collect(),
                union(intersected.clone(), spans2.iter().cloned().collect())
            ),
            unioned.clone(),
        );

        assert!(
            intersect(
                spans1.iter().cloned().collect(),
                spans2.iter().cloned().collect()
            )
            .is_empty()
        );
        assert!(intersect(spans1.iter().cloned().collect(), intersected.clone()).is_empty());
        assert!(intersect(spans2.iter().cloned().collect(), intersected.clone()).is_empty());

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
        assert!(set.iter_desc().next().is_none());
        assert!(set.iter_desc().rev().next().is_none());
        assert_eq!(set.iter_desc().size_hint(), (0, Some(0)));

        let set = SpanSet::from(0..=1);
        assert_eq!(set.iter_desc().collect::<Vec<Id>>(), vec![1, 0]);
        assert_eq!(set.iter_desc().rev().collect::<Vec<Id>>(), vec![0, 1]);
        assert_eq!(set.iter_desc().size_hint(), (2, Some(2)));
        assert_eq!(set.iter_desc().count(), 2);

        let mut iter = set.iter_desc();
        assert!(iter.next().is_some());
        assert!(iter.next_back().is_some());
        assert!(iter.next_back().is_none());

        let set = SpanSet::from_spans(vec![3..=5, 7..=8]);
        assert_eq!(set.iter_desc().collect::<Vec<Id>>(), vec![8, 7, 5, 4, 3]);
        assert_eq!(
            set.iter_desc().rev().collect::<Vec<Id>>(),
            vec![3, 4, 5, 7, 8]
        );
        assert_eq!(set.iter_desc().size_hint(), (5, Some(5)));
        assert_eq!(set.iter_desc().last(), Some(Id(3)));

        assert_eq!(
            set.clone().into_iter().collect::<Vec<Id>>(),
            vec![8, 7, 5, 4, 3]
        );
        assert_eq!(
            set.clone().into_iter().rev().collect::<Vec<Id>>(),
            vec![3, 4, 5, 7, 8]
        );
        assert_eq!(
            set.clone()
                .into_iter()
                .rev()
                .skip(1)
                .take(2)
                .rev()
                .collect::<Vec<Id>>(),
            vec![5, 4]
        );

        let set = SpanSet::from_spans(vec![3..=5, 7..=8]);
        let mut iter = set.iter_desc();
        assert_eq!(iter.next().unwrap(), 8);
        assert_eq!(iter.next_back().unwrap(), 3);

        let mut iter2 = iter.clone();
        assert_eq!(iter.next().unwrap(), 7);
        assert_eq!(iter.next_back().unwrap(), 4);
        assert_eq!(iter2.next().unwrap(), 7);
        assert_eq!(iter2.next_back().unwrap(), 4);
    }

    #[test]
    fn test_push() {
        let mut set = SpanSet::from(10..=20);
        set.push(5..=15);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=20)]);

        let mut set = SpanSet::from(10..=20);
        set.push(5..=9);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=20)]);

        let mut set = SpanSet::from(10..=20);
        set.push(5..=8);
        assert_eq!(
            set.as_spans(),
            &vec![Span::from(10..=20), Span::from(5..=8)]
        );

        let mut set = SpanSet::from(10..=20);
        set.push(5..=30);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=30)]);

        let mut set = SpanSet::from(10..=20);
        set.push(20..=30);
        assert_eq!(set.as_spans(), &vec![Span::from(10..=30)]);

        let mut set = SpanSet::from(10..=20);
        set.push(10..=20);
        assert_eq!(set.as_spans(), &vec![Span::from(10..=20)]);

        let mut set = SpanSet::from(10..=20);
        set.push(22..=30);
        assert_eq!(
            set.as_spans(),
            &vec![Span::from(22..=30), Span::from(10..=20)]
        );
    }

    #[test]
    fn test_push_brute_force() {
        // Brute force pushing all spans in 1..=45 range to a SpanSet.
        let set = SpanSet::from_spans(vec![5..=10, 15..=16, 18..=20, 23..=23, 26..=30, 35..=40]);
        for low in 1..=45 {
            for high in low..=45 {
                let expected = SpanSet::from_spans(
                    (1..=45)
                        .filter(|&i| (i >= low && i <= high) || set.contains(Id(i)))
                        .map(Id),
                );
                let mut set = set.clone();
                set.push(low..=high);
                assert_eq!(set.as_spans(), expected.as_spans());
            }
        }
    }

    #[test]
    fn test_span_contains_brute_force() {
        let set = SpanSet::from_spans(vec![5..=10, 15..=16, 18..=20, 23..=23, 26..=30, 35..=40]);
        for low in 1..=45 {
            for high in low..=45 {
                let span = Span::from(low..=high);
                let result1 = set.span_contains(span);
                let result2 = set
                    .as_spans()
                    .iter()
                    .find(|s| s.contains(Id(low)) && s.contains(Id(high)));
                assert_eq!(result1, result2);
            }
        }
    }

    #[test]
    fn test_span_iter_nth() {
        let set = SpanSet::from_spans(vec![5..=10, 15..=15, 18..=20, 23..=23, 26..=30, 35..=40]);
        let vec: Vec<Id> = set.iter_desc().collect();
        for n in 0..=(vec.len() + 2) {
            assert_eq!(set.iter_desc().nth(n), vec.get(n).cloned());
        }
    }

    #[test]
    fn test_span_iter_nth_back() {
        let set = SpanSet::from_spans(vec![5..=10, 15..=15, 18..=20, 23..=23, 26..=30, 35..=40]);
        let vec: Vec<Id> = set.iter_asc().collect();
        for n in 0..=(vec.len() + 2) {
            assert_eq!(set.iter_desc().nth_back(n), vec.get(n).cloned());
        }
    }

    #[test]
    fn test_intersection_span_min() {
        let set = SpanSet::from_spans(vec![1..=10, 11..=20, 30..=40]);
        assert_eq!(set.intersection_span_min((15..=45).into()), Some(Id(15)));
        assert_eq!(set.intersection_span_min((20..=32).into()), Some(Id(20)));
        assert_eq!(set.intersection_span_min((21..=29).into()), None);
        assert_eq!(set.intersection_span_min((21..=32).into()), Some(Id(30)));
        assert_eq!(set.intersection_span_min((35..=45).into()), Some(Id(35)));
        assert_eq!(set.intersection_span_min((45..=55).into()), None);
    }

    #[test]
    fn test_debug() {
        let set = SpanSet::from_spans(vec![1..=1, 2..=9, 10..=10, 20..=20, 31..=35, 36..=40]);
        assert_eq!(format!("{:10?}", &set), "1..=10 20 31..=40");
        assert_eq!(format!("{:3?}", &set), "1..=10 20 31..=40");
        assert_eq!(format!("{:2?}", &set), "1..=10 20 and 1 span");
        assert_eq!(format!("{:1?}", &set), "1..=10 and 2 spans");
    }
}
