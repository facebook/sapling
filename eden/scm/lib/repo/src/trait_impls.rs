/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;

use edenapi::EdenApi;
use storemodel::StoreInfo;

use crate::repo::Repo;

impl StoreInfo for Repo {
    fn has_requirement(&self, requirement: &str) -> bool {
        // For storage we only check store_requirements.
        // "remotefilelog" should be but predates store requirements.
        self.store_requirements.contains(requirement)
            || (requirement == "remotefilelog" && self.requirements.contains(requirement))
    }

    fn config(&self) -> &dyn configmodel::Config {
        Repo::config(self)
    }

    fn store_path(&self) -> &Path {
        &self.store_path
    }

    fn remote_peer(&self) -> anyhow::Result<Option<Arc<dyn EdenApi>>> {
        Ok(self.optional_eden_api()?)
    }
}
