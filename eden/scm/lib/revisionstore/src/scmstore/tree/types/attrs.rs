/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ops::BitAnd;
use std::ops::BitOr;
use std::ops::BitOrAssign;
use std::ops::Not;
use std::ops::Sub;

use crate::scmstore::attrs::StoreAttrs;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TreeAttributes {
    pub content: bool,
    pub parents: bool,
    pub aux_data: bool,
}

impl StoreAttrs for TreeAttributes {
    const NONE: Self = TreeAttributes {
        content: false,
        parents: false,
        aux_data: false,
    };

    /// Returns all the attributes which are present or can be computed from present attributes.
    fn with_computable(&self) -> TreeAttributes {
        *self
    }
}

impl TreeAttributes {
    pub const CONTENT: Self = TreeAttributes {
        content: true,
        parents: false,
        aux_data: false,
    };
    pub const PARENTS: Self = TreeAttributes {
        content: false,
        parents: true,
        aux_data: false,
    };
    pub const AUX_DATA: Self = TreeAttributes {
        content: false,
        parents: false,
        aux_data: true,
    };
}

impl Not for TreeAttributes {
    type Output = Self;

    fn not(self) -> Self::Output {
        TreeAttributes {
            content: !self.content,
            parents: !self.parents,
            aux_data: !self.aux_data,
        }
    }
}

impl BitAnd for TreeAttributes {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        TreeAttributes {
            content: self.content & rhs.content,
            parents: self.parents & rhs.parents,
            aux_data: self.aux_data & rhs.aux_data,
        }
    }
}

impl BitOr for TreeAttributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        TreeAttributes {
            content: self.content | rhs.content,
            parents: self.parents | rhs.parents,
            aux_data: self.aux_data | rhs.aux_data,
        }
    }
}

impl BitOrAssign for TreeAttributes {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs
    }
}

/// The subtraction operator is implemented here to mean "set difference" aka relative complement.
impl Sub for TreeAttributes {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self & !rhs
    }
}
