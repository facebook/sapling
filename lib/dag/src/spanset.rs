// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # spanset
//!
//! See [`SpanSet`] for the main structure.

type Id = u64;
type Span = (Id, Id);

/// A set of integer spans.
#[derive(Clone, Debug)]
pub struct SpanSet {
    spans: Vec<Span>,
}

impl SpanSet {
    /// Construct a [`SpanSet`] containing given sorted spans.
    ///
    /// Panic if the provided `spans` is not in decreasing `(start, end)` order,
    /// or has overlapped spans.
    pub fn from_sorted_spans(spans: Vec<Span>) -> Self {
        let result = SpanSet { spans };
        assert!(result.is_valid());
        result
    }

    /// Check if the spans satisfies internal assumptions: sorted and not
    /// overlapped.
    fn is_valid(&self) -> bool {
        self.spans
            .iter()
            .rev()
            .cloned()
            .fold((-1, true), |(last_end, is_sorted), (start, end)| {
                (
                    end as i64,
                    is_sorted && last_end < start as i64 && start <= end,
                )
            })
            .1
    }

    /// Count integers covered by this [`SpanSet`].
    pub fn count(&self) -> u64 {
        self.spans
            .iter()
            .fold(0, |acc, (start, end)| acc + (end - start + 1) as u64)
    }

