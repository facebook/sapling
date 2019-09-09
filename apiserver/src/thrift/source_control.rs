// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use mononoke_api::{
    ChangesetContext, ChangesetId, CoreContext, Mononoke, MononokeError, RepoContext,
};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use source_control::types as thrift;
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

const MAX_LIMIT: i64 = 1000;

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

/// Generate mappings for multiple commits' identities into the requested
/// identity schemes.
async fn map_commit_identities(
    repo_ctx: &RepoContext,
    ids: Vec<ChangesetId>,
    schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<
    BTreeMap<ChangesetId, BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
    MononokeError,
> {
    let mut result = BTreeMap::new();
    for id in ids.iter() {
        let mut idmap = BTreeMap::new();
        idmap.insert(
            thrift::CommitIdentityScheme::BONSAI,
            thrift::CommitId::bonsai(id.as_ref().into()),
        );
        result.insert(*id, idmap);
    }
    if schemes.contains(&thrift::CommitIdentityScheme::HG) {
        for (cs_id, hg_cs_id) in repo_ctx.changeset_hg_ids(ids).await?.into_iter() {
            result.entry(cs_id).or_insert_with(BTreeMap::new).insert(
                thrift::CommitIdentityScheme::HG,
                thrift::CommitId::hg(hg_cs_id.as_ref().into()),
            );
        }
    }
    Ok(result)
}

mod errors {
    use super::thrift;

    pub(super) fn invalid_request(reason: impl ToString) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::INVALID_REQUEST,
            reason: reason.to_string(),
        }
    }

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

    /// List bookmarks.
    async fn repo_list_bookmarks(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn> {
        let ctx = self.create_ctx(Some(&repo));
        let limit = match params.limit {
            0 => None,
            limit @ 1...MAX_LIMIT => Some(limit as u64),
            limit => {
                return Err(errors::invalid_request(format!(
                    "limit ({}) out of range (0..{})",
                    limit, MAX_LIMIT,
                ))
                .into())
            }
        };
        let prefix = if !params.bookmark_prefix.is_empty() {
            Some(params.bookmark_prefix)
        } else {
            None
        };
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)?
            .ok_or_else(|| errors::repo_not_found(&repo.name))?;
        let bookmarks = repo
            .list_bookmarks(params.include_scratch, prefix, limit)
            .collect()
            .compat()
            .await?;
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                Some(ids) => (name, ids.clone()),
                None => (name, BTreeMap::new()),
            })
            .collect();
        Ok(thrift::RepoListBookmarksResponse { bookmarks })
    }
}
