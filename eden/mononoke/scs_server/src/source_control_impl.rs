/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use connection_security_checker::ConnectionSecurityChecker;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::try_join;
use futures::FutureExt;
use futures_ext::FbFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use identity::Identity;
use login_objects_thrift::EnvironmentType;
use maplit::hashset;
use megarepo_api::MegarepoApi;
use metaconfig_types::CommonConfig;
use metadata::Metadata;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CoreContext;
use mononoke_api::FileContext;
use mononoke_api::FileId;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_api::SessionContainer;
use mononoke_api::TreeContext;
use mononoke_api::TreeId;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use once_cell::sync::Lazy;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use repo_authorization::AuthorizationContext;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use slog::Logger;
use source_control as thrift;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use srserver::RequestContext;
use stats::prelude::*;
use time_ext::DurationExt;
use tunables::tunables;

use crate::commit_id::CommitIdExt;
use crate::errors;
use crate::errors::ServiceErrorResultExt;
use crate::errors::Status;
use crate::from_request::FromRequest;
use crate::scuba_params::AddScubaParams;
use crate::scuba_response::AddScubaResponse;
use crate::specifiers::SpecifierExt;

const FORWARDED_IDENTITIES_HEADER: &str = "scm_forwarded_identities";
const FORWARDED_CLIENT_IP_HEADER: &str = "scm_forwarded_client_ip";
const FORWARDED_CLIENT_DEBUG_HEADER: &str = "scm_forwarded_client_debug";

define_stats! {
    prefix = "mononoke.scs_server";
    total_request_start: timeseries(Rate, Sum),
    total_request_success: timeseries(Rate, Sum),
    total_request_internal_failure: timeseries(Rate, Sum),
    total_request_invalid: timeseries(Rate, Sum),
    total_request_cancelled: timeseries(Rate, Sum),

    // permille is used in canaries, because canaries do not allow for tracking formulas
    total_request_internal_failure_permille: timeseries(Average),
    total_request_invalid_permille: timeseries(Average),

    // Duration per method
    method_completion_time_ms: dynamic_histogram("method.{}.completion_time_ms", (method: String); 10, 0, 1_000, Average, Sum, Count; P 5; P 50 ; P 90),
}

static POPULAR_METHODS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| hashset! {"repo_list_hg_manifest"});

#[derive(Clone)]
pub(crate) struct SourceControlServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) mononoke: Arc<Mononoke>,
    pub(crate) megarepo_api: Arc<MegarepoApi>,
    pub(crate) logger: Logger,
    pub(crate) scuba_builder: MononokeScubaSampleBuilder,
    pub(crate) identity: Identity,
    pub(crate) scribe: Scribe,
    identity_proxy_checker: Arc<ConnectionSecurityChecker>,
}

pub(crate) struct SourceControlServiceThriftImpl(SourceControlServiceImpl);

impl SourceControlServiceImpl {
    pub fn new(
        fb: FacebookInit,
        mononoke: Arc<Mononoke>,
        megarepo_api: Arc<MegarepoApi>,
        logger: Logger,
        mut scuba_builder: MononokeScubaSampleBuilder,
        scribe: Scribe,
        identity_proxy_checker: ConnectionSecurityChecker,
        common_config: &CommonConfig,
    ) -> Self {
        scuba_builder.add_common_server_data();

        Self {
            fb,
            mononoke,
            megarepo_api,
            logger,
            scuba_builder,
            identity: Identity::new(
                common_config.internal_identity.id_type.as_str(),
                common_config.internal_identity.id_data.as_str(),
            ),
            scribe,
            identity_proxy_checker: Arc::new(identity_proxy_checker),
        }
    }

    pub(crate) fn thrift_server(&self) -> SourceControlServiceThriftImpl {
        SourceControlServiceThriftImpl(self.clone())
    }

    pub(crate) async fn create_ctx(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        specifier: Option<&dyn SpecifierExt>,
        params: &dyn AddScubaParams,
    ) -> Result<CoreContext, errors::ServiceError> {
        let session = self.create_session(req_ctxt).await?;
        let identities = session.metadata().identities();
        let mut scuba = self.create_scuba(name, req_ctxt, specifier, params, &identities)?;
        scuba.add("session_uuid", session.metadata().session_id().to_string());

        let ctx = session.new_context_with_scribe(self.logger.clone(), scuba, self.scribe.clone());
        Ok(ctx)
    }

