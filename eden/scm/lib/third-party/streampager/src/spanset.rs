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
use std::collections::VecDeque;
use std::ops::Bound;
use std::ops::RangeBounds;

type Id = usize;

/// Range `low..=high`. `low` must be <= `high`.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Span {
    pub(crate) low: Id,
    pub(crate) high: Id,
}

/// A set of integer spans.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct SpanSet {
    /// `spans` are sorted in DESC order.
    spans: VecDeque<Span>,
}

impl PartialOrd for Span {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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

    pub fn try_from_bounds(bounds: impl RangeBounds<Id>) -> Option<Self> {
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

impl From<Id> for Span {
    fn from(value: Id) -> Self {
        Self::new(value, value)
    }
}

impl<T: Into<Span>> From<T> for SpanSet {
    fn from(span: T) -> SpanSet {
        SpanSet::from_sorted_spans(std::iter::once(span.into()))
    }
}

impl SpanSet {
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

    /// Validate the spans are in the expected order and there are no mergeable
    /// adjacent spans.
    #[cfg(debug_assertions)]
    fn validate(&self) {
        for (i, span) in self.spans.iter().enumerate() {
            assert!(span.low <= span.high);
            if i > 0 {
                assert!(
                    span.high + 1 < self.spans[i - 1].low,
                    "spans are not in DESC order or has mergeable adjacent spans (around #{})",
                    i
                );
            }
        }
    }

    pub fn contains(&self, value: impl Into<Span>) -> bool {
        self.span_contains(value).is_some()
    }

    /// Find the [`Span`] that covers the given `value`.
    pub fn span_contains(&self, value: impl Into<Span>) -> Option<&Span> {
        let span = value.into();
        let idx = match self
            .spans
            .binary_search_by(|probe| span.low.cmp(&probe.low))
        {
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
                    .binary_search_by(|probe| (span.high + 1).cmp(&probe.low))
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
                *self = self.union(&SpanSet::from(span))
            }
        }
    }

    // `BitSet` compatibility.

    pub fn new() -> Self {
        Self::default()
    }

    pub fn extend(&mut self, bounds: impl RangeBounds<Id>) {
        if let Some(span) = Span::try_from_bounds(bounds) {
            self.push(span);
        }
    }

    pub fn insert(&mut self, id: Id) {
        self.push(id);
    }

    pub fn remove(&mut self, id: Id) {
        self.remove_range(id..=id);
    }

    pub fn remove_range(&mut self, bounds: impl RangeBounds<Id>) {
        if let Some(span) = Span::try_from_bounds(bounds) {
            // PERF: This is O(spans). But it can have a faster path.
            *self = self.difference(&span.into())
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
