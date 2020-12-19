/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bitflags::bitflags;
use std::fmt;
use std::sync::atomic::{self, AtomicU32};
use std::sync::Arc;

/// A linked list tracking a logic "version" with compatibility rules:
/// - Append-only changes bump the version, the new version is backwards
///   compatible.
/// - Non-append-only changes create a new version that is incompatible with
///   all other versions (in the current process).
/// - Clones (cheaply) preserve the version.
///
/// Supported operations:
/// - `new() -> x`: Create a new version that is incompatible with other
///   versions.
/// - `clone(x) -> y`: Clone `x` to `y`. `x` and `y` are compatible.
/// - `bump(x) -> y`: Bump `x` to `y`. `y` is backwards-compatible with `x`.
///   `x` is not backwards-compatible with `y`.
/// - `compatible(x, y) -> x | y | None`: Find the version that is
///   backwards-compatible with both `x` and `y`.
///
/// The linked list can be shared in other linked lists. So they form a tree
/// effectively. Comparing to a DAG, there is no "merge" operation.
/// Compatibility questions become reachability questions.
#[derive(Clone)]
pub struct VerLink {
    inner: Arc<Inner>,
}

bitflags! {
    /// Side of compatibility. Return value of `VerLink::compatible_side`.
    pub struct Side: u8 {
        /// The left side is backwards-compatible with and right side.
        const LEFT = 1;

        /// The right side is backwards-compatible with the left side.
        const RIGHT = 2;
    }
}

#[derive(Clone)]
struct Inner {
    parent: Option<VerLink>,

    /// The base number. Two `VerLink`s with different base numbers are incompatible.
    /// Used as an optimization to exit `compatible` early.
    base: u32,

    /// The "generation number", distance to root (the parentless `VerLink`).
    /// If `x.compatible(y)` returns `x`, then `x.gen` must be >= `y.gen`.
    /// Used as an optimization to exit `compatible` early.
    gen: u32,
}

impl VerLink {
    /// Creates a new `VerLink` that is incompatible with all other `VerLink`s
    /// in the process.
    pub fn new() -> Self {
        let inner = Inner {
            parent: None,
            base: next_id(),
            gen: 0,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Bumps the `VerLink` for backward-compatible (ex. append-only) changes.
    pub fn bump(&mut self) {
        match Arc::get_mut(&mut self.inner) {
            // This is an optimization to avoid increasing the length of the
            // linked list (which slows down "compatible" calculation) if
            // possible.
            Some(_inner) => {
                // Increasing gen is not necessary for correctness.
                // _inner.gen += 1;
            }
            None => {
                let next_inner = Inner {
                    parent: Some(self.clone()),
                    base: self.inner.base,
                    gen: self.inner.gen + 1,
                };
                let next = Self {
                    inner: Arc::new(next_inner),
                };
                *self = next;
            }
        }
    }

    /// Find the `VerLink` that is compatible with both `self` and `other`.
    /// Commutative. `compatible(a, b)` is `compatible(b, a)`.
    ///
    /// `compatible(a, b)` is `Some(b)`, if:
    /// - `b` is `a.clone()`, neither `a` nor `b` are `bump()`ed.
    /// - `b` was `a.clone()`, `b` was `bump()`ed since then, `a` wasn't `bump()`ed.
    ///
    /// `compatible(a, b)` is `None` if:
    /// - `b` wasn't created via `a.clone()`.
    /// - `b` was `a.clone()`, but both `b` and `a` are `bump()`ed afterwards.
    pub fn compatible<'a>(&'a self, other: &'a VerLink) -> Option<&'a VerLink> {
        if self.inner.base == other.inner.base {
            if self.inner.gen < other.inner.gen {
                return other.compatible(self);
            } else {
                // self.gen >= other.gen
                let mut cur = Some(self);
                while let Some(this) = cur {
                    if this.inner.gen < other.inner.gen {
                        break;
                    }
                    if Arc::ptr_eq(&this.inner, &other.inner) {
                        return Some(self);
                    }
                    cur = this.inner.parent.as_ref();
                }
            }
        }
        None
    }

    /// Find the "side" of compatibility.
    /// `LEFT`: `self` is backwards-compatible with `other`.
    /// `RIGHT`: `other` is backwards-compatible with `self`.
    pub fn compatible_side<'a>(&'a self, other: &'a VerLink) -> Side {
        let mut result = Side::empty();
        let compat = self.compatible(other);
        if compat == Some(self) {
            result |= Side::LEFT;
        }
        if compat == Some(other) {
            result |= Side::RIGHT;
        }
        result
    }
}

impl Side {
    /// Returns true if either `self.left()` or `self.right()` is true.
    pub fn either(self) -> bool {
        self != Self::empty()
    }

