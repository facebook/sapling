// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Wrap an Arc to implement Hash and Eq in terms of pointer equality.

use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

use heapsize::HeapSizeOf;

/// Wrap an `Arc<T>` so that equality and hash are implemented in terms of the pointer
///
/// This allows a single instance of an object to be compared for equality and to be hashed -
/// in other words, may be used in a hashable container. This does not require `T` to implement
/// equality or hash.
#[derive(Debug)]
pub struct PtrWrap<T>(Arc<T>, usize);

impl<T> PtrWrap<T> {
    /// Wrap an `Arc<T>` so that it can be compared for equality and be hashed based on its
    /// pointer value.
    pub fn new(repo: &Arc<T>) -> Self {
        let repo = repo.clone();
        let ptr = repo.as_ref() as *const T as usize;
        PtrWrap(repo, ptr)
    }
}

impl<T> AsRef<Arc<T>> for PtrWrap<T> {
    fn as_ref(&self) -> &Arc<T> {
        &self.0
    }
}

impl<T> AsRef<T> for PtrWrap<T> {
    fn as_ref(&self) -> &T {
        self.0.as_ref()
    }
}

impl<T> Deref for PtrWrap<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> Eq for PtrWrap<T> {}

impl<T> PartialEq for PtrWrap<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Hash for PtrWrap<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.1)
    }
}

impl<T> Clone for PtrWrap<T> {
    fn clone(&self) -> Self {
        PtrWrap(self.0.clone(), self.1)
    }
}

impl<'a, T> From<&'a Arc<T>> for PtrWrap<T> {
    fn from(orig: &'a Arc<T>) -> Self {
        PtrWrap::new(orig)
    }
}

impl<T> HeapSizeOf for PtrWrap<T> {
    // Don't count the shared parts of the pointer as heap size
    fn heap_size_of_children(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn same_clone() {
        let v = Arc::new(1);
        let pw1 = PtrWrap::new(&v);
        let pw2 = pw1.clone();

        assert_eq!(pw1, pw2);
        assert_eq!(*pw1, *pw2);
    }

    #[test]
    fn same_arc() {
        let v1 = Arc::new(1);
        let v2 = v1.clone();
        let pw1 = PtrWrap::new(&v1);
        let pw2 = PtrWrap::new(&v2);

        assert_eq!(pw1, pw2);
        assert_eq!(*pw1, *pw2);
    }

    #[test]
    fn same_made() {
        let v = Arc::new(1);
        let pw1 = PtrWrap::new(&v);
        let pw2 = PtrWrap::new(&v);

        assert_eq!(pw1, pw2);
        assert_eq!(*pw1, *pw2);
    }

    #[test]
    fn different() {
        let v1 = Arc::new(1);
        let v2 = Arc::new(1);
        let pw1 = PtrWrap::new(&v1);
        let pw2 = PtrWrap::new(&v2);

        assert_ne!(pw1, pw2);
        assert_eq!(*pw1, *pw2);
    }
}
