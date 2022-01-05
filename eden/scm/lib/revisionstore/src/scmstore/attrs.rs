/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::BitAnd;
use std::ops::BitOr;
use std::ops::Not;
use std::ops::Sub;

pub trait StoreAttrs:
    Copy
    + Clone
    + BitAnd<Output = Self>
    + BitOr<Output = Self>
    + Not<Output = Self>
    + Sub<Output = Self>
    + PartialEq
    + std::fmt::Debug
    + Sized
{
    const NONE: Self;
    fn with_computable(&self) -> Self;

    /// Returns true if no attributes are set, otherwise false.
    fn none(&self) -> bool {
        *self == Self::NONE
    }

    /// Returns true if at least one attribute is set, otherwise false.
    fn any(&self) -> bool {
        *self != Self::NONE
    }

    /// Returns true if all attributes are set, otherwise false.
    fn all(&self) -> bool {
        !*self == Self::NONE
    }

    /// Returns true if all the specified attributes are set, otherwise false.
    fn has(&self, attrs: Self) -> bool {
        (attrs - *self).none()
    }
}
