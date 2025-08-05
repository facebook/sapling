/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use metaconfig_types::RepoClientKnobs;
use mononoke_api::Mononoke;
use mononoke_api::Repo;
use repo_client::PushRedirectorArgs;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

use crate::errors::ErrorKind;

#[derive(Clone)]
pub struct RepoHandler {
    pub logger: Logger,
    pub scuba: MononokeScubaSampleBuilder,
    pub repo: Arc<Repo>,
    pub maybe_push_redirector_args: Option<PushRedirectorArgs<Repo>>,
    pub repo_client_knobs: RepoClientKnobs,
}

pub fn repo_handler(mononoke: Arc<Mononoke<Repo>>, repo_name: &str) -> anyhow::Result<RepoHandler> {
    let source_repo = mononoke.raw_repo(repo_name).ok_or_else(|| {
        anyhow!(
            "Requested repo {} is not being served by this server",
            &repo_name
        )
    })?;
    let base = source_repo.repo_handler_base.clone();
    let maybe_push_redirector_args = match &base.maybe_push_redirector_base {
        Some(push_redirector_base) => {
            let large_repo_id = push_redirector_base.common_commit_sync_config.large_repo_id;
            let target_repo = mononoke
                .raw_repo_by_id(large_repo_id.id())
                .ok_or(ErrorKind::LargeRepoNotFound(large_repo_id))?;
            Some(PushRedirectorArgs::new(
                target_repo,
                Arc::clone(&source_repo),
                push_redirector_base.synced_commit_mapping.clone(),
                Arc::clone(&push_redirector_base.target_repo_dbs),
            ))
        }
        None => None,
    };

    Ok(RepoHandler {
        logger: base.logger.clone(),
        scuba: base.scuba.clone(),
        repo_client_knobs: base.repo_client_knobs.clone(),
        repo: source_repo,
        maybe_push_redirector_args,
    })
}
