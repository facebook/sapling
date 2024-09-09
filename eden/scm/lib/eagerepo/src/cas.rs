/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cas_client::CasClient;
use configmodel::Config;
use configmodel::ConfigExt;
use repourl::RepoUrl;

use crate::EagerRepo;

/// Optionally build `CasClient` from config.
///
/// If the config does not specify eagerepo-based remote, return `Ok(None)`.
pub fn cas_client_from_config(config: &dyn Config) -> anyhow::Result<Option<Arc<dyn CasClient>>> {
    if let Ok(url) = config.must_get::<RepoUrl>("paths", "default") {
        if let Some(path) = EagerRepo::url_to_dir(&url) {
            tracing::debug!(target: "cas", "creating eager remote client");
            let repo = EagerRepo::open(&path)?;
            return Ok(Some(Arc::new(repo.store) as Arc<dyn CasClient>));
        }
    }
    Ok(None)
}
