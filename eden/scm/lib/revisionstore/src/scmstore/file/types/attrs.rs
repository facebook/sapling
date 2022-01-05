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

use edenapi_types::FileAttributes as EdenApiFileAttributes;
use serde::Deserialize;
use serde::Serialize;

use crate::scmstore::attrs::StoreAttrs;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileAttributes {
    pub content: bool,
    pub aux_data: bool,
}

impl From<FileAttributes> for EdenApiFileAttributes {
    fn from(v: FileAttributes) -> Self {
        EdenApiFileAttributes {
            content: v.content,
            aux_data: v.aux_data,
        }
    }
}

impl StoreAttrs for FileAttributes {
    const NONE: Self = FileAttributes {
        content: false,
        aux_data: false,
    };

    /// Returns all the attributes which are present or can be computed from present attributes.
    fn with_computable(&self) -> FileAttributes {
        if self.content {
            *self | FileAttributes::AUX
        } else {
            *self
        }
    }
}

impl FileAttributes {
    pub const CONTENT: Self = FileAttributes {
        content: true,
        aux_data: false,
    };

    pub const AUX: Self = FileAttributes {
        content: false,
        aux_data: true,
    };
}

impl Not for FileAttributes {
    type Output = Self;

    fn not(self) -> Self::Output {
        FileAttributes {
            content: !self.content,
            aux_data: !self.aux_data,
        }
    }
}

impl BitAnd for FileAttributes {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        FileAttributes {
            content: self.content & rhs.content,
            aux_data: self.aux_data & rhs.aux_data,
        }
    }
}

impl BitOr for FileAttributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        FileAttributes {
            content: self.content | rhs.content,
            aux_data: self.aux_data | rhs.aux_data,
        }
    }
}

/// The subtraction operator is implemented here to mean "set difference" aka relative complement.
impl Sub for FileAttributes {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self & !rhs
    }
}
