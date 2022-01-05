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

use crate::scmstore::attrs::StoreAttrs;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TreeAttributes {
    pub content: bool,
}

impl StoreAttrs for TreeAttributes {
    const NONE: Self = TreeAttributes { content: false };

    /// Returns all the attributes which are present or can be computed from present attributes.
    fn with_computable(&self) -> TreeAttributes {
        *self
    }
}

impl TreeAttributes {
    pub const CONTENT: Self = TreeAttributes { content: true };
}

impl Not for TreeAttributes {
    type Output = Self;

    fn not(self) -> Self::Output {
        TreeAttributes {
            content: !self.content,
        }
    }
}

impl BitAnd for TreeAttributes {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        TreeAttributes {
            content: self.content & rhs.content,
        }
    }
}

impl BitOr for TreeAttributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        TreeAttributes {
            content: self.content | rhs.content,
        }
    }
}

/// The subtraction operator is implemented here to mean "set difference" aka relative complement.
impl Sub for TreeAttributes {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self & !rhs
    }
}
