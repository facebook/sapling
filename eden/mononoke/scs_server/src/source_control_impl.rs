/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use context::generate_session_id;
use fbinit::FacebookInit;
use futures_stats::{FutureStats, TimedFutureExt};
use identity::Identity;
use mononoke_api::{
    ChangesetContext, ChangesetSpecifier, CoreContext, FileContext, FileId, Mononoke, RepoContext,
    SessionContainer, TreeContext, TreeId,
};
use mononoke_types::hash::{Sha1, Sha256};
use permission_checker::{MononokeIdentity, MononokeIdentitySet};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt, ScubaValue};
use slog::Logger;
use source_control as thrift;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use srserver::RequestContext;
use stats::prelude::*;
use time_ext::DurationExt;

use crate::errors;
use crate::from_request::FromRequest;
use crate::params::AddScubaParams;
use crate::specifiers::SpecifierExt;

const SCS_IDENTITY: &str = "scm_service_identity";

define_stats! {
    prefix = "mononoke.scs_server";
    total_request_start: timeseries(Rate, Sum),
    total_request_success: timeseries(Rate, Sum),
    total_request_internal_failure: timeseries(Rate, Sum),
    total_request_invalid: timeseries(Rate, Sum),

    // permille is used in canaries, because canaries do not allow for tracking formulas
    total_request_internal_failure_permille: timeseries(Average),
    total_request_invalid_permille: timeseries(Average),

    // Duration per method
    method_completion_time_ms: dynamic_histogram("method.{}.completion_time_ms", (method: String); 10, 0, 1_000, Average, Sum, Count; P 5; P 50 ; P 90),
}

#[derive(Clone)]
pub(crate) struct SourceControlServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) mononoke: Arc<Mononoke>,
    pub(crate) logger: Logger,
    pub(crate) scuba_builder: ScubaSampleBuilder,
    pub(crate) service_identity: Identity,
}

pub(crate) struct SourceControlServiceThriftImpl(SourceControlServiceImpl);

impl SourceControlServiceImpl {
    pub fn new(
        fb: FacebookInit,
        mononoke: Arc<Mononoke>,
        logger: Logger,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        Self {
            fb,
            mononoke,
            logger,
            scuba_builder,
            service_identity: Identity::with_service(SCS_IDENTITY),
        }
    }

    pub(crate) fn thrift_server(&self) -> SourceControlServiceThriftImpl {
        SourceControlServiceThriftImpl(self.clone())
    }

    pub(crate) fn create_ctx(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        specifier: Option<&dyn SpecifierExt>,
        params: &dyn AddScubaParams,
    ) -> Result<CoreContext, errors::ServiceError> {
        let mut scuba = self.scuba_builder.clone();
        scuba.add_common_server_data();
        scuba.add("type", "thrift");
        scuba.add("method", name);
        if let Some(specifier) = specifier {
            if let Some(reponame) = specifier.scuba_reponame() {
                scuba.add("reponame", reponame);
            }
            if let Some(commit) = specifier.scuba_commit() {
                scuba.add("commit", commit);
            }
            if let Some(path) = specifier.scuba_path() {
                scuba.add("path", path);
            }
        }
        params.add_scuba_params(&mut scuba);
        let session_id = generate_session_id();
        scuba.add("session_uuid", session_id.to_string());

        const CLIENT_HEADERS: &[&str] = &[
            "client_id",
            "client_type",
            "client_correlator",
            "proxy_client_id",
        ];
        for &header in CLIENT_HEADERS.iter() {
            let value = req_ctxt.header(header).map_err(errors::internal_error)?;
            if let Some(value) = value {
                scuba.add(header, value);
            }
        }

        let identities = req_ctxt
            .identities_for_service(&self.service_identity)
            .map_err(errors::internal_error)?;
        let identities = identities.entries();
        scuba.add(
            "identities",
            identities
                .iter()
                .map(|id| id.to_string())
                .collect::<ScubaValue>(),
        );

        let identities: MononokeIdentitySet = identities
            .into_iter()
            .filter_map(|id| MononokeIdentity::try_from_identity_ref(id).ok())
            .collect();
        let session = SessionContainer::builder(self.fb)
            .session_id(session_id)
            .identities(identities)
            .build();

        let ctx = session.new_context(self.logger.clone(), scuba);

        Ok(ctx)
    }

