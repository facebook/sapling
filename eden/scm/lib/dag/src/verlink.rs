/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::sync::atomic;
use std::sync::atomic::AtomicU32;

/// A linked list tracking a logic "version" with compatibility rules:
/// - Append-only changes bump the version, the new version is backwards
///   compatible.
/// - Non-append-only changes create a new version that is incompatible with
///   all other versions (in the current process).
/// - Clones (cheaply) preserve the version.
///
/// Supported operations:
/// - `new() -> x`: Create a new version that is not comparable (compatible)
///   with other versions.
/// - `clone(x) -> y`: Clone `x` to `y`. `x == y`. `x` and `y` are compatible.
/// - `bump(x) -> y`: Bump `x` to `y`. `y > x`. `y` is backwards-compatible with
///   `x`. Note: `y` is not comparable (compatible) with other `bump(x)`.
/// - `x > y`: `true` if `x` is backwards compatible with `y`.
///
/// The linked list can be shared in other linked lists. So they form a tree
/// effectively. Comparing to a DAG, there is no "merge" operation.
/// Compatibility questions become reachability questions.
#[derive(Clone)]
pub struct VerLink {
    inner: Arc<Inner>,
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
    r#gen: u32,
}

impl VerLink {
    /// Creates a new `VerLink` that is incompatible with all other `VerLink`s
    /// in the process.
    pub fn new() -> Self {
        let inner = Inner {
            parent: None,
            base: next_id(),
            r#gen: 0,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Bumps the `VerLink` for backward-compatible (ex. append-only) changes.
    /// Note the "append-only" means only adding commits without "stripping"
    /// or "rewriting" commits. It is different from the "append-only" concept
    /// from the storage layer, because the "stripping" or "rewriting" might
    /// be implemented as "appending" special data on the storage layer.
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
                    r#gen: self.inner.r#gen + 1,
                };
                let next = Self {
                    inner: Arc::new(next_inner),
                };
                *self = next;
            }
        }
    }
}

impl PartialOrd for VerLink {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        if self.inner.base != other.inner.base {
            // Fast path: self and other have different root nodes and are not
            // reachable from each other.
            return None;
        }
        if self.inner.r#gen < other.inner.r#gen {
            other.partial_cmp(self).map(|o| o.reverse())
        } else {
            debug_assert!(self.inner.r#gen >= other.inner.r#gen);
            if Arc::ptr_eq(&self.inner, &other.inner) {
                return Some(cmp::Ordering::Equal);
            }
            let mut cur = self.inner.parent.as_ref();
            while let Some(this) = cur {
                if this.inner.r#gen < other.inner.r#gen {
                    // Fast path: not possible to reach other from here.
                    return None;
                }
                if Arc::ptr_eq(&this.inner, &other.inner) {
                    return Some(cmp::Ordering::Greater);
                }
                cur = this.inner.parent.as_ref();
            }
            None
        }
    }
}

impl PartialEq for VerLink {
    fn eq(&self, other: &Self) -> bool {
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

/// A small cache that associate `storage_version` with `VerLink`.
/// Useful to "restore" `VerLink` after reading the same content from disk.
/// For each filesystem path we only cache its latest version to reduce memory
/// "leak" caused by a static state.
static CACHE: OnceLock<RwLock<HashMap<String, ((u64, u64), VerLink)>>> = OnceLock::new();

fn storage_version_cache() -> &'static RwLock<HashMap<String, ((u64, u64), VerLink)>> {
    CACHE.get_or_init(Default::default)
}

// Cache related.
impl VerLink {
    /// Clear the cache that maps storage version to VerLink.
    pub fn clear_storage_version_cache() {
        let mut cache = storage_version_cache().write().unwrap();
        cache.clear();
    }

    /// Lookup a `VerLink` from a given storage version.
    pub fn from_storage_version(str_id: &str, version: (u64, u64)) -> Option<VerLink> {
        let cache = storage_version_cache().read().unwrap();
        let (cached_version, verlink) = cache.get(str_id)?;
        if cached_version == &version {
            Some(verlink.clone())
        } else {
            None
        }
    }

    /// Associate the `VerLink` with a storage version.
    pub fn associate_storage_version(&self, str_id: String, version: (u64, u64)) {
        let mut cache = storage_version_cache().write().unwrap();
        cache.insert(str_id, (version, self.clone()));
    }

    /// Lookup a `VerLink` from a given storage version, or create a new `VerLink`
    /// and remember it in cache.
    pub fn from_storage_version_or_new(str_id: &str, version: (u64, u64)) -> VerLink {
        match Self::from_storage_version(str_id, version) {
            Some(v) => v,
            None => {
                let v = Self::new();
                v.associate_storage_version(str_id.to_string(), version);
                v
            }
        }
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
        assert_eq!(compatible(&b, &c), Some(&c));
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

    #[test]
    fn test_storage_version_cache() {
        let a = VerLink::new();
        let v = (100, 200);
        assert!(VerLink::from_storage_version("x", v).is_none());
        a.associate_storage_version("x".to_string(), v);
        assert_eq!(VerLink::from_storage_version("x", v).unwrap(), a);

        let b = VerLink::from_storage_version_or_new("y", v);
        assert_ne!(&b, &a);
        let c = VerLink::from_storage_version_or_new("y", v);
        assert_eq!(&b, &c);
    }

    /// Find the more compatible version.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn compatible<'a>(a: &'a VerLink, b: &'a VerLink) -> Option<&'a VerLink> {
        if a == b {
            assert!(!(a != b));
            assert!(!(a < b));
            assert!(!(b < a));
            assert!(!(a > b));
            assert!(!(b > a));
            Some(a)
        } else if a < b {
            assert!(!(a == b));
            assert!(a != b);
            assert!(!(b < a));
            assert!(!(a > b));
            assert!(b > a);
            Some(b)
        } else if a > b {
            assert!(!(a == b));
            assert!(a != b);
            assert!(!(a < b));
            assert!(b < a);
            assert!(!(b > a));
            Some(a)
        } else {
            assert!(!(a == b));
            assert!(a != b);
            assert!(!(a < b));
            assert!(!(a > b));
            assert!(!(b < a));
            assert!(!(b > a));
            None
        }
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
