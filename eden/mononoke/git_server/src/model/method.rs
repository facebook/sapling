/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

/// Enum representing the method (and the corresponding handler) supported by the Git Server
#[derive(Copy, Clone)]
pub enum GitMethod {
    /// Method responsible for performing incremental pull of the repo
    Pull,
    /// Method responsible for performing full clone of the repo
    Clone,
    /// Method responsible for listing all known refs to the client
    LsRefs,
}

impl GitMethod {
    /// Returns true if the method is read-only
    pub fn is_read_only(&self) -> bool {
        // Since the current version of Mononoke Git server does not support pushes, all
        // its methods are read-only
        true
    }
}

impl fmt::Display for GitMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Pull => "pull",
            Self::Clone => "clone",
            Self::LsRefs => "ls-refs",
        };
        write!(f, "{}", name)
    }
}
