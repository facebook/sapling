/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;

#[derive(Debug, Copy, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchMode {
    /// The fetch may hit remote servers.
    AllowRemote,
    /// The fetch is limited to RAM and disk.
    LocalOnly,
    /// The fetch is only hits remote servers.
    RemoteOnly,
}

impl FetchMode {
    pub fn is_local(self) -> bool {
        matches!(self, FetchMode::LocalOnly)
    }

    pub fn from_local(local: bool) -> Self {
        if local {
            Self::LocalOnly
        } else {
            Self::AllowRemote
        }
    }

    pub fn is_remote(self) -> bool {
        matches!(self, FetchMode::RemoteOnly)
    }

    pub fn from_remote(remote: bool) -> Self {
        if remote {
            Self::RemoteOnly
        } else {
            Self::AllowRemote
        }
    }
}
