/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::BitOr;

use crate::scmstore::{
    tree::types::{LazyTree, TreeAttributes},
    value::StoreValue,
};

#[derive(Debug)]
pub struct StoreTree {
    pub(crate) content: Option<LazyTree>,
}

impl StoreValue for StoreTree {
    type Attrs = TreeAttributes;

    /// Returns which attributes are present in this StoreTree
    fn attrs(&self) -> TreeAttributes {
        TreeAttributes {
            content: self.content.is_some(),
        }
    }

    /// Return a StoreTree with only the specified subset of attributes
    fn mask(self, attrs: TreeAttributes) -> Self {
        StoreTree {
            content: if attrs.content { self.content } else { None },
        }
    }
}

impl BitOr for StoreTree {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        StoreTree {
            content: self.content.or(rhs.content),
        }
    }
}

impl Default for StoreTree {
    fn default() -> Self {
        StoreTree { content: None }
    }
}
impl From<LazyTree> for StoreTree {
    fn from(v: LazyTree) -> Self {
        StoreTree { content: Some(v) }
    }
}