    /// Create and configure a scuba sample builder for a request.
    fn create_scuba(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        specifier: Option<&dyn SpecifierExt>,
        params: &dyn AddScubaParams,
        identities: &MononokeIdentitySet,
    ) -> Result<MononokeScubaSampleBuilder, errors::ServiceError> {
        let mut scuba = self.scuba_builder.clone().with_seq("seq");
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

        let sampling_rate = core::num::NonZeroU64::new(if POPULAR_METHODS.contains(name) {
            tunables().get_scs_popular_methods_sampling_rate() as u64
        } else {
            tunables().get_scs_other_methods_sampling_rate() as u64
        });
        if let Some(sampling_rate) = sampling_rate {
            scuba.sampled(sampling_rate);
        } else {
            scuba.unsampled();
        }

        params.add_scuba_params(&mut scuba);

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

        scuba.add(
            "identities",
            identities
                .iter()
                .map(|id| id.to_string())
                .collect::<ScubaValue>(),
        );

        Ok(scuba)
    }

    async fn create_metadata(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<Metadata, errors::ServiceError> {
        let header = |h: &str| req_ctxt.header(h).map_err(errors::invalid_request);

        let tls_identities: MononokeIdentitySet = req_ctxt
            .identities()
            .map_err(errors::internal_error)?
            .entries()
            .into_iter()
            .map(MononokeIdentity::from_identity_ref)
            .collect();

        // Get any valid CAT identieies.
        let cats_identities: MononokeIdentitySet = req_ctxt
            .identities_cats(
                &self.identity,
                &[EnvironmentType::PROD, EnvironmentType::CORP],
            )
            .map_err(errors::internal_error)?
            .entries()
            .into_iter()
            .map(MononokeIdentity::from_identity_ref)
            .collect();

        let is_trusted = self
            .identity_proxy_checker
            .check_if_trusted(&tls_identities)
            .await
            .map_err(errors::invalid_request)?;

        if is_trusted {
            if let (Some(forwarded_identities), Some(forwarded_ip)) = (
                header(FORWARDED_IDENTITIES_HEADER)?,
                header(FORWARDED_CLIENT_IP_HEADER)?,
            ) {
                let mut header_identities: MononokeIdentitySet =
                    serde_json::from_str(forwarded_identities.as_str())
                        .map_err(errors::invalid_request)?;
                let client_ip = Some(
                    forwarded_ip
                        .parse::<IpAddr>()
                        .map_err(errors::invalid_request)?,
                );
                let client_debug = header(FORWARDED_CLIENT_DEBUG_HEADER)?.is_some();

                header_identities.extend(cats_identities.into_iter());
                let mut metadata =
                    Metadata::new(None, header_identities, client_debug, client_ip).await;

                metadata.add_original_identities(tls_identities);

                return Ok(metadata);
            }
        }

        Ok(Metadata::new(
            None,
            tls_identities.union(&cats_identities).cloned().collect(),
            false,
            None,
        )
        .await)
    }

    /// Create and configure the session container for a request.
    async fn create_session(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<SessionContainer, errors::ServiceError> {
        let metadata = self.create_metadata(req_ctxt).await?;
        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .blobstore_maybe_read_qps_limiter(tunables().get_scs_request_read_qps())
            .await
            .blobstore_maybe_write_qps_limiter(tunables().get_scs_request_write_qps())
            .await
            .build();
        Ok(session)
    }

    /// Get the repo specified by a `thrift::RepoSpecifier`.
    pub(crate) async fn repo(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
    ) -> Result<RepoContext, errors::ServiceError> {
        let authz = AuthorizationContext::new();
        self.repo_impl(ctx, repo, authz, |_| async { Ok(None) })
            .await
    }

    /// Get the repo specified by a `thrift::RepoSpecifier` for access by a
    /// named service.
    pub(crate) async fn repo_for_service(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
        service_name: Option<String>,
    ) -> Result<RepoContext, errors::ServiceError> {
        let authz = match service_name {
            Some(service_name) => AuthorizationContext::new_for_service_writes(service_name),
            None => AuthorizationContext::new(),
        };
        self.repo_impl(ctx, repo, authz, |_| async { Ok(None) })
            .await
    }

    async fn repo_impl<F, R>(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
        authz: AuthorizationContext,
        bubble_fetcher: F,
    ) -> Result<RepoContext, errors::ServiceError>
    where
        F: FnOnce(RepoEphemeralStore) -> R,
        R: Future<Output = anyhow::Result<Option<BubbleId>>>,
    {
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)
            .await?
            .ok_or_else(|| errors::repo_not_found(repo.description()))?
            .with_bubble(bubble_fetcher)
            .await?
            .with_authorization_context(authz)
            .build()
            .await?;
        Ok(repo)
    }

    fn bubble_fetcher_for_changeset(
        &self,
        specifier: ChangesetSpecifier,
    ) -> impl FnOnce(RepoEphemeralStore) -> BoxFuture<'static, anyhow::Result<Option<BubbleId>>>
    {
        move |ephemeral| async move { specifier.bubble_id(ephemeral).await }.boxed()
    }

    /// Get the repo and changeset specified by a `thrift::CommitSpecifier`.
    pub(crate) async fn repo_changeset(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
    ) -> Result<(RepoContext, ChangesetContext), errors::ServiceError> {
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let repo = self
            .repo_impl(
                ctx,
                &commit.repo,
                AuthorizationContext::new(),
                self.bubble_fetcher_for_changeset(changeset_specifier.clone()),
            )
            .await?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| errors::commit_not_found(commit.description()))?;
        Ok((repo, changeset))
    }

