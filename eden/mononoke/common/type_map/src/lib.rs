/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! TypeMap is a heterogeneous collection (i.e can store object of an aribitrary type).
//!
//! `TypeMap` can store single instance of an object per type. Any object of `Arc<T>` type
//! can be stored inside `TypeMap` as long as `T` implements `std::any::Any` trait.
//!
//! On a high level `TypeMap` is basically a mapping from `TypeId` to `Arc<dyn Any>`.
//! When object is inserted, its type is erased by converting `Arc<T>` to `Arc<dyn Any>`.
//! When we want to retrieve an object by its type it is done again by `TypeId` and `Arc<dyn Any>`
//! is converted back to `Arc<T>`.
//!
//! Special care is taken to make sure this collection works well with `Send` and `Sync` objects,
//! as well as `Arc<?Sized>` types (i.e `Arc<dyn Trait>` types).

use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

struct Handle<T: ?Sized + Send + Sync + 'static>(Arc<T>);

/// Heterogeneous collection of objects.
#[derive(Clone)]
pub struct TypeMap {
    mapping: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Default for TypeMap {
    fn default() -> Self {
        Self {
            mapping: Default::default(),
        }
    }
}

impl TypeMap {
    /// Create an empty collection.
    pub fn new() -> Self {
        Default::default()
    }

    /// Adds new object to the collection.
    ///
    /// Returns old value associated with this type, if any.
    pub fn insert<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) -> Option<Arc<T>> {
        self.mapping
            .insert(TypeId::of::<Handle<T>>(), Arc::new(Handle(value)))
            .and_then(|value| {
                let handle = value.downcast_ref::<Handle<T>>()?;
                Some(handle.0.clone())
            })
    }

    /// Get an object by its type.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<&Arc<T>> {
        self.mapping
            .get(&TypeId::of::<Handle<T>>())
            .and_then(|value| {
                let handle = value.downcast_ref::<Handle<T>>()?;
                Some(&handle.0)
            })
    }

    /// Number of elements stored in the collection.
    pub fn len(&self) -> usize {
        self.mapping.len()
    }

    /// Check if the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug)]
    struct Type(i32);

    trait Trait: Send + Sync {
        fn method(&self) -> i32;
    }

    impl Trait for Type {
        fn method(&self) -> i32 {
            self.0
        }
    }

    #[test]
    fn basic_test() {
        // make sure it works with unsized types
        let mut m = TypeMap::new();
        assert!(m.insert::<dyn Trait>(Arc::new(Type(42))).is_none());

        // ensure that it is Send + Sync
        let handle = std::thread::spawn(move || m.get::<dyn Trait>().cloned());
        assert_eq!(handle.join().unwrap().unwrap().method(), 42)
    }
}
