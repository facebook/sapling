/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
