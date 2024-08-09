/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # idset
//!
//! See [`IdSet`] for the main structure.

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
use std::sync::Arc;

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
pub struct IdSet {
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
    /// Similar to [`IdSet`], ids are sorted in descending order.
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

    /// Test if this span overlaps with another.
    pub fn overlaps_with(&self, other: &Self) -> bool {
        self.low <= other.high && other.low <= self.high
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

impl<T: Into<Span>> From<T> for IdSet {
    fn from(span: T) -> IdSet {
        IdSet::from_single_span(span.into())
    }
}

impl From<Span> for RangeInclusive<Id> {
    fn from(span: Span) -> RangeInclusive<Id> {
        span.low..=span.high
    }
}

// This is used by `gca(set)` where `set` usually contains 2 ids. The code
// can then be written as `gca((a, b))`.
impl From<(Id, Id)> for IdSet {
    fn from(ids: (Id, Id)) -> IdSet {
        IdSet::from_spans([ids.0, ids.1].iter().cloned())
    }
}

impl IdSet {
    /// Construct a [`IdSet`] containing given spans.
    /// Overlapped or adjacent spans will be merged automatically.
    pub fn from_spans<T: Into<Span>, I: IntoIterator<Item = T>>(spans: I) -> Self {
        let mut heap: BinaryHeap<Span> = spans.into_iter().map(|span| span.into()).collect();
        let mut spans = VecDeque::with_capacity(heap.len().min(64));
        while let Some(span) = heap.pop() {
            push_with_union(&mut spans, span);
        }
        let result = IdSet { spans };
        // `result` should be valid because the use of `push_with_union`.
        #[cfg(debug_assertions)]
        result.validate();
        result
    }

    /// Construct a [`IdSet`] that contains a single span.
    pub fn from_single_span(span: Span) -> Self {
        let spans: VecDeque<_> = std::iter::once(span).collect();
        Self { spans }
    }

    /// Construct a [`IdSet`] containing given spans.
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

    /// Construct an empty [`IdSet`].
    pub fn empty() -> Self {
        let spans = VecDeque::new();
        IdSet { spans }
    }

    /// Construct a full [`IdSet`] that contains everything.
    /// Warning: The [`Id`] in this set might be unknown to an actual storage.
    pub fn full() -> Self {
        Span::full().into()
    }

    /// Check if this [`IdSet`] contains nothing.
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

    /// Count integers covered by this [`IdSet`].
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
        let mut result = IdSet::empty();
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
        let mut result = IdSet::empty();
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
    pub fn union(&self, rhs: &IdSet) -> IdSet {
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
                    let result = IdSet { spans };
                    #[cfg(debug_assertions)]
                    result.validate();
                    return result;
                }
            }
        }
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, rhs: &IdSet) -> IdSet {
        let mut spans = VecDeque::with_capacity(self.spans.len().max(rhs.spans.len()).min(32));
        let push = |span: Span| push_with_union(&mut spans, span);
        intersect_iter(self.spans.iter().cloned(), rhs.spans.iter().cloned(), push);

        let result = IdSet { spans };
        #[cfg(debug_assertions)]
        result.validate();
        result
    }

    /// Calculates spans that are included only by this set, not `rhs`.
    pub fn difference(&self, rhs: &IdSet) -> IdSet {
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
                    let result = IdSet { spans };
                    #[cfg(debug_assertions)]
                    result.validate();
                    return result;
                }
            }
        }
    }

    /// Iterate `Id`s in descending order.
    pub fn iter_desc(&self) -> IdSetIter<&IdSet> {
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
        IdSetIter {
            span_set: self,
            front: (0, 0),
            back,
        }
    }

    /// Iterate `Id`s in ascending order.
    pub fn iter_asc(&self) -> Rev<IdSetIter<&Self>> {
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
        self.spans.front().map(|span| span.high)
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
            let last = &mut self.spans[0];
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

    /// Internal use only. Append a [`IdSet`], which must have lower
    /// boundaries than the existing spans.
    ///
    /// This is faster than [`IdSet::union`]. used when it's known
    /// that the all ids in `set` being added is below the minimal id
    /// in the `self` set.
    pub(crate) fn push_set(&mut self, set: &IdSet) {
        for span in &set.spans {
            self.push_span(*span);
        }
    }

    /// Get a reference to the spans.
    pub fn as_spans(&self) -> &VecDeque<Span> {
        &self.spans
    }

    /// Make this [`IdSet`] contain the specified `span`.
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
                let last = &mut self.spans[len - 1];
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
                let first = &mut self.spans[0];
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
                        let cur = &mut self.spans[idx];
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
                *self = self.union(&IdSet::from(span))
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
        let last = &mut spans[len - 1];
        debug_assert!(last.high >= span.high);
        if last.low <= span.high + 1 {
            // Union spans in-place.
            last.low = last.low.min(span.low);
        } else {
            spans.push_back(span)
        }
    }
}

