/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::BitAnd;
use std::ops::BitOr;
use std::ops::BitOrAssign;
use std::ops::Not;
use std::ops::Sub;

use edenapi_types::FileAttributes as SaplingRemoteApiFileAttributes;
use serde::Deserialize;
use serde::Serialize;

use crate::scmstore::attrs::StoreAttrs;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileAttributes {
    // "header" refers to the hg file header (e.g. copy info).
    pub pure_content: bool,
    pub content_header: bool,
    pub aux_data: bool,
}

impl From<FileAttributes> for SaplingRemoteApiFileAttributes {
    fn from(v: FileAttributes) -> Self {
        SaplingRemoteApiFileAttributes {
            content: v.pure_content || v.content_header,
            aux_data: v.aux_data,
        }
    }
}

impl StoreAttrs for FileAttributes {
    const NONE: Self = FileAttributes {
        pure_content: false,
        content_header: false,
        aux_data: false,
    };

    /// Returns all the attributes which are present or can be computed from present attributes.
    fn with_computable(&self) -> FileAttributes {
        if self.pure_content {
            *self | FileAttributes::AUX
        } else {
            *self
        }
    }
}

impl FileAttributes {
    pub const CONTENT: Self = FileAttributes {
        pure_content: true,
        content_header: true,
        aux_data: false,
    };

    // Don't need the content header.
    pub const PURE_CONTENT: Self = FileAttributes {
        pure_content: true,
        content_header: false,
        aux_data: false,
    };

    pub const AUX: Self = FileAttributes {
        pure_content: false,
        content_header: false,
        aux_data: true,
    };
}

impl Not for FileAttributes {
    type Output = Self;

    fn not(self) -> Self::Output {
        FileAttributes {
            pure_content: !self.pure_content,
            content_header: !self.content_header,
            aux_data: !self.aux_data,
        }
    }
}

impl BitAnd for FileAttributes {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        FileAttributes {
            pure_content: self.pure_content & rhs.pure_content,
            content_header: self.content_header & rhs.content_header,
            aux_data: self.aux_data & rhs.aux_data,
        }
    }
}

impl BitOr for FileAttributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        FileAttributes {
            pure_content: self.pure_content | rhs.pure_content,
            content_header: self.content_header | rhs.content_header,
            aux_data: self.aux_data | rhs.aux_data,
        }
    }
}

impl BitOrAssign for FileAttributes {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

/// The subtraction operator is implemented here to mean "set difference" aka relative complement.
impl Sub for FileAttributes {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self & !rhs
    }
}
