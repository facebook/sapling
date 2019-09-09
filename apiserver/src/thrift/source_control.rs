// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use faster_hex::hex_string;
use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use futures_util::try_join;
use mononoke_api::{
    ChangesetContext, ChangesetId, ChangesetSpecifier, CoreContext, HgChangesetId, Mononoke,
    MononokeError, RepoContext,
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

impl ScubaInfoProvider for thrift::CommitSpecifier {
    fn scuba_reponame(&self) -> Option<String> {
        self.repo.scuba_reponame()
    }
    fn scuba_commit(&self) -> Option<String> {
        Some(commit_id_to_string(&self.id))
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

/// Returns the commit identity scheme of a commit ID.
fn commit_id_scheme(id: &thrift::CommitId) -> thrift::CommitIdentityScheme {
    match id {
        thrift::CommitId::bonsai(_) => thrift::CommitIdentityScheme::BONSAI,
        thrift::CommitId::hg(_) => thrift::CommitIdentityScheme::HG,
        thrift::CommitId::git(_) => thrift::CommitIdentityScheme::GIT,
        thrift::CommitId::global_rev(_) => thrift::CommitIdentityScheme::GLOBAL_REV,
        thrift::CommitId::UnknownField(t) => (*t).into(),
    }
}

/// Convert a `thrift::CommitId` to a string for display. This would normally
/// be implemented as `Display for thrift::CommitId`, but it is defined in
/// the generated crate.
fn commit_id_to_string(id: &thrift::CommitId) -> String {
    match id {
        thrift::CommitId::bonsai(id) => hex_string(&id).expect("hex_string should never fail"),
        thrift::CommitId::hg(id) => hex_string(&id).expect("hex_string should never fail"),
        thrift::CommitId::git(id) => hex_string(&id).expect("hex_string should never fail"),
        thrift::CommitId::global_rev(rev) => rev.to_string(),
        thrift::CommitId::UnknownField(t) => format!("unknown id type ({})", t),
    }
}

/// Convert a `thrift::CommitId` into a `mononoke_api::ChangesetSpecifier`.  This would
/// normally be implemented as `From<thrift::CommitId> for ChangesetSpecifier`, but it is
/// defined in the generated crate.
fn commit_id_to_changeset_specifier(
    commit: &thrift::CommitId,
) -> Result<ChangesetSpecifier, thrift::RequestError> {
    match commit {
        thrift::CommitId::bonsai(id) => {
            let cs_id = ChangesetId::from_bytes(&id).map_err(|e| {
                errors::invalid_request(format!(
                    "invalid commit id (scheme={} {}): {}",
                    commit_id_scheme(commit),
                    commit_id_to_string(commit),
                    e.to_string()
                ))
            })?;
            Ok(ChangesetSpecifier::Bonsai(cs_id))
        }
        thrift::CommitId::hg(id) => {
            let hg_cs_id = HgChangesetId::from_bytes(&id).map_err(|e| {
                errors::invalid_request(format!(
                    "invalid commit id (scheme={} {}): {}",
                    commit_id_scheme(commit),
                    commit_id_to_string(commit),
                    e.to_string()
                ))
            })?;
            Ok(ChangesetSpecifier::Hg(hg_cs_id))
        }
        _ => Err(errors::invalid_request(format!(
            "unsupported commit identity scheme ({})",
            commit_id_scheme(commit)
        ))),
    }
}

mod errors {
    use super::thrift;
    use mononoke_api::ChangesetSpecifier;

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

    pub(super) fn commit_not_found(commit: &ChangesetSpecifier) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::COMMIT_NOT_FOUND,
            reason: format!("commit not found ({})", commit),
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

    /// Look up commit.
    async fn commit_lookup(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self
            .mononoke
            .repo(ctx, &commit.repo.name)?
            .ok_or_else(|| errors::repo_not_found(&commit.repo.name))?;
        match repo
            .changeset(commit_id_to_changeset_specifier(&commit.id)?)
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
            }),
        }
    }

    /// Get commit info.
    async fn commit_info(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, service::CommitInfoExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self
            .mononoke
            .repo(ctx, &commit.repo.name)?
            .ok_or_else(|| errors::repo_not_found(&commit.repo.name))?;

        let changeset_specifier = commit_id_to_changeset_specifier(&commit.id)?;
        match repo.changeset(changeset_specifier).await? {
            Some(changeset) => {
                async fn map_parent_identities(
                    repo: &RepoContext,
                    changeset: &ChangesetContext,
                    identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
                ) -> Result<
                    Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
                    MononokeError,
                > {
                    let parents = changeset.parents().await?;
                    let parent_id_mapping =
                        map_commit_identities(&repo, parents.clone(), identity_schemes).await?;
                    Ok(parents
                        .iter()
                        .map(|parent_id| {
                            parent_id_mapping
                                .get(parent_id)
                                .map(Clone::clone)
                                .unwrap_or_else(BTreeMap::new)
                        })
                        .collect())
                }

                let (ids, message, date, author, parents, extra) = try_join!(
                    map_commit_identity(&changeset, &params.identity_schemes),
                    changeset.message(),
                    changeset.author_date(),
                    changeset.author(),
                    map_parent_identities(&repo, &changeset, &params.identity_schemes),
                    changeset.extras(),
                )?;
                Ok(thrift::CommitInfo {
                    ids,
                    message,
                    date: date.timestamp(),
                    author,
                    parents,
                    extra: extra.into_iter().collect(),
                })
            }
            None => Err(errors::commit_not_found(&changeset_specifier).into()),
        }
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    async fn commit_is_ancestor_of(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, service::CommitIsAncestorOfExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self
            .mononoke
            .repo(ctx, &commit.repo.name)?
            .ok_or_else(|| errors::repo_not_found(&commit.repo.name))?;
        let changeset_specifier = commit_id_to_changeset_specifier(&commit.id)?;
        let other_changeset_specifier = commit_id_to_changeset_specifier(&params.other_commit_id)?;
        let (changeset, other_changeset_id) = try_join!(
            repo.changeset(changeset_specifier),
            repo.resolve_specifier(other_changeset_specifier),
        )?;
        let changeset = changeset.ok_or_else(|| errors::commit_not_found(&changeset_specifier))?;
        let other_changeset_id = other_changeset_id
            .ok_or_else(|| errors::commit_not_found(&other_changeset_specifier))?;
        let is_ancestor_of = changeset.is_ancestor_of(other_changeset_id).await?;
        Ok(is_ancestor_of)
    }
}