    /// Returns true if both `self.left()` and `self.right()` is true.
    pub fn both(self) -> bool {
        self == Self::LEFT | Self::RIGHT
    }

    /// Returns true if `self` contains `LEFT`.
    pub fn left(self) -> bool {
        self.contains(Self::LEFT)
    }

    /// Returns true if `self` contains `RIGHT`.
    pub fn right(self) -> bool {
        self.contains(Self::RIGHT)
    }

    /// Returns `left` if `self.left()` is true.
    /// Returns `right` if `self.right()` is true.
    /// Returns `None` otherwise.
    pub fn apply<T>(self, left: T, right: T) -> Option<T> {
        if self.left() {
            Some(left)
        } else if self.right() {
            Some(right)
        } else {
            None
        }
    }
}

impl PartialEq<&VerLink> for &VerLink {
    fn eq(&self, other: &&VerLink) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for VerLink {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut cur = Some(self);
        while let Some(this) = cur {
            write!(f, "{:p}", this.inner.as_ref())?;
            cur = this.inner.parent.as_ref();
            f.write_str("->")?;
            if cur.is_none() {
                write!(f, "{}", this.inner.base)?;
            }
        }
        Ok(())
    }
}

fn next_id() -> u32 {
    static ID: AtomicU32 = AtomicU32::new(0);
    ID.fetch_add(1, atomic::Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_individual_news_are_incompatible() {
        let a = VerLink::new();
        let b = VerLink::new();
        assert!(compatible(&a, &b).is_none());
    }

    #[test]
    fn test_clone_compatible() {
        let a = VerLink::new();
        let b = a.clone();
        assert_eq!(&a, &b);
        assert_eq!(compatible(&a, &b), Some(&b));
    }

    #[test]
    fn test_bump_is_different_and_backwards_compatible() {
        let a = VerLink::new();
        let mut b = a.clone();
        b.bump();
        assert_ne!(&b, &a);
        assert_eq!(compatible(&a, &b), Some(&b));

        b.bump();
        b.bump();
        assert_eq!(compatible(&a, &b), Some(&b));
    }

    #[test]
    fn test_clone_bump_twice() {
        let a = VerLink::new();
        let mut b = a.clone();
        b.bump();
        let mut c = b.clone();
        c.bump();
        assert_eq!(compatible(&a, &c), Some(&c));
    }

    #[test]
    fn test_bump_independently_become_incompatible() {
        let mut a = VerLink::new();
        let mut b = a.clone();
        b.bump();
        a.bump();
        assert_eq!(compatible(&a, &b), None);
    }

    #[test]
    fn test_bump_avoid_increase_len_if_possible() {
        let a = VerLink::new();
        let mut b = a.clone();
        assert_eq!(a.chain_len(), 1);
        assert_eq!(b.chain_len(), 1);
        b.bump(); // Increases chain_len by 1.
        assert_eq!(b.chain_len(), 2);
        b.bump(); // Does not change chain len.
        b.bump();
        assert_eq!(b.chain_len(), 2);
    }

    fn compatible<'a>(a: &'a VerLink, b: &'a VerLink) -> Option<&'a VerLink> {
        let result1 = a.compatible(b);
        let result2 = b.compatible(a);
        assert_eq!(result1, result2);
        let side = a.compatible_side(b);
        assert_eq!(side.contains(Side::LEFT), result1 == Some(a));
        assert_eq!(side.contains(Side::RIGHT), result1 == Some(b));
        assert_eq!(
            side.apply(Side::LEFT, Side::RIGHT).unwrap_or(Side::empty()),
            if side.both() { Side::LEFT } else { side }
        );
        result1
    }

    impl VerLink {
        /// Length of the linked list.
        fn chain_len(&self) -> usize {
            let mut len = 0;
            let mut cur = Some(self);
            while let Some(this) = cur {
                len += 1;
                cur = this.inner.parent.as_ref();
            }
            len
        }
    }
}