    /// Tests if a given value exists in this set.
    pub fn contains(&self, value: Id) -> bool {
        match self.spans.binary_search_by(|probe| value.cmp(&probe.0)) {
            Ok(_) => true,
            Err(idx) => self
                .spans
                .get(idx)
                .map(|span| span.0 <= value && span.1 >= value)
                .unwrap_or(false),
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
                    if left.1 < right.1 {
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
                    let span = (left.0.max(right.0), left.1.min(right.1));
                    if span.0 <= span.1 {
                        push(span);
                    }
                    let right1 = (right.1 as i64).min(span.0 as i64 - 1);
                    if right1 >= right.0 as i64 {
                        next_right = Some((right.0, right1 as Id));
                    } else {
                        next_right = iter_right.next();
                    }
                    let left1 = (left.1 as i64).min(span.0 as i64 - 1);
                    if left1 >= left.0 as i64 {
                        next_left = Some((left.0, left1 as Id));
                    } else {
                        next_left = iter_left.next();
                    }
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
                    if right.0 > left.1 {
                        next_right = iter_right.next();
                    } else {
                        next_left = if right.1 < left.0 {
                            push(left);
                            iter_left.next()
                        } else {
                            // |----------------- left ------------------|
                            // |--- span1 ---|--- right ---|--- span2 ---|
                            let span2 = (right.1 + 1, left.1);
                            if span2.0 <= span2.1 {
                                push(span2);
                            }
                            if right.0 > 0 {
                                let span1 = (left.0, right.0 - 1);
                                if span1.0 <= span1.1 {
                                    Some(span1)
                                } else {
                                    iter_left.next()
                                }
                            } else {
                                None
                            }
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
                self.spans.last().map(|span| span.1 - span.0).unwrap_or(0),
            ),
        }
    }
}

/// Push a span to `Vec<Span>`. Try to union them in-place.
fn push_with_union(spans: &mut Vec<Span>, span: Span) {
    match spans.last_mut() {
        None => spans.push(span),
        Some(mut last) => {
            debug_assert!(last.1 >= span.1);
            if last.0 <= span.1 + 1 {
                // Union spans in-place.
                last.0 = last.0.min(span.0);
            } else {
                spans.push(span)
            }
        }
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
            self.front = if span_id == span.1 - span.0 {
                (vec_id + 1, 0)
            } else {
                (vec_id, span_id + 1)
            };
            Some(span.1 - span_id)
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
                    span.1 - span.0
                } else {
                    0
                };
                (vec_id - 1, span_len)
            } else {
                (vec_id, span_id - 1)
            };
            Some(span.1 - span_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_unordered_spans1() {
        SpanSet::from_sorted_spans(vec![(2, 1)]);
    }

    #[test]
    #[should_panic]
    fn test_unordered_spans2() {
        SpanSet::from_sorted_spans(vec![(1, 2), (3, 4)]);
    }

    #[test]
    #[should_panic]
    fn test_overlapped_spans() {
        SpanSet::from_sorted_spans(vec![(3, 4), (1, 3)]);
    }

    #[test]
    fn test_valid_spans() {
        SpanSet::from_sorted_spans(vec![]);
        SpanSet::from_sorted_spans(vec![(4, 4), (3, 3), (1, 2)]);
    }

    #[test]
    fn test_count() {
        let set = SpanSet::from_sorted_spans(Vec::new());
        assert_eq!(set.count(), 0);

        let set = SpanSet::from_sorted_spans(vec![(31, 40), (20, 20), (1, 10)]);
        assert_eq!(set.count(), 10 + 1 + 10);
    }

    #[test]
    fn test_contains() {
        let set = SpanSet::from_sorted_spans(Vec::new());
        assert!(!set.contains(0));
        assert!(!set.contains(10));

        let set = SpanSet::from_sorted_spans(vec![(31, 40), (20, 20), (1, 10)]);
        assert!(!set.contains(0));
        assert!(set.contains(1));
        assert!(set.contains(5));
        assert!(set.contains(10));
        assert!(!set.contains(11));

        assert!(!set.contains(19));
        assert!(set.contains(20));
        assert!(!set.contains(21));

        assert!(!set.contains(30));
        assert!(set.contains(31));
        assert!(set.contains(32));
        assert!(set.contains(39));
        assert!(set.contains(40));
        assert!(!set.contains(41));
    }

    fn union(a: Vec<Span>, b: Vec<Span>) -> Vec<Span> {
        let a = SpanSet::from_sorted_spans(a);
        let b = SpanSet::from_sorted_spans(b);
        let spans1 = a.union(&b).spans;
        let spans2 = b.union(&a).spans;
        assert_eq!(spans1, spans2);
        spans1
    }

    #[test]
    fn test_union() {
        assert_eq!(union(vec![(1, 10)], vec![(10, 20)]), vec![(1, 20)]);
        assert_eq!(union(vec![(1, 30)], vec![(10, 20)]), vec![(1, 30)]);
        assert_eq!(
            union(vec![(10, 10), (8, 8), (6, 6)], vec![(9, 9), (7, 7), (5, 5)]),
            vec![(5, 10)]
        );
        assert_eq!(
            union(vec![(10, 10), (8, 9), (6, 6)], vec![(5, 5)]),
            vec![(8, 10), (5, 6)]
        );
    }

    fn intersect(a: Vec<Span>, b: Vec<Span>) -> Vec<Span> {
        let a = SpanSet::from_sorted_spans(a);
        let b = SpanSet::from_sorted_spans(b);
        let spans1 = a.intersection(&b).spans;
        let spans2 = b.intersection(&a).spans;
        assert_eq!(spans1, spans2);
        spans1
    }

    #[test]
    fn test_intersection() {
        assert_eq!(intersect(vec![(1, 10)], vec![(11, 20)]), vec![]);
        assert_eq!(intersect(vec![(1, 10)], vec![(10, 20)]), vec![(10, 10)]);
        assert_eq!(intersect(vec![(1, 30)], vec![(10, 20)]), vec![(10, 20)]);
        assert_eq!(
            intersect(vec![(15, 20), (0, 10)], vec![(0, 30)]),
            vec![(15, 20), (0, 10)]
        );
        assert_eq!(
            intersect(vec![(15, 20), (0, 10)], vec![(5, 19)]),
            vec![(15, 19), (5, 10)]
        );
        assert_eq!(
            intersect(vec![(10, 10), (9, 9), (8, 8), (7, 7)], vec![(8, 11)]),
            vec![(8, 10)]
        );
        assert_eq!(
            intersect(vec![(10, 10), (9, 9), (8, 8), (7, 7)], vec![(5, 8)]),
            vec![(7, 8)]
        );
    }

    fn difference(a: Vec<Span>, b: Vec<Span>) -> Vec<Span> {
        let a = SpanSet::from_sorted_spans(a);
        let b = SpanSet::from_sorted_spans(b);
        let spans1 = a.difference(&b).spans;
        let spans2 = b.difference(&a).spans;

        // |------------- a -------------------|
        // |--- spans1 ---|--- intersection ---|--- spans2 ---|
        //                |------------------- b -------------|
        let intersected = intersect(a.spans.clone(), b.spans.clone());
        let unioned = union(a.spans.clone(), b.spans.clone());
        assert_eq!(
            union(intersected.clone(), spans1.clone()),
            union(a.spans.clone(), Vec::new())
        );
        assert_eq!(
            union(intersected.clone(), spans2.clone()),
            union(b.spans.clone(), Vec::new())
        );
        assert_eq!(
            union(spans1.clone(), union(intersected.clone(), spans2.clone())),
            unioned.clone(),
        );

        spans1
    }

    #[test]
    fn test_difference() {
        assert_eq!(difference(vec![(0, 5)], Vec::new()), vec![(0, 5)]);
        assert_eq!(difference(vec![], vec![(0, 5)]), vec![]);
        assert_eq!(difference(vec![(0, 0)], vec![(1, 1)]), vec![(0, 0)]);
        assert_eq!(difference(vec![(0, 0)], vec![(0, 1)]), vec![]);
        assert_eq!(difference(vec![(0, 10)], vec![(0, 5)]), vec![(6, 10)]);

        assert_eq!(
            difference(vec![(0, 10)], vec![(7, 8), (3, 4)]),
            vec![(9, 10), (5, 6), (0, 2)]
        );
        assert_eq!(
            difference(vec![(10, 12), (7, 8), (3, 4)], vec![(4, 11)]),
            vec![(12, 12), (3, 3)]
        );
    }

    #[test]
    fn test_iter() {
        let set = SpanSet::from_sorted_spans(vec![]);
        assert!(set.iter().next().is_none());
        assert!(set.iter().rev().next().is_none());

        let set = SpanSet::from_sorted_spans(vec![(0, 1)]);
        assert_eq!(set.iter().collect::<Vec<Id>>(), vec![1, 0]);
        assert_eq!(set.iter().rev().collect::<Vec<Id>>(), vec![0, 1]);

        let mut iter = set.iter();
        assert!(iter.next().is_some());
        assert!(iter.next_back().is_some());
        assert!(iter.next_back().is_none());

        let set = SpanSet::from_sorted_spans(vec![(7, 8), (3, 5)]);
        assert_eq!(set.iter().collect::<Vec<Id>>(), vec![8, 7, 5, 4, 3]);
        assert_eq!(set.iter().rev().collect::<Vec<Id>>(), vec![3, 4, 5, 7, 8]);
    }
}
