/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod context;

pub use context::GitServerContext;
#[allow(unused_imports)]
pub use context::RepositoryRequestContext;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use serde::Deserialize;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct ServiceType {
    pub service: String,
}

#[allow(dead_code)]
impl ServiceType {
    pub fn new(service: String) -> Self {
        Self { service }
    }
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct ResponseType {
    pub response: String,
}

#[allow(dead_code)]
impl ResponseType {
    pub fn new(response: String) -> Self {
        Self { response }
    }
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct RepositoryParams {
    /// The name of the repository. It is a vec of strings because repo with `/` in their
    /// names are captured as multiple segments in the path.
    repository: Vec<String>,
}

#[allow(dead_code)]
impl RepositoryParams {
    pub fn repo_name(&self) -> String {
        let repo = self.repository.join("/");
        match repo.strip_suffix(".git") {
            Some(repo) => repo.to_string(),
            None => repo,
        }
    }
}
