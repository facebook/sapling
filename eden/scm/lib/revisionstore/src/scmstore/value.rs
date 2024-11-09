/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::BitOr;

use crate::scmstore::attrs::StoreAttrs;

pub trait StoreValue: BitOr<Output = Self> + Default + Sized {
    type Attrs: StoreAttrs;

    /// Returns the attributes present in the value.
    fn attrs(&self) -> Self::Attrs;

    /// Return only the specified attributes of the value.
    fn mask(self, attrs: Self::Attrs) -> Self;
}
