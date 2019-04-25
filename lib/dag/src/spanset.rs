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
}