impl Debug for IdSet {
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

/// Similar to `Span` but the iteration order is defined by `start` (inclusive) and `end`
/// (inclusive), not hardcoded DESC. `start` might be larger or smaller than `end`.
#[derive(Clone, Copy, Debug)]
pub struct OrderedSpan {
    pub start: Id,
    pub end: Id,
}

impl OrderedSpan {
    /// Number of `Id`s in the span. Must >= 1.
    pub fn count(&self) -> u64 {
        self.start.0.abs_diff(self.end.0) + 1
    }

    pub fn min(&self) -> Id {
        self.start.min(self.end)
    }

    pub fn max(&self) -> Id {
        self.start.max(self.end)
    }

    fn nth(&self, n: u64) -> Option<Id> {
        if self.start <= self.end {
            let id = self.start + n;
            if id > self.end { None } else { Some(id) }
        } else {
            let id = self.start - n;
            if id < self.end { None } else { Some(id) }
        }
    }

    fn skip(&self, n: u64) -> Option<Self> {
        if n >= self.count() {
            None
        } else if self.start <= self.end {
            Some(Self {
                start: self.start + n,
                end: self.end,
            })
        } else {
            Some(Self {
                start: self.start - n,
                end: self.end,
            })
        }
    }

    fn take(&self, n: u64) -> Option<Self> {
        if n == 0 {
            None
        } else if n >= self.count() {
            Some(*self)
        } else if self.start <= self.end {
            Some(Self {
                start: self.start,
                end: self.start + n - 1,
            })
        } else {
            Some(Self {
                start: self.start,
                end: self.start + 1 - n,
            })
        }
    }

    /// Attempt to push an `Id` to the current span and preserve iteration order.
    /// For example, `OrderedSpan { start: 10, end: 20 }.push(21)` produces
    /// `OrderedSpan { start: 10, end: 21 }`.
    fn maybe_push(&self, id: Id) -> Option<Self> {
        if id.group() == self.start.group()
            && ((self.start <= self.end && id == self.end + 1)
                || (self.start >= self.end && id + 1 == self.end))
        {
            Some(Self {
                start: self.start,
                end: id,
            })
        } else {
            None
        }
    }

