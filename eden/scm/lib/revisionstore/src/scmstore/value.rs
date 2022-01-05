/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
