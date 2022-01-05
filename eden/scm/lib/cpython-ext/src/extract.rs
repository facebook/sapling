/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::Python;

/// Trait for extracting a Rust object from a Python wrapper class.
///
/// A common pattern when writing bindings for Rust objects is to define
/// a Python wrapper class using the `py_class!` macro, with the underlying
/// Rust object stored as a data field within the Python object.
///
/// When Rust code interacts with a Python wrapper, it may want to work
/// with the underlying Rust object directly. This trait provides a means
/// to do so. Note that the `extract_inner` methods takes `&self`, meaning
/// the inner Rust value cannot be moved out of the wrapper. As a result,
/// the inner value will typically be wrapped in something like an `Arc`.
pub trait ExtractInner {
    type Inner;

    fn extract_inner(&self, py: Python) -> Self::Inner;
}

/// Similar to `ExtractInner`, but returns a reference to the wrapped
/// Rust object. Types that implement this trait will automatically
/// implement `ExtractInner` if the inner type implements `Clone`.
pub trait ExtractInnerRef {
    type Inner;

    fn extract_inner_ref<'a>(&'a self, py: Python<'a>) -> &'a Self::Inner;
}

impl<T> ExtractInner for T
where
    T: ExtractInnerRef,
    T::Inner: Clone + 'static,
{
    type Inner = <T as ExtractInnerRef>::Inner;

    fn extract_inner(&self, py: Python) -> Self::Inner {
        self.extract_inner_ref(py).clone()
    }
}
