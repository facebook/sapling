/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::{self};

use gotham_derive::StateData;

use crate::command::Command;

/// Enum representing the method (and the corresponding handler) supported by the Git Server
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GitMethod {
    /// Method responsible for advertising the server capabilities for git-upload-pack to the client
    AdvertiseRead,
    /// Method responsible for advertising the server capabilities for git-receive-pack to the client
    AdvertiseWrite,
    /// Method responsible for performing incremental pull of the repo
    Pull,
    /// Method responsible for performing full clone of the repo
    Clone,
    /// Method responsible for listing all known refs to the client
    LsRefs,
    /// Method responsible for pushing changes to the repo
    Push,
}

impl GitMethod {
    /// Returns true if the method is read-only
    pub fn is_read_only(&self) -> bool {
        *self != Self::Push
    }
}

impl fmt::Display for GitMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Pull => "pull",
            Self::Clone => "clone",
            Self::LsRefs => "ls-refs",
            Self::AdvertiseRead => "advertise-read",
            Self::AdvertiseWrite => "advertise-write",
            Self::Push => "push",
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

    pub fn standard(repo: String, method: GitMethod) -> Self {
        Self {
            repo,
            method,
            variants: vec![GitMethodVariant::Standard],
        }
    }

    pub fn from_command(command: &Command, repo: String) -> Self {
        let (method, variants) = match command {
            Command::LsRefs(_) => (GitMethod::LsRefs, vec![GitMethodVariant::Standard]),
            Command::Fetch(ref fetch_args) => {
                let method = if fetch_args.haves.is_empty() && fetch_args.done {
                    GitMethod::Clone
                } else {
                    GitMethod::Pull
                };
                let mut variants = vec![];
                if fetch_args.is_shallow() {
                    variants.push(GitMethodVariant::Shallow);
                }
                if fetch_args.is_filter() {
                    variants.push(GitMethodVariant::Filter);
                }
                if variants.is_empty() {
                    variants.push(GitMethodVariant::Standard);
                }
                (method, variants)
            }
            Command::Push(ref push_args) => {
                if push_args.is_shallow() {
                    (GitMethod::Push, vec![GitMethodVariant::Shallow])
                } else {
                    (GitMethod::Push, vec![GitMethodVariant::Standard])
                }
            }
        };
        GitMethodInfo {
            method,
            variants,
            repo,
        }
    }
}

/// Struct representing the validation errors returned by Mononoke Git server when
/// processing a push request
#[derive(Clone, StateData, Default)]
pub struct PushValidationErrors {
    pub ref_with_errors: HashMap<String, String>,
}

impl PushValidationErrors {
    pub fn add_error(&mut self, ref_name: String, error: String) {
        self.ref_with_errors.insert(ref_name, error);
    }

    pub fn is_empty(&self) -> bool {
        self.ref_with_errors.is_empty()
    }
}

impl Display for PushValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (ref_name, error) in &self.ref_with_errors {
            write!(f, "{} => {}\n", ref_name, error)?;
        }
        Ok(())
    }
}