    /// Attempt to push another [`OrderedSpan`] and preserve iteration order.
    /// For example,
    /// `OrderedSpan { start: 10, end: 20 }.push(OrderedSpan { start : 21, end: 30 })`
    /// produces `OrderedSpan { start: 10, end: 30 }`.
    fn maybe_push_span(&self, span: Self) -> Option<Self> {
        if span.start.group() == self.start.group()
            && ((self.start <= self.end && span.start == self.end + 1 && span.start <= span.end)
                || (self.start >= self.end && span.start + 1 == self.end && span.start >= span.end))
        {
            Some(Self {
                start: self.start,
                end: span.end,
            })
        } else {
            None
        }
    }
}

/// Used by [`IdSetIter`] for more flexible iteration.
pub trait IndexSpan {
    /// Get the span (start, end).
    /// The iteration starts from `start` (inclusive) and ends at `end` (inclusive).
    fn get_span(&self, index: usize) -> OrderedSpan;
}

impl IndexSpan for IdSet {
    fn get_span(&self, index: usize) -> OrderedSpan {
        let Span { low, high } = self.spans[index];
        // Iterate from `high` to `low` by default.
        OrderedSpan {
            start: high,
            end: low,
        }
    }
}

impl IndexSpan for &IdSet {
    fn get_span(&self, index: usize) -> OrderedSpan {
        <IdSet as IndexSpan>::get_span(self, index)
    }
}

/// Iterator of integers in a [`IdSet`].
#[derive(Clone)]
pub struct IdSetIter<T> {
    span_set: T,
    // (index of span_set.spans, index of span_set.spans[i])
    front: (isize, u64),
    back: (isize, u64),
}

impl<T: IndexSpan> IdSetIter<T> {
    fn count_remaining(&self) -> u64 {
        let mut front = self.front;
        let back = self.back;
        let mut count = 0;
        while front <= back {
            let (vec_id, span_id) = front;
            let (back_vec_id, back_span_id) = back;
            if vec_id < back_vec_id {
                let span = self.span_set.get_span(vec_id as usize);
                count += span.count().saturating_sub(span_id);
                front = (vec_id + 1, 0);
            } else {
                count += back_span_id - span_id + 1;
                front = (vec_id + 1, 0);
            }
        }
        count
    }
}

impl<T: IndexSpan> Iterator for IdSetIter<T> {
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
            let span = self.span_set.get_span(vec_id as usize);
            self.front = if span_id + 1 == span.count() {
                (vec_id + 1, 0)
            } else {
                (vec_id, span_id + 1)
            };
            #[cfg(test)]
            assert_eq!(self.size_hint().0 + 1, old_size);
            span.nth(span_id)
        }
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        #[cfg(test)]
        let expected_size = self.size_hint().0.max(n + 1) - n - 1;
        let mut n = n as u64;
        while self.front <= self.back {
            let (vec_id, span_id) = self.front;
            let span = self.span_set.get_span(vec_id as usize);
            let span_remaining = span.count() - span_id;
            if n >= span_remaining {
                n -= span_remaining;
                self.front = (vec_id + 1, 0)
            } else {
                let span_id = span_id + n;
                self.front = (vec_id, span_id);
                let result = self.next();
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

impl<T: IndexSpan> DoubleEndedIterator for IdSetIter<T> {
    fn next_back(&mut self) -> Option<Id> {
        if self.front > self.back {
            #[cfg(test)]
            assert_eq!(self.size_hint().0, 0);
            None
        } else {
            #[cfg(test)]
            let old_size = self.size_hint().0;
            let (vec_id, span_id) = self.back;
            let span = self.span_set.get_span(vec_id as usize);
            self.back = if span_id == 0 {
                let span_len = if vec_id > 0 {
                    let span = self.span_set.get_span((vec_id - 1) as usize);
                    span.count() - 1
                } else {
                    0
                };
                (vec_id - 1, span_len)
            } else {
                (vec_id, span_id - 1)
            };
            #[cfg(test)]
            assert_eq!(self.size_hint().0 + 1, old_size);
            span.nth(span_id)
        }
    }

    fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
        #[cfg(test)]
        let expected_size = self.size_hint().0.max(n + 1) - n - 1;
        let mut n = n as u64;
        while self.front <= self.back {
            let (vec_id, span_id) = self.back;
            let span = self.span_set.get_span(vec_id as usize);
            let span_remaining = span_id + 1;
            if n >= span_remaining {
                n -= span_remaining;
                let span_end = if vec_id > 0 {
                    self.span_set.get_span((vec_id - 1) as usize).count() - 1
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
                            self.span_set.get_span((vec_id - 1) as usize).count() - 1
                        } else {
                            0
                        };
                        self.back = (vec_id - 1, span_end);
                    } else {
                        self.back.1 -= 1;
                    }
                    span.nth(span_id)
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

impl<T: IndexSpan> ExactSizeIterator for IdSetIter<T> {
    fn len(&self) -> usize {
        self.count_remaining() as _
    }
}

impl IntoIterator for IdSet {
    type Item = Id;
    type IntoIter = IdSetIter<IdSet>;

    /// Get an iterator for integers in this [`IdSet`].
    fn into_iter(self) -> IdSetIter<IdSet> {
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
        IdSetIter {
            span_set: self,
            front: (0, 0),
            back,
        }
    }
}

/// Mainly for iteration (skip, take, count, into_iter) handling.
#[derive(Clone, Debug)]
pub struct IdList(Arc<Vec<OrderedSpan>>);

impl IdList {
    /// Creates `IdList`. Preserves `ids` iteration order.
    pub fn from_ids<I: Into<Id>>(ids: impl IntoIterator<Item = I>) -> Self {
        let mut list = Vec::new();
        let mut span = None;
        for id in ids {
            let id = id.into();
            span = match span {
                None => Some(OrderedSpan { start: id, end: id }),
                Some(current) => match current.maybe_push(id) {
                    Some(next) => Some(next),
                    None => {
                        list.push(current);
                        Some(OrderedSpan { start: id, end: id })
                    }
                },
            }
        }
        if let Some(span) = span.take() {
            list.push(span)
        }
        Self(Arc::new(list))
    }

    /// Creates `IdList`. Preserves `ids` iteration order.
    pub fn from_spans<S: Into<OrderedSpan>>(spans: impl IntoIterator<Item = S>) -> Self {
        let mut list = Vec::new();
        let mut span = None;
        for s in spans {
            let s = s.into();
            span = match span {
                None => Some(s),
                Some(current) => match current.maybe_push_span(s) {
                    Some(next) => Some(next),
                    None => {
                        list.push(current);
                        Some(s)
                    }
                },
            }
        }
        if let Some(span) = span.take() {
            list.push(span)
        }
        Self(Arc::new(list))
    }

    /// Count all `Id`s in the list.
    pub fn count(&self) -> u64 {
        self.0.iter().map(|i| i.count()).sum()
    }

    /// Skip the first `n` items.
    pub fn skip(&self, mut n: u64) -> Self {
        #[cfg(test)]
        let expected = self.count().saturating_sub(n);
        let mut result = Vec::new();
        for span in self.0.as_ref() {
            if n == 0 {
                result.push(*span);
            } else {
                let count = span.count();
                if count <= n {
                    // This span is skipped entirely.
                    n -= count;
                } else {
                    // This span is skipped partially.
                    debug_assert!(n > 0);
                    debug_assert!(n < count);
                    if let Some(span) = span.skip(n) {
                        result.push(span)
                    }
                    n = 0;
                }
            }
        }
        let result = Self(Arc::new(result));
        #[cfg(test)]
        assert_eq!(result.count(), expected);
        result
    }

    /// Only take the first `n` items.
    pub fn take(&self, mut n: u64) -> Self {
        #[cfg(test)]
        let expected = n.min(self.count());
        let mut result = Vec::new();
        for span in self.0.as_ref() {
            if n == 0 {
                break;
            } else {
                let count = span.count();
                if count <= n {
                    // This span is taken entirely.
                    n -= count;
                    result.push(*span);
                } else {
                    // Part of the span is the last to be taken.
                    debug_assert!(n > 0);
                    debug_assert!(n < count);
                    if let Some(span) = span.take(n) {
                        result.push(span)
                    }
                    n = 0;
                }
            }
        }
        let result = Self(Arc::new(result));
        #[cfg(test)]
        assert_eq!(result.count(), expected);
        result
    }

    /// Convert to `IdSet`.
    pub fn to_set(&self) -> IdSet {
        let spans = self.0.iter().map(|OrderedSpan { start, end }| {
            let (low, high) = if start <= end {
                (*start, *end)
            } else {
                (*end, *start)
            };
            Span::new(low, high)
        });
        IdSet::from_spans(spans)
    }

    /// Access `OrderedSpan` directly. This can be useful to figure out if the
    /// spans are in a particular order.
    pub fn as_spans(&self) -> &[OrderedSpan] {
        &self.0
    }
}

impl IndexSpan for Arc<Vec<OrderedSpan>> {
    fn get_span(&self, index: usize) -> OrderedSpan {
        self[index]
    }
}

impl IntoIterator for &IdList {
    type Item = Id;
    type IntoIter = IdSetIter<Arc<Vec<OrderedSpan>>>;

    fn into_iter(self) -> Self::IntoIter {
        let len = self.0.len();
        let back = (
            len as isize - 1,
            if len == 0 {
                0
            } else {
                self.0[len - 1].count() - 1
            },
        );
        IdSetIter {
            span_set: self.0.clone(),
            front: (0, 0),
            back,
        }
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
    use std::collections::HashSet;

    use quickcheck::quickcheck;

    use super::*;
    use crate::tests::dbg;
    use crate::tests::nid;

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

    impl From<(u64, u64)> for IdSet {
        fn from(ids: (u64, u64)) -> IdSet {
            IdSet::from_spans([ids.0, ids.1].iter().cloned().map(Id))
        }
    }

    impl From<Span> for RangeInclusive<u64> {
        fn from(span: Span) -> RangeInclusive<u64> {
            span.low.0..=span.high.0
        }
    }

    #[test]
    fn test_overlapped_spans() {
        let span = IdSet::from_spans(vec![1..=3, 3..=4]);
        assert_eq!(span.as_spans(), &[Span::from(1..=4)]);
    }

    #[test]
    fn test_valid_spans() {
        IdSet::empty();
        IdSet::from_spans(vec![4..=4, 3..=3, 1..=2]);
    }

    #[test]
    fn test_from_sorted_spans_merge() {
        let s = IdSet::from_sorted_spans(vec![4..=4, 3..=3, 1..=2]);
        assert_eq!(dbg(s), "1..=4");
    }

    #[test]
    fn test_count() {
        let set = IdSet::empty();
        assert_eq!(set.count(), 0);

        let set = IdSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        assert_eq!(set.count(), 10 + 1 + 10);
    }

    #[test]
    fn test_skip() {
        let set = IdSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        let skip = |n| dbg(set.skip(n));
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
        let set = IdSet::from_spans(vec![1..=10, 20..=20, 31..=40]);
        let take = |n| dbg(set.take(n));
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
        let set = IdSet::empty();
        assert!(!set.contains(0));
        assert!(!set.contains(10));

        let set = IdSet::from_spans(vec![1..=1, 2..=9, 10..=10, 20..=20, 31..=35, 36..=40]);
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
        let a = IdSet::from_spans(a);
        let b = IdSet::from_spans(b);
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
        let a = IdSet::from_spans(a);
        let b = IdSet::from_spans(b);
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
        let a = IdSet::from_spans(a);
        let b = IdSet::from_spans(b);
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
        let set = IdSet::empty();
        assert!(set.iter_desc().next().is_none());
        assert!(set.iter_desc().next_back().is_none());
        assert_eq!(set.iter_desc().size_hint(), (0, Some(0)));

        let set = IdSet::from(0..=1);
        assert_eq!(set.iter_desc().collect::<Vec<Id>>(), vec![1, 0]);
        assert_eq!(set.iter_desc().rev().collect::<Vec<Id>>(), vec![0, 1]);
        assert_eq!(set.iter_desc().size_hint(), (2, Some(2)));
        assert_eq!(set.iter_desc().count(), 2);

        let mut iter = set.iter_desc();
        assert!(iter.next().is_some());
        assert!(iter.next_back().is_some());
        assert!(iter.next_back().is_none());

        let set = IdSet::from_spans(vec![3..=5, 7..=8]);
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

        let set = IdSet::from_spans(vec![3..=5, 7..=8]);
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
        let mut set = IdSet::from(10..=20);
        set.push(5..=15);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=20)]);

        let mut set = IdSet::from(10..=20);
        set.push(5..=9);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=20)]);

        let mut set = IdSet::from(10..=20);
        set.push(5..=8);
        assert_eq!(
            set.as_spans(),
            &vec![Span::from(10..=20), Span::from(5..=8)]
        );

        let mut set = IdSet::from(10..=20);
        set.push(5..=30);
        assert_eq!(set.as_spans(), &vec![Span::from(5..=30)]);

        let mut set = IdSet::from(10..=20);
        set.push(20..=30);
        assert_eq!(set.as_spans(), &vec![Span::from(10..=30)]);

        let mut set = IdSet::from(10..=20);
        set.push(10..=20);
        assert_eq!(set.as_spans(), &vec![Span::from(10..=20)]);

        let mut set = IdSet::from(10..=20);
        set.push(22..=30);
        assert_eq!(
            set.as_spans(),
            &vec![Span::from(22..=30), Span::from(10..=20)]
        );
    }