    /// Get the repo and pair of changesets specified by a `thrift::CommitSpecifier`
    /// and `thrift::CommitId` pair.
    pub(crate) async fn repo_changeset_pair(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
        other_commit: &thrift::CommitId,
    ) -> Result<(RepoContext, ChangesetContext, ChangesetContext), errors::ServiceError> {
        let changeset_specifier =
            ChangesetSpecifier::from_request(&commit.id).context("invalid target commit id")?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(other_commit)
            .context("invalid or missing other commit id")?;
        if other_changeset_specifier.in_bubble() {
            Err(errors::invalid_request(format!(
                "Can't compare against a snapshot: {}",
                other_changeset_specifier
            )))?
        }
        let repo = self
            .repo_impl(
                ctx,
                &commit.repo,
                AuthorizationContext::new(),
                self.bubble_fetcher_for_changeset(changeset_specifier.clone()),
            )
            .await?;
        let (changeset, other_changeset) = try_join!(
            async {
                Ok::<_, errors::ServiceError>(
                    repo.changeset(changeset_specifier)
                        .await
                        .context("failed to resolve target commit")?
                        .ok_or_else(|| errors::commit_not_found(commit.description()))?,
                )
            },
            async {
                Ok::<_, errors::ServiceError>(
                    repo.changeset(other_changeset_specifier)
                        .await
                        .context("failed to resolve other commit")?
                        .ok_or_else(|| {
                            errors::commit_not_found(format!(
                                "repo={} commit={}",
                                commit.repo.name,
                                other_commit.to_string()
                            ))
                        })?,
                )
            },
        )?;
        Ok((repo, changeset, other_changeset))
    }