    /// Get the repo specified by a `thrift::RepoSpecifier`.
    pub(crate) async fn repo(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
    ) -> Result<RepoContext, errors::ServiceError> {
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)
            .await?
            .ok_or_else(|| errors::repo_not_found(repo.description()))?;
        Ok(repo)
    }

    /// Get the repo and changeset specified by a `thrift::CommitSpecifier`.
    pub(crate) async fn repo_changeset(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
    ) -> Result<(RepoContext, ChangesetContext), errors::ServiceError> {
        let repo = self.repo(ctx, &commit.repo).await?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| errors::commit_not_found(commit.description()))?;
        Ok((repo, changeset))
    }

    /// Get the repo and tree specified by a `thrift::TreeSpecifier`.
    ///
    /// Returns `None` if the tree is specified by commit path and that path
    /// is not a directory in that commit.
    pub(crate) async fn repo_tree(
        &self,
        ctx: CoreContext,
        tree: &thrift::TreeSpecifier,
    ) -> Result<(RepoContext, Option<TreeContext>), errors::ServiceError> {
        let (repo, tree) = match tree {
            thrift::TreeSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.tree().await?)
            }
            thrift::TreeSpecifier::by_id(tree_id) => {
                let repo = self.repo(ctx, &tree_id.repo).await?;
                let tree_id = TreeId::from_request(&tree_id.id)?;
                let tree = repo
                    .tree(tree_id)
                    .await?
                    .ok_or_else(|| errors::tree_not_found(tree.description()))?;
                (repo, Some(tree))
            }
            thrift::TreeSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "tree specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, tree))
    }

    /// Get the repo and file specified by a `thrift::FileSpecifier`.
    ///
    /// Returns `None` if the file is specified by commit path, and that path
    /// is not a file in that commit.
    pub(crate) async fn repo_file(
        &self,
        ctx: CoreContext,
        file: &thrift::FileSpecifier,
    ) -> Result<(RepoContext, Option<FileContext>), errors::ServiceError> {
        let (repo, file) = match file {
            thrift::FileSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.file().await?)
            }
            thrift::FileSpecifier::by_id(file_id) => {
                let repo = self.repo(ctx, &file_id.repo).await?;
                let file_id = FileId::from_request(&file_id.id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha1_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo).await?;
                let file_sha1 = Sha1::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha1(file_sha1)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha256_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo).await?;
                let file_sha256 = Sha256::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha256(file_sha256)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "file specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, file))
    }
}

fn log_result<T>(ctx: CoreContext, stats: &FutureStats, result: &Result<T, errors::ServiceError>) {
    let mut success = 0;
    let mut internal_failure = 0;
    let mut invalid_request = 0;

    let (status, error) = match result {
        Ok(_) => {
            success = 1;
            ("SUCCESS", None)
        }
        Err(errors::ServiceError::Request(e)) => {
            invalid_request = 1;
            ("REQUEST_ERROR", Some(format!("{:?}", e)))
        }
        Err(errors::ServiceError::Internal(e)) => {
            internal_failure = 1;
            ("INTERNAL_ERROR", Some(format!("{:?}", e)))
        }
    };

    STATS::total_request_success.add_value(success);
    STATS::total_request_internal_failure.add_value(internal_failure);
    STATS::total_request_invalid.add_value(invalid_request);
    STATS::total_request_internal_failure_permille.add_value(internal_failure * 1000);
    STATS::total_request_invalid_permille.add_value(invalid_request * 1000);

    let mut scuba = ctx.scuba().clone();

    ctx.perf_counters().insert_perf_counters(&mut scuba);

    scuba.add_future_stats(stats);
    scuba.add("status", status);
    if let Some(error) = error {
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg("Request complete", None);
}

// Define a macro to construct a CoreContext based on the thrift parameters.
macro_rules! create_ctx {
    ( $service_impl:expr, $method_name:ident, $req_ctxt:ident, $params_name:ident ) => {
        $service_impl.create_ctx(stringify!($method_name), $req_ctxt, None, &$params_name)
    };

    ( $service_impl:expr, $method_name:ident, $req_ctxt:ident, $obj_name:ident, $params_name:ident ) => {
        $service_impl.create_ctx(
            stringify!($method_name),
            $req_ctxt,
            Some(&$obj_name),
            &$params_name,
        )
    };
}

// Define a macro that generates a non-async wrapper that delegates to the
// async implementation of the method.
//
// The implementations of the methods can be found in the `methods` module.
macro_rules! impl_thrift_methods {
    ( $( async fn $method_name:ident($( $param_name:ident : $param_type:ty, )*) -> Result<$ok_type:ty, $err_type:ty>; )* ) => {
        $(
            fn $method_name<'implementation, 'req_ctxt, 'async_trait>(
                &'implementation self,
                req_ctxt: &'req_ctxt RequestContext,
                $( $param_name: $param_type ),*
            ) -> Pin<Box<dyn Future<Output = Result<$ok_type, $err_type>> + Send + 'async_trait>>
            where
                'implementation: 'async_trait,
                'req_ctxt: 'async_trait,
                Self: Sync + 'async_trait,
            {
                let handler = async move {
                    let ctx = create_ctx!(self.0, $method_name, req_ctxt, $( $param_name ),*)?;
                    ctx.scuba().clone().log_with_msg("Request start", None);
                    STATS::total_request_start.add_value(1);
                    let (stats, res) = (self.0)
                        .$method_name(ctx.clone(), $( $param_name ),* )
                        .timed()
                        .await;
                    log_result(ctx, &stats, &res);
                    let method = stringify!($method_name).to_string();
                    STATS::method_completion_time_ms.add_value(stats.completion_time.as_millis_unchecked() as i64, (method,));
                    Ok(res?)
                };
                Box::pin(handler)
            }
        )*
    }
}

