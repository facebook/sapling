/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod context;
mod method;
mod push_data;
mod pushvars;

pub use context::GitServerContext;
pub use context::RepositoryRequestContext;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
pub use method::BundleUriOutcome;
pub use method::GitMethod;
pub use method::GitMethodInfo;
pub use method::PushValidationErrors;
pub use push_data::PushData;
pub use pushvars::Pushvars;
use serde::Deserialize;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct ServiceType {
    pub service: Service,
}

#[derive(
    strum::EnumString,
    strum::Display,
    Debug,
    Deserialize,
    Clone,
    Copy,
    StateData
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab_case")]
/// The type of service that is being served by the server
pub enum Service {
    /// The service that handles git upload-pack requests
    GitUploadPack,
    /// The service that handles git receive-pack requests
    GitReceivePack,
}

/// The type of responses that can be returned by the server.
#[derive(strum::EnumString, strum::Display, Debug, Deserialize, StateData)]
#[strum(serialize_all = "kebab_case")]
pub enum ResponseType {
    /// The response corresponding to capability advertisement from the git server
    Advertisement,
    /// The response corresponding to a git upload-pack request
    Result,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct RepositoryParams {
    /// The name of the repository. It is a vec of strings because repo with `/` in their
    /// names are captured as multiple segments in the path.
    repository: Vec<String>,
}

impl RepositoryParams {
    pub fn repo_name(&self) -> String {
        let repo = self.repository.join("/");
        match repo.strip_suffix(".git") {
            Some(repo) => repo.to_string(),
            None => repo,
        }
    }
}
