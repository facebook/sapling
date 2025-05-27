/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
    #[serde(transparent)]
    pub struct FetchMode: u16 {
        /// The fetch may hit remote servers.
        const REMOTE = 1;

        /// The fetch may hit local repo/cache storage.
        const LOCAL = 2;

        /// The fetch may request extra data from remote server.
        const PREFETCH = 4;

        /// Caller doesn't care about the result data - ok to skip some work.
        const IGNORE_RESULT = 8;
    }
}

#[allow(non_upper_case_globals)]
impl FetchMode {
    /// The fetch may hit local caches and/or remote servers.
    pub const AllowRemote: Self = Self::LOCAL.union(Self::REMOTE);

    /// The fetch is limited to RAM and disk.
    pub const LocalOnly: Self = Self::LOCAL;

    /// The fetch is only hits remote servers.
    pub const RemoteOnly: Self = Self::REMOTE;

    pub fn is_local(self) -> bool {
        self == Self::LocalOnly
    }

    pub fn from_local(local: bool) -> Self {
        if local {
            Self::LocalOnly
        } else {
            Self::AllowRemote
        }
    }

    pub fn is_remote(self) -> bool {
        self == Self::RemoteOnly
    }

    pub fn from_remote(remote: bool) -> Self {
        if remote {
            Self::RemoteOnly
        } else {
            Self::AllowRemote
        }
    }

    pub fn ignore_result(self) -> bool {
        self.contains(Self::IGNORE_RESULT)
    }
}