    #[test]
    fn test_push_brute_force() {
        // Brute force pushing all spans in 1..=45 range to a IdSet.
        let set = IdSet::from_spans(vec![5..=10, 15..=16, 18..=20, 23..=23, 26..=30, 35..=40]);
        for low in 1..=45 {
            for high in low..=45 {
                let expected = IdSet::from_spans(
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
        let set = IdSet::from_spans(vec![5..=10, 15..=16, 18..=20, 23..=23, 26..=30, 35..=40]);
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
        let set = IdSet::from_spans(vec![5..=10, 15..=15, 18..=20, 23..=23, 26..=30, 35..=40]);
        let vec: Vec<Id> = set.iter_desc().collect();
        for n in 0..=(vec.len() + 2) {
            assert_eq!(set.iter_desc().nth(n), vec.get(n).cloned());
        }
    }

    #[test]
    fn test_span_iter_nth_back() {
        let set = IdSet::from_spans(vec![5..=10, 15..=15, 18..=20, 23..=23, 26..=30, 35..=40]);
        let vec: Vec<Id> = set.iter_asc().collect();
        for n in 0..=(vec.len() + 2) {
            assert_eq!(set.iter_desc().nth_back(n), vec.get(n).cloned());
        }
    }

    #[test]
    fn test_intersection_span_min() {
        let set = IdSet::from_spans(vec![1..=10, 11..=20, 30..=40]);
        assert_eq!(set.intersection_span_min((15..=45).into()), Some(Id(15)));
        assert_eq!(set.intersection_span_min((20..=32).into()), Some(Id(20)));
        assert_eq!(set.intersection_span_min((21..=29).into()), None);
        assert_eq!(set.intersection_span_min((21..=32).into()), Some(Id(30)));
        assert_eq!(set.intersection_span_min((35..=45).into()), Some(Id(35)));
        assert_eq!(set.intersection_span_min((45..=55).into()), None);
    }

    #[test]
    fn test_debug() {
        let set = IdSet::from_spans(vec![1..=1, 2..=9, 10..=10, 20..=20, 31..=35, 36..=40]);
        assert_eq!(format!("{:10?}", &set), "1..=10 20 31..=40");
        assert_eq!(format!("{:3?}", &set), "1..=10 20 31..=40");
        assert_eq!(format!("{:2?}", &set), "1..=10 20 and 1 span");
        assert_eq!(format!("{:1?}", &set), "1..=10 and 2 spans");
    }

    #[test]
    fn test_span_overlaps_with() {
        const N: u64 = 10;
        for span1_low in 0..N {
            for span1_high in span1_low..N {
                for span2_low in 0..N {
                    for span2_high in span2_low..N {
                        let span1 = Span::new(Id(span1_low), Id(span1_high));
                        let span2 = Span::new(Id(span2_low), Id(span2_high));
                        let overlap_naive = (span1_low..=span1_high)
                            .collect::<HashSet<_>>()
                            .intersection(&(span2_low..=span2_high).collect::<HashSet<_>>())
                            .count()
                            > 0;
                        assert_eq!(overlap_naive, span1.overlaps_with(&span2));
                    }
                }
            }
        }
    }

    fn check_id_list_iter(ids: &[Id]) {
        let list = IdList::from_ids(ids.iter().copied());
        assert_eq!(list.into_iter().next(), ids.first().copied());
        assert_eq!(list.into_iter().next_back(), ids.last().copied());
        let iter = list.into_iter();
        assert_eq!(iter.size_hint(), (ids.len(), Some(ids.len())));
        assert_eq!(iter.collect::<Vec<Id>>(), ids.to_vec());
        let iter = list.into_iter();
        let mut rev_ids = ids.to_vec();
        rev_ids.reverse();
        assert_eq!(iter.rev().collect::<Vec<Id>>(), rev_ids);
        for i in 0..=ids.len().min(10) {
            let nth = list.into_iter().nth(i);
            assert_eq!(nth, ids.get(i).copied(), "{:?}.nth({})", ids, i);
        }
    }

    #[test]
    fn test_id_list_iter() {
        check_id_list_iter(&[]);
        check_id_list_iter(&[
            Id(0),
            Id(1),
            Id(2),
            Id(5),
            Id(4),
            Id(3),
            nid(1),
            nid(2),
            nid(4),
            nid(3),
        ]);
    }

    #[test]
    fn test_id_list_iter_quickcheck() {
        fn check(ids: Vec<u8>) -> bool {
            let ids = ids.into_iter().map(|i| Id(i as u64)).collect::<Vec<Id>>();
            check_id_list_iter(&ids);
            true
        }
        quickcheck(check as fn(Vec<u8>) -> bool);
    }

    fn check_id_list_skip_take(list: &IdList, skip: u64, take: u64) {
        let sub_list = list.skip(skip);
        let sub_list = sub_list.take(take);
        let iter = list.into_iter();
        let ids = iter.skip(skip as _).take(take as _).collect::<Vec<_>>();
        assert_eq!(
            sub_list.into_iter().collect::<Vec<Id>>(),
            ids,
            "{:?}.skip({}).take({})",
            list,
            skip,
            take
        );
    }

    #[test]
    fn test_id_list_skip_take() {
        for ids in [
            &[] as &[u64],
            &[1],
            &[1, 2, 3, 7, 6, 5],
            &[7, 6, 5, 1, 2, 3],
            &[11, 12, 22, 21, 31, 32],
            &[10, 30, 20, 50, 40],
        ] {
            let len = ids.len() as u64;
            let list = IdList::from_ids(ids.iter().map(|&i| Id(i)));
            for skip in 0..=len + 2 {
                for take in 0..=len + 2 {
                    check_id_list_skip_take(&list, skip, take)
                }
            }
        }
    }

    #[test]
    fn test_id_list_skip_take_quickcheck() {
        fn check(ids: Vec<u8>, skip: u8, take: u8) -> bool {
            let list = IdList::from_ids(ids.into_iter().map(|i| Id(i as u64)));
            check_id_list_skip_take(&list, skip as _, take as _);
            true
        }
        quickcheck(check as fn(Vec<u8>, u8, u8) -> bool);
    }

    #[test]
    fn test_id_list_to_id_set() {
        let list = IdList::from_ids([1, 3, 2, 4, 9, 8, 11, 12, 6, 10].iter().map(|i| Id(*i)));
        let set = list.to_set();
        assert_eq!(dbg(set), "1..=4 6 8..=12");
    }

    #[test]
    fn test_id_list_from_spans() {
        let list = IdList::from_spans(
            [
                (10, 20),
                (21, 30),
                (90, 80),
                (79, 60),
                (51, 51),
                (50, 50),
                (55, 55),
                (56, 56),
            ]
            .iter()
            .map(|(a, b)| OrderedSpan {
                start: Id(*a),
                end: Id(*b),
            }),
        );
        assert_eq!(
            dbg(list.0),
            "[OrderedSpan { start: 10, end: 30 }, OrderedSpan { start: 90, end: 60 }, OrderedSpan { start: 51, end: 50 }, OrderedSpan { start: 55, end: 56 }]"
        );
    }
}
