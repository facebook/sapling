/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