impl SourceControlService for SourceControlServiceThriftImpl {
    type RequestContext = RequestContext;

    impl_thrift_methods! {
        async fn list_repos(
            params: thrift::ListReposParams,
        ) -> Result<Vec<thrift::Repo>, service::ListReposExn>;

        async fn repo_resolve_bookmark(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoResolveBookmarkParams,
        ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn>;

        async fn repo_resolve_commit_prefix(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoResolveCommitPrefixParams,
        ) -> Result<thrift::RepoResolveCommitPrefixResponse, service::RepoResolveCommitPrefixExn>;

        async fn repo_list_bookmarks(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoListBookmarksParams,
        ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn>;

        async fn commit_common_base_with(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitCommonBaseWithParams,
        ) -> Result<thrift::CommitLookupResponse, service::CommitCommonBaseWithExn>;

        async fn commit_lookup(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitLookupParams,
        ) -> Result<thrift::CommitLookupResponse, service::CommitLookupExn>;

        async fn commit_file_diffs(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitFileDiffsParams,
        ) -> Result<thrift::CommitFileDiffsResponse, service::CommitFileDiffsExn>;

        async fn commit_info(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitInfoParams,
        ) -> Result<thrift::CommitInfo, service::CommitInfoExn>;

        async fn commit_is_ancestor_of(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitIsAncestorOfParams,
        ) -> Result<bool, service::CommitIsAncestorOfExn>;

        async fn commit_compare(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitCompareParams,
        ) -> Result<thrift::CommitCompareResponse, service::CommitCompareExn>;

        async fn commit_find_files(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitFindFilesParams,
        ) -> Result<thrift::CommitFindFilesResponse, service::CommitFindFilesExn>;

        async fn commit_history(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitHistoryParams,
        ) -> Result<thrift::CommitHistoryResponse, service::CommitHistoryExn>;

        async fn commit_lookup_xrepo(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitLookupXRepoParams,
        ) -> Result<thrift::CommitLookupResponse, service::CommitLookupXrepoExn>;

        async fn commit_path_info(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathInfoParams,
        ) -> Result<thrift::CommitPathInfoResponse, service::CommitPathInfoExn>;

        async fn commit_path_blame(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathBlameParams,
        ) -> Result<thrift::CommitPathBlameResponse, service::CommitPathBlameExn>;

        async fn commit_path_history(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathHistoryParams,
        ) -> Result<thrift::CommitPathHistoryResponse, service::CommitPathHistoryExn>;

        async fn tree_list(
            tree: thrift::TreeSpecifier,
            params: thrift::TreeListParams,
        ) -> Result<thrift::TreeListResponse, service::TreeListExn>;

        async fn file_exists(
            file: thrift::FileSpecifier,
            _params: thrift::FileExistsParams,
        ) -> Result<bool, service::FileExistsExn>;

        async fn file_info(
            file: thrift::FileSpecifier,
            _params: thrift::FileInfoParams,
        ) -> Result<thrift::FileInfo, service::FileInfoExn>;

        async fn file_content_chunk(
            file: thrift::FileSpecifier,
            params: thrift::FileContentChunkParams,
        ) -> Result<thrift::FileChunk, service::FileContentChunkExn>;

        async fn repo_create_commit(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoCreateCommitParams,
        ) -> Result<thrift::RepoCreateCommitResponse, service::RepoCreateCommitExn>;

        async fn repo_stack_info(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoStackInfoParams,
        ) -> Result<thrift::RepoStackInfoResponse, service::RepoStackInfoExn>;

        async fn repo_move_bookmark(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoMoveBookmarkParams,
        ) -> Result<thrift::RepoMoveBookmarkResponse, service::RepoMoveBookmarkExn>;
    }
}
