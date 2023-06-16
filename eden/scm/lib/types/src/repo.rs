/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum StorageFormat {
    Revlog,
    RemoteFilelog,
    Eagerepo,
    Git,
}

impl StorageFormat {
    pub fn is_git(self) -> bool {
        self == Self::Git
    }

    pub fn is_eager(self) -> bool {
        self == Self::Eagerepo
    }
}
