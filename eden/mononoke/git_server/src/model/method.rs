/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use gotham_derive::StateData;

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

/// Enum representing the variant of the methods supported by the Git Server
#[derive(Copy, Clone)]
pub enum GitMethodVariant {
    /// Git method variant for when the client specified filter criteria in
    /// the request
    Filter,
    /// Git method variant for when the client specified one of the shallow
    /// arguments in the request
    Shallow,
    /// Git method variant for when the client utilized the standard workflow
    Standard,
}

impl fmt::Display for GitMethodVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Filter => "filter",
            Self::Shallow => "shallow",
            Self::Standard => "standard",
        };
        write!(f, "{}", name)
    }
}

/// Struct providing info about the method and its variants supported
/// by the Git Server in the context of a given repo
#[derive(Clone, StateData)]
pub struct GitMethodInfo {
    pub repo: String,
    pub method: GitMethod,
    pub variants: Vec<GitMethodVariant>,
}

impl GitMethodInfo {
    pub fn variants_to_string(&self) -> String {
        self.variants
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join(",")
    }
}
