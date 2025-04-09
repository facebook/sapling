//! Track screen refresh regions.

use std::cmp::{max, min};

use crate::spanset::SpanSet;

/// Tracks which parts of the screen need to be refreshed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Refresh {
    /// Nothing to render.
    None,

    /// The rows in the bitset must be rendered.
    Rows(SpanSet),

    /// The whole screen must be rendered.
    All,
}

fn fill_range(b: &mut SpanSet, start: usize, end: usize, fill: bool) {
    if fill {
        b.extend(start..end);
    } else {
        b.remove_range(start..end);
    }
}

impl Refresh {
    /// Add a range of rows to the rows that must be rendered.
    pub(crate) fn add_range(&mut self, start: usize, end: usize) {
        match *self {
            Refresh::None => {
                let mut b = SpanSet::new();
                b.extend(start..end);
                *self = Refresh::Rows(b);
            }
            Refresh::Rows(ref mut b) => {
                b.extend(start..end);
            }
            Refresh::All => {}
        }
    }

    /// Rotate the range of rows between start and end upwards (towards 0).  Rows that roll past
    /// the start are dropped.  New rows introduced are filled with the fill value.
    pub(crate) fn rotate_range_up(&mut self, start: usize, end: usize, step: usize, fill: bool) {
        match *self {
            Refresh::All => {}
            Refresh::None => {
                if fill {
                    let mut b = SpanSet::new();
                    let mid = max(start, end.saturating_sub(step));
                    b.extend(mid..end);
                    *self = Refresh::Rows(b);
                }
            }
            Refresh::Rows(ref mut b) => {
                let mid = max(start, end.saturating_sub(step));
                for row in start..mid {
                    if b.contains(row + step) {
                        b.insert(row);
                    } else {
                        b.remove(row);
                    }
                }
                fill_range(b, mid, end, fill);
            }
        }
    }

    /// Rotate the range of rows between start and end downwards (away from 0).  Rows that roll
    /// past the end are dropped.  New rows introduced are filled with the fill value.
    pub(crate) fn rotate_range_down(&mut self, start: usize, end: usize, step: usize, fill: bool) {
        match *self {
            Refresh::None => {
                if fill {
                    let mut b = SpanSet::new();
                    let mid = min(start.saturating_add(step), end);
                    b.extend(start..mid);
                    *self = Refresh::Rows(b);
                }
            }
            Refresh::Rows(ref mut b) => {
                let mid = min(start.saturating_add(step), end);
                for row in (mid..end).rev() {
                    if b.contains(row - step) {
                        b.insert(row);
                    } else {
                        b.remove(row);
                    }
                }
                fill_range(b, start, mid, fill);
            }
            Refresh::All => {}
        }
    }

    /// Does the range contain the given orow
    pub(crate) fn contains(&self, row: usize) -> bool {
        match *self {
            Refresh::None => false,
            Refresh::Rows(ref b) => b.contains(row),
            Refresh::All => true,
        }
    }
}
