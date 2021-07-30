/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::{BitAnd, BitOr, Not, Sub};

use edenapi_types::FileAttributes as EdenApiFileAttributes;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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

impl FileAttributes {
    /// Returns all the attributes which are present or can be computed from present attributes.
    pub(crate) fn with_computable(&self) -> FileAttributes {
        if self.content {
            *self | FileAttributes::AUX
        } else {
            *self
        }
    }

    /// Returns true if all the specified attributes are set, otherwise false.
    pub fn has(&self, attrs: FileAttributes) -> bool {
        (attrs - *self).none()
    }

    /// Returns true if no attributes are set, otherwise false.
    pub fn none(&self) -> bool {
        *self == FileAttributes::NONE
    }

    /// Returns true if at least one attribute is set, otherwise false.
    pub fn any(&self) -> bool {
        *self != FileAttributes::NONE
    }

    /// Returns true if all attributes are set, otherwise false.
    pub fn all(&self) -> bool {
        !*self == FileAttributes::NONE
    }

    pub const NONE: Self = FileAttributes {
        content: false,
        aux_data: false,
    };

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
