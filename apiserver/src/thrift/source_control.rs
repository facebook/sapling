// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use mononoke_api::{ChangesetContext, CoreContext, Mononoke, MononokeError};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use source_control::types as thrift;
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

trait ScubaInfoProvider {
    fn scuba_reponame(&self) -> Option<String> {
        None
    }
    fn scuba_commit(&self) -> Option<String> {
        None
    }
    fn scuba_path(&self) -> Option<String> {
        None
    }
}

impl ScubaInfoProvider for thrift::RepoSpecifier {
    fn scuba_reponame(&self) -> Option<String> {
        Some(self.name.clone())
    }
}

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(mononoke: Arc<Mononoke>, logger: Logger, scuba_builder: ScubaSampleBuilder) -> Self {
        Self {
            mononoke,
            logger,
            scuba_builder,
        }
    }

    fn create_ctx(&self, scuba_info_provider: Option<&dyn ScubaInfoProvider>) -> CoreContext {
        let mut scuba = self.scuba_builder.clone();
        scuba.add_common_server_data().add("type", "thrift");
        if let Some(scuba_info_provider) = scuba_info_provider {
            if let Some(reponame) = scuba_info_provider.scuba_reponame() {
                scuba.add("reponame", reponame);
            }
            if let Some(commit) = scuba_info_provider.scuba_commit() {
                scuba.add("commit", commit);
            }
            if let Some(path) = scuba_info_provider.scuba_path() {
                scuba.add("path", path);
            }
        }
        let uuid = Uuid::new_v4();
        scuba.add("session_uuid", uuid.to_string());
        CoreContext::new(
            uuid,
            self.logger.clone(),
            scuba,
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
            None,
        )
    }
}

/// Generate a mapping for a commit's identity into the requested identity
/// schemes.
async fn map_commit_identity(
    changeset_ctx: &ChangesetContext,
    schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>, MononokeError> {
    let mut ids = BTreeMap::new();
    ids.insert(
        thrift::CommitIdentityScheme::BONSAI,
        thrift::CommitId::bonsai(changeset_ctx.id().as_ref().into()),
    );
    if schemes.contains(&thrift::CommitIdentityScheme::HG) {
        if let Some(hg_cs_id) = changeset_ctx.hg_id().await? {
            ids.insert(
                thrift::CommitIdentityScheme::HG,
                thrift::CommitId::hg(hg_cs_id.as_ref().into()),
            );
        }
    }
    Ok(ids)
}

mod errors {
    use super::thrift;

    pub(super) fn repo_not_found(reponame: impl AsRef<str>) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
            reason: format!("repo not found ({})", reponame.as_ref()),
        }
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {
    async fn list_repos(
        &self,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, service::ListReposExn> {
        let _ctx = self.create_ctx(None);
        let rsp = self
            .mononoke
            .repo_names()
            .map(|repo_name| thrift::Repo {
                name: repo_name.to_string(),
            })
            .collect();
        Ok(rsp)
    }

    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    async fn repo_resolve_bookmark(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn> {
        let ctx = self.create_ctx(Some(&repo));
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)?
            .ok_or_else(|| errors::repo_not_found(&repo.name))?;
        match repo.resolve_bookmark(params.bookmark_name).await? {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::RepoResolveBookmarkResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::RepoResolveBookmarkResponse {
                exists: false,
                ids: None,
            }),
        }
    }
}
