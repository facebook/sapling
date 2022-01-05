/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Acquire;

/// Similar to `Box<T>` but provides `&'static T`.
/// Cannot be dropped directly.
#[repr(transparent)]
#[derive(Eq, PartialOrd, PartialEq, Hash)]
pub struct StaticBox<T>(Pin<Box<T>>);

impl<T: 'static> StaticBox<T> {
    /// Construct the [`StaticBox`].
    pub fn new(value: T) -> Self {
        Self(Box::pin(value))
    }

    /// Obtain the `'static` reference.
    pub fn static_ref(&self) -> &'static T {
        let result: &T = &self.0;
        // safety: StaticBox cannot be dropped and &T is not moving because of Pin.
        let result: &'static T = unsafe { std::mem::transmute(result) };
        result
    }

    /// Obtain the mutable reference.
    pub fn as_mut(&mut self) -> Pin<&mut T> {
        self.0.as_mut()
    }
}

impl<T> ops::Deref for StaticBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::borrow::Borrow<str> for StaticBox<String> {
    fn borrow(&self) -> &str {
        &**self.0
    }
}

pub(crate) static UNSAFE_ALLOW_DROP: AtomicBool = AtomicBool::new(false);

impl<T> Drop for StaticBox<T> {
    fn drop(&mut self) {
        if !UNSAFE_ALLOW_DROP.load(Acquire) {
            panic!("StaticBox cannot be dropped");
        }
    }
}