    /// Get the changeset id specified by a `thrift::CommitId`.
    pub(crate) async fn changeset_id(
        &self,
        repo: &RepoContext,
        id: &thrift::CommitId,
    ) -> Result<ChangesetId, errors::ServiceError> {
        let changeset_specifier = ChangesetSpecifier::from_request(id)?;
        Ok(repo
            .resolve_specifier(changeset_specifier)
            .await?
            .ok_or_else(|| {
                errors::commit_not_found(format!("repo={} commit={}", repo.name(), id.to_string()))
            })?)
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
                let path = changeset.path_with_content(&commit_path.path)?;
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
                let path = changeset.path_with_content(&commit_path.path)?;
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

fn log_result<T: AddScubaResponse>(
    ctx: CoreContext,
    stats: &FutureStats,
    result: &Result<T, impl errors::LoggableError>,
) {
    let mut scuba = ctx.scuba().clone();

    let (status, error, invalid_request, internal_failure) = match result {
        Ok(response) => {
            response.add_scuba_response(&mut scuba);
            ("SUCCESS", None, 0, 0)
        }
        Err(err) => {
            let (status, desc) = err.status_and_description();
            match status {
                Status::RequestError => ("REQUEST_ERROR", Some(desc), 1, 0),
                Status::InternalError => ("INTERNAL_ERROR", Some(desc), 0, 1),
            }
        }
    };
    let success = if error.is_none() { 1 } else { 0 };

    STATS::total_request_success.add_value(success);
    STATS::total_request_internal_failure.add_value(internal_failure);
    STATS::total_request_invalid.add_value(invalid_request);
    STATS::total_request_cancelled.add_value(0);
    STATS::total_request_internal_failure_permille.add_value(internal_failure * 1000);
    STATS::total_request_invalid_permille.add_value(invalid_request * 1000);

    ctx.perf_counters().insert_perf_counters(&mut scuba);

    scuba.add_future_stats(stats);
    scuba.add("status", status);
    if let Some(error) = error {
        if !tunables().get_scs_error_log_sampling() {
            scuba.unsampled();
        }
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg("Request complete", None);
}

fn log_cancelled(ctx: &CoreContext, stats: &FutureStats) {
    STATS::total_request_success.add_value(0);
    STATS::total_request_internal_failure.add_value(0);
    STATS::total_request_invalid.add_value(0);
    STATS::total_request_cancelled.add_value(1);

    let mut scuba = ctx.scuba().clone();
    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.add_future_stats(stats);
    scuba.add("status", "CANCELLED");
    scuba.log_with_msg("Request cancelled", None);
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
                    let ctx = create_ctx!(self.0, $method_name, req_ctxt, $( $param_name ),*).await?;
                    ctx.scuba().clone().log_with_msg("Request start", None);
                    STATS::total_request_start.add_value(1);
                    let (stats, res) = (self.0)
                        .$method_name(ctx.clone(), $( $param_name ),* )
                        .timed()
                        .on_cancel_with_data(|stats| log_cancelled(&ctx, &stats))
                        .await;
                    log_result(ctx, &stats, &res);
                    let method = stringify!($method_name).to_string();
                    STATS::method_completion_time_ms.add_value(stats.completion_time.as_millis_unchecked() as i64, (method,));
                    res.map_err(Into::into)
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

        async fn commit_lookup_pushrebase_history(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitLookupPushrebaseHistoryParams,
        ) -> Result<thrift::CommitLookupPushrebaseHistoryResponse, service::CommitLookupPushrebaseHistoryExn>;

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

        async fn commit_list_descendant_bookmarks(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitListDescendantBookmarksParams,
        ) -> Result<thrift::CommitListDescendantBookmarksResponse, service::CommitListDescendantBookmarksExn>;

        async fn commit_run_hooks(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitRunHooksParams,
        ) -> Result<thrift::CommitRunHooksResponse, service::CommitRunHooksExn>;

        async fn commit_lookup_xrepo(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitLookupXRepoParams,
        ) -> Result<thrift::CommitLookupResponse, service::CommitLookupXrepoExn>;

        async fn commit_path_exists(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathExistsParams,
        ) -> Result<thrift::CommitPathExistsResponse, service::CommitPathExistsExn>;

        async fn commit_path_info(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathInfoParams,
        ) -> Result<thrift::CommitPathInfoResponse, service::CommitPathInfoExn>;

        async fn commit_multiple_path_info(
            commit_path: thrift::CommitSpecifier,
            params: thrift::CommitMultiplePathInfoParams,
        ) -> Result<thrift::CommitMultiplePathInfoResponse, service::CommitMultiplePathInfoExn>;

        async fn commit_path_blame(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathBlameParams,
        ) -> Result<thrift::CommitPathBlameResponse, service::CommitPathBlameExn>;

        async fn commit_path_history(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathHistoryParams,
        ) -> Result<thrift::CommitPathHistoryResponse, service::CommitPathHistoryExn>;

        async fn commit_path_last_changed(
            commit_path: thrift::CommitPathSpecifier,
            params: thrift::CommitPathLastChangedParams,
        ) -> Result<thrift::CommitPathLastChangedResponse, service::CommitPathLastChangedExn>;

        async fn commit_multiple_path_last_changed(
            commit_path: thrift::CommitSpecifier,
            params: thrift::CommitMultiplePathLastChangedParams,
        ) -> Result<thrift::CommitMultiplePathLastChangedResponse, service::CommitMultiplePathLastChangedExn>;

        async fn tree_exists(
            tree: thrift::TreeSpecifier,
            params: thrift::TreeExistsParams,
        ) -> Result<bool, service::TreeExistsExn>;

        async fn commit_sparse_profile_delta(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitSparseProfileDeltaParams,
        ) -> Result<thrift::CommitSparseProfileDeltaResponse, service::CommitSparseProfileDeltaExn>;

        async fn commit_sparse_profile_size(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitSparseProfileSizeParams,
        ) -> Result<thrift::CommitSparseProfileSizeResponse, service::CommitSparseProfileSizeExn>;

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

        async fn file_diff(
            file: thrift::FileSpecifier,
            params: thrift::FileDiffParams,
        ) -> Result<thrift::FileDiffResponse, service::FileDiffExn>;

        async fn repo_create_commit(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoCreateCommitParams,
        ) -> Result<thrift::RepoCreateCommitResponse, service::RepoCreateCommitExn>;

        async fn repo_bookmark_info(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoBookmarkInfoParams,
        ) -> Result<thrift::RepoBookmarkInfoResponse, service::RepoBookmarkInfoExn>;

        async fn repo_stack_info(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoStackInfoParams,
        ) -> Result<thrift::RepoStackInfoResponse, service::RepoStackInfoExn>;

        async fn repo_create_bookmark(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoCreateBookmarkParams,
        ) -> Result<thrift::RepoCreateBookmarkResponse, service::RepoCreateBookmarkExn>;

        async fn repo_move_bookmark(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoMoveBookmarkParams,
        ) -> Result<thrift::RepoMoveBookmarkResponse, service::RepoMoveBookmarkExn>;

        async fn repo_delete_bookmark(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoDeleteBookmarkParams,
        ) -> Result<thrift::RepoDeleteBookmarkResponse, service::RepoDeleteBookmarkExn>;

        async fn repo_land_stack(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoLandStackParams,
        ) -> Result<thrift::RepoLandStackResponse, service::RepoLandStackExn>;

        async fn megarepo_add_sync_target_config(
            params: thrift::MegarepoAddConfigParams,
        ) -> Result<thrift::MegarepoAddConfigResponse, service::MegarepoAddSyncTargetConfigExn>;

        async fn megarepo_read_target_config(
            params: thrift::MegarepoReadConfigParams,
        ) -> Result<thrift::MegarepoReadConfigResponse, service::MegarepoReadTargetConfigExn>;

        async fn megarepo_add_sync_target(
            params: thrift::MegarepoAddTargetParams,
        ) -> Result<thrift::MegarepoAddTargetToken, service::MegarepoAddSyncTargetExn>;

        async fn megarepo_add_sync_target_poll(
            params: thrift::MegarepoAddTargetToken,
        ) -> Result<thrift::MegarepoAddTargetPollResponse, service::MegarepoAddSyncTargetPollExn>;

        async fn megarepo_add_branching_sync_target(
            params: thrift::MegarepoAddBranchingTargetParams,
        ) -> Result<thrift::MegarepoAddBranchingTargetToken, service::MegarepoAddBranchingSyncTargetExn>;

        async fn megarepo_add_branching_sync_target_poll(
            params: thrift::MegarepoAddBranchingTargetToken,
        ) -> Result<thrift::MegarepoAddBranchingTargetPollResponse, service::MegarepoAddBranchingSyncTargetPollExn>;

        async fn megarepo_change_target_config(
            params: thrift::MegarepoChangeTargetConfigParams,
        ) -> Result<thrift::MegarepoChangeConfigToken, service::MegarepoChangeTargetConfigExn>;

        async fn megarepo_change_target_config_poll(
            token: thrift::MegarepoChangeConfigToken,
        ) -> Result<thrift::MegarepoChangeTargetConfigPollResponse, service::MegarepoChangeTargetConfigPollExn>;

        async fn megarepo_sync_changeset(
            params: thrift::MegarepoSyncChangesetParams,
        ) -> Result<thrift::MegarepoSyncChangesetToken, service::MegarepoSyncChangesetExn>;

        async fn megarepo_sync_changeset_poll(
            token: thrift::MegarepoSyncChangesetToken,
        ) -> Result<thrift::MegarepoSyncChangesetPollResponse, service::MegarepoSyncChangesetPollExn>;

        async fn megarepo_remerge_source(
            params: thrift::MegarepoRemergeSourceParams,
        ) -> Result<thrift::MegarepoRemergeSourceToken, service::MegarepoRemergeSourceExn>;

        async fn megarepo_remerge_source_poll(
            token: thrift::MegarepoRemergeSourceToken,
        ) -> Result<thrift::MegarepoRemergeSourcePollResponse, service::MegarepoRemergeSourcePollExn>;

        async fn repo_list_hg_manifest(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoListHgManifestParams,
        ) -> Result<thrift::RepoListHgManifestResponse, service::RepoListHgManifestExn>;
    }
}
