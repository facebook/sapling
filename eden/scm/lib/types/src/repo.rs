/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
        // The "revlog" format writes to EagerRepoStore.
        // The pure Rust logic does not understand revlog but fine with eagerepo.
        // Note: The Python logic might still want to use the non-eager storage
        // like filescmstore etc.
        self == Self::Eagerepo || self == Self::Revlog
    }

    pub fn is_revlog(self) -> bool {
        self == Self::Revlog
    }
}
