/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::future::Future;
use std::net::IpAddr;
use std::num::NonZeroU64;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(fbcode_build)]
use MononokeScsRequest_ods3::Instrument_MononokeScsRequest;
#[cfg(fbcode_build)]
use MononokeScsRequest_ods3_types::MononokeScsRequest;
#[cfg(fbcode_build)]
use MononokeScsRequest_ods3_types::SCSRequestEvent;
#[cfg(fbcode_build)]
use MononokeScsRequest_ods3_types::SCSRequestOutcome as Outcome;
#[cfg(fbcode_build)]
use MononokeScsRequest_ods3_types::SCSRequestType;
use async_requests::AsyncMethodRequestQueue;
use clientinfo::CLIENT_INFO_HEADER;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use connection_security_checker::ConnectionSecurityChecker;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use factory_group::FactoryGroup;
use fbinit::FacebookInit;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::try_join;
use futures_ext::FbFutureExt;
use futures_stats::FutureStats;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryStreamExt;
use futures_stats::TryStreamStats;
use futures_watchdog::WatchdogExt;
use git_source_of_truth::GitSourceOfTruthConfig;
use identity::Identity;
#[cfg(fbcode_build)]
use lazy_static::lazy_static;
use login_objects_thrift::EnvironmentType;
use megarepo_api::MegarepoApi;
use memory::MemoryStats;
use metaconfig_types::CommonConfig;
use metadata::Metadata;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CoreContext;
use mononoke_api::FileContext;
use mononoke_api::FileId;
use mononoke_api::Mononoke;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_api::SessionContainer;
use mononoke_api::TreeContext;
use mononoke_api::TreeId;
use mononoke_app::MononokeApp;
use mononoke_configs::MononokeConfigs;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use repo_authorization::AuthorizationContext;
use scribe_ext::Scribe;
use scs_errors::LoggableError;
use scs_errors::ServiceErrorResultExt;
use scs_errors::Status;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use slog::Logger;
use slog::debug;
use source_control as thrift;
use source_control_services::SourceControlService;
use source_control_services::errors::source_control_service as service;
use srserver::RequestContext;
use stats::prelude::*;
use time_ext::DurationExt;
use tracing::Instrument;

use crate::from_request::FromRequest;
use crate::scuba_params::AddScubaParams;
use crate::scuba_response::AddScubaResponse;
use crate::specifiers::SpecifierExt;

const FORWARDED_IDENTITIES_HEADER: &str = "scm_forwarded_identities";
const FORWARDED_CLIENT_IP_HEADER: &str = "scm_forwarded_client_ip";
const FORWARDED_CLIENT_PORT_HEADER: &str = "scm_forwarded_client_port";
const FORWARDED_CLIENT_DEBUG_HEADER: &str = "scm_forwarded_client_debug";
const FORWARDED_OTHER_CATS_HEADER: &str = "scm_forwarded_other_cats";
const PER_REQUEST_READ_QPS: usize = 4000;
const PER_REQUEST_WRITE_QPS: usize = 4000;

#[cfg(fbcode_build)]
lazy_static! {
    static ref SCS_REQUEST_STATS_INSTRUMENT: Instrument_MononokeScsRequest =
        Instrument_MononokeScsRequest::new();
}

define_stats! {
    prefix = "mononoke.scs_server";
    total_request_start: timeseries(Rate, Sum),
    total_request_success: timeseries(Rate, Sum),
    total_request_internal_failure: timeseries(Rate, Sum),
    total_request_invalid: timeseries(Rate, Sum),
    total_request_cancelled: timeseries(Rate, Sum),
    total_request_overloaded: timeseries(Rate, Sum),

    total_chunk_start: timeseries(Rate, Sum),
    total_chunk_success: timeseries(Rate, Sum),
    total_chunk_internal_failure: timeseries(Rate, Sum),
    total_chunk_invalid: timeseries(Rate, Sum),
    total_chunk_cancelled: timeseries(Rate, Sum),
    total_chunk_overloaded: timeseries(Rate, Sum),

    // permille is used in canaries, because canaries do not allow for tracking formulas
    total_request_internal_failure_permille: timeseries(Average),
    total_request_invalid_permille: timeseries(Average),

    // Duration per method
    method_completion_time_ms: dynamic_histogram("method.{}.completion_time_ms", (method: String); 10, 0, 1_000, Average, Sum, Count; P 5; P 50 ; P 90),
    total_method_requests:  dynamic_timeseries("method.{}.total_method_requests", (method: String); Rate, Sum),
    total_method_internal_failure:  dynamic_timeseries("method.{}.total_method_internal_failure", (method: String); Rate, Sum),

}

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) mononoke: Arc<Mononoke<Repo>>,
    pub(crate) megarepo_api: Arc<MegarepoApi<Repo>>,
    pub(crate) logger: Logger,
    pub(crate) scuba_builder: MononokeScubaSampleBuilder,
    pub(crate) identity: Identity,
    pub(crate) scribe: Scribe,
    pub(crate) configs: Arc<MononokeConfigs>,
    pub(crate) factory_group: Option<Arc<FactoryGroup<2>>>,
    pub(crate) async_requests_queue: Option<Arc<AsyncMethodRequestQueue>>,
    identity_proxy_checker: Arc<ConnectionSecurityChecker>,
    pub(crate) acl_provider: Arc<dyn AclProvider>,
    #[allow(unused)]
    pub(crate) git_source_of_truth_config: Arc<dyn GitSourceOfTruthConfig>,
    pub(crate) watchdog_max_poll: u64,
}

pub struct SourceControlServiceThriftImpl(Arc<SourceControlServiceImpl>);

impl SourceControlServiceImpl {
    pub async fn new(
        fb: FacebookInit,
        app: &MononokeApp,
        mononoke: Arc<Mononoke<Repo>>,
        megarepo_api: Arc<MegarepoApi<Repo>>,
        logger: Logger,
        mut scuba_builder: MononokeScubaSampleBuilder,
        scribe: Scribe,
        identity_proxy_checker: ConnectionSecurityChecker,
        configs: Arc<MononokeConfigs>,
        common_config: &CommonConfig,
        factory_group: Option<Arc<FactoryGroup<2>>>,
        async_requests_queue: Option<Arc<AsyncMethodRequestQueue>>,
        git_source_of_truth_config: Arc<dyn GitSourceOfTruthConfig>,
        watchdog_max_poll: u64,
    ) -> Result<Self, anyhow::Error> {
        scuba_builder.add_common_server_data();

        Ok(Self {
            fb,
            mononoke: mononoke.clone(),
            megarepo_api,
            logger,
            scuba_builder,
            identity: Identity::new(
                common_config.internal_identity.id_type.as_str(),
                common_config.internal_identity.id_data.as_str(),
            ),
            scribe,
            configs,
            identity_proxy_checker: Arc::new(identity_proxy_checker),
            factory_group,
            async_requests_queue,
            acl_provider: app.environment().acl_provider.clone(),
            git_source_of_truth_config,
            watchdog_max_poll,
        })
    }

    pub fn thrift_server(&self) -> SourceControlServiceThriftImpl {
        SourceControlServiceThriftImpl(Arc::new(self.clone()))
    }

    pub(crate) async fn create_ctx(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        specifier: Option<&dyn SpecifierExt>,
        params: &dyn AddScubaParams,
    ) -> Result<(CoreContext, String, Option<String>), scs_errors::ServiceError> {
        let session = self.create_session(req_ctxt).await?;
        let identities = session.metadata().identities();
        let mut scuba = self.create_scuba(name, req_ctxt, specifier, params, identities)?;
        if let Some(client_info) = session.metadata().client_request_info() {
            scuba.add_client_request_info(client_info);
        }
        let session_uuid = session.metadata().session_id().to_string();
        scuba.add("session_uuid", session_uuid.clone());

        let ctx = session.new_context_with_scribe(self.logger.clone(), scuba, self.scribe.clone());

        let repo_name = if let Some(specifier) = specifier {
            specifier.scuba_reponame()
        } else {
            None
        };
        Ok((ctx, session_uuid, repo_name))
    }

    /// Create and configure a scuba sample builder for a request.
    fn create_scuba(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        specifier: Option<&dyn SpecifierExt>,
        params: &dyn AddScubaParams,
        identities: &MononokeIdentitySet,
    ) -> Result<MononokeScubaSampleBuilder, scs_errors::ServiceError> {
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

        if let Some(config_info) = self.configs.as_ref().config_info().as_ref() {
            scuba.add("config_store_version", config_info.content_hash.clone());
            scuba.add("config_store_last_updated_at", config_info.last_updated_at);
        }

        let sampling_rate =
            justknobs::get_as::<u64>("scm/mononoke:scs_method_sampling_rate", Some(name))
                .ok()
                .and_then(NonZeroU64::new);
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
            let value = req_ctxt
                .header(header)
                .map_err(scs_errors::internal_error)?;
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
    ) -> Result<Metadata, scs_errors::ServiceError> {
        let header = |h: &str| req_ctxt.header(h).map_err(scs_errors::invalid_request);

        let tls_identities: MononokeIdentitySet = req_ctxt
            .identities()
            .map_err(scs_errors::internal_error)?
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
            .map_err(scs_errors::internal_error)?
            .entries()
            .into_iter()
            .map(MononokeIdentity::from_identity_ref)
            .collect();

        let client_info: Option<ClientInfo> = req_ctxt
            .header(CLIENT_INFO_HEADER)
            .map_err(scs_errors::invalid_request)?
            .as_ref()
            .and_then(|ci| serde_json::from_str(ci).ok());

        let is_trusted = self
            .identity_proxy_checker
            .check_if_trusted(&tls_identities)
            .await;

        if is_trusted {
            if let (Some(forwarded_identities), Some(forwarded_ip), Some(forwarded_port)) = (
                header(FORWARDED_IDENTITIES_HEADER)?,
                header(FORWARDED_CLIENT_IP_HEADER)?,
                header(FORWARDED_CLIENT_PORT_HEADER)?,
            ) {
                let mut header_identities: MononokeIdentitySet =
                    serde_json::from_str(forwarded_identities.as_str())
                        .map_err(scs_errors::invalid_request)?;
                let client_ip = Some(
                    forwarded_ip
                        .parse::<IpAddr>()
                        .map_err(scs_errors::invalid_request)?,
                );
                let client_port = Some(
                    forwarded_port
                        .parse::<u16>()
                        .map_err(scs_errors::invalid_request)?,
                );
                let client_debug = header(FORWARDED_CLIENT_DEBUG_HEADER)?.is_some();

                header_identities.extend(cats_identities.into_iter());
                let mut metadata = Metadata::new(
                    None,
                    header_identities,
                    client_debug,
                    metadata::security::is_client_untrusted(|h| req_ctxt.header(h))
                        .map_err(scs_errors::invalid_request)?,
                    client_ip,
                    client_port,
                )
                .await;

                metadata.add_original_identities(tls_identities);

                if let Some(other_cats) = header(FORWARDED_OTHER_CATS_HEADER)? {
                    metadata.add_raw_encoded_cats(other_cats);
                }
                let client_info = client_info.unwrap_or_else(|| {
                    ClientInfo::default_with_entry_point(ClientEntryPoint::ScsServer)
                });
                metadata.add_client_info(client_info);
                return Ok(metadata);
            }
        }

        let mut metadata = Metadata::new(
            None,
            tls_identities.union(&cats_identities).cloned().collect(),
            false,
            metadata::security::is_client_untrusted(|h| req_ctxt.header(h))
                .map_err(scs_errors::invalid_request)?,
            Some(
                req_ctxt
                    .get_peer_ip_address()
                    .map_err(scs_errors::internal_error)?,
            ),
            Some(
                req_ctxt
                    .get_peer_port()
                    .map_err(scs_errors::internal_error)?,
            ),
        )
        .await;

        let client_info = client_info
            .unwrap_or_else(|| ClientInfo::default_with_entry_point(ClientEntryPoint::ScsServer));
        metadata.add_client_info(client_info);
        Ok(metadata)
    }

    /// Create and configure the session container for a request.
    async fn create_session(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<SessionContainer, scs_errors::ServiceError> {
        let metadata = self.create_metadata(req_ctxt).await?;
        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .blobstore_maybe_read_qps_limiter(PER_REQUEST_READ_QPS)
            .await
            .blobstore_maybe_write_qps_limiter(PER_REQUEST_WRITE_QPS)
            .await
            .build();
        Ok(session)
    }

    /// Get the repo specified by a `thrift::RepoSpecifier`.
    pub(crate) async fn repo(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
    ) -> Result<RepoContext<Repo>, scs_errors::ServiceError> {
        let authz = AuthorizationContext::new(&ctx);
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
    ) -> Result<RepoContext<Repo>, scs_errors::ServiceError> {
        let authz = match service_name {
            Some(service_name) => AuthorizationContext::new_for_service_writes(service_name),
            None => AuthorizationContext::new(&ctx),
        };
        self.repo_impl(ctx, repo, authz, |_| async { Ok(None) })
            .await
    }

    pub async fn repo_impl<F, R>(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
        authz: AuthorizationContext,
        bubble_fetcher: F,
    ) -> Result<RepoContext<Repo>, scs_errors::ServiceError>
    where
        F: FnOnce(RepoEphemeralStore) -> R,
        R: Future<Output = anyhow::Result<Option<BubbleId>>>,
    {
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)
            .await?
            .ok_or_else(|| scs_errors::repo_not_found(repo.description()))?
            .with_bubble(bubble_fetcher)
            .await?
            .with_authorization_context(authz)
            .build()
            .await?;
        Ok(repo)
    }

    fn bubble_fetcher_for_changeset(
        &self,
        ctx: CoreContext,
        specifier: ChangesetSpecifier,
    ) -> impl FnOnce(RepoEphemeralStore) -> BoxFuture<'static, anyhow::Result<Option<BubbleId>>> + use<>
    {
        move |ephemeral| async move { specifier.bubble_id(&ctx, ephemeral).await }.boxed()
    }

    /// Get the repo and changeset specified by a `thrift::CommitSpecifier`.
    pub(crate) async fn repo_changeset(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
    ) -> Result<(RepoContext<Repo>, ChangesetContext<Repo>), scs_errors::ServiceError> {
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let authz = AuthorizationContext::new(&ctx);
        let bubble_fetcher =
            self.bubble_fetcher_for_changeset(ctx.clone(), changeset_specifier.clone());
        let repo = self
            .repo_impl(ctx, &commit.repo, authz, bubble_fetcher)
            .await?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| scs_errors::commit_not_found(commit.description()))?;
        Ok((repo, changeset))
    }

    /// Get the repo and pair of changesets specified by a `thrift::CommitSpecifier`
    /// and `thrift::CommitId` pair.
    pub(crate) async fn repo_changeset_pair(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
        other_commit: &thrift::CommitId,
    ) -> Result<
        (
            RepoContext<Repo>,
            ChangesetContext<Repo>,
            ChangesetContext<Repo>,
        ),
        scs_errors::ServiceError,
    > {
        let changeset_specifier =
            ChangesetSpecifier::from_request(&commit.id).context("invalid target commit id")?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(other_commit)
            .context("invalid or missing other commit id")?;
        if other_changeset_specifier.in_bubble() {
            Err(scs_errors::invalid_request(format!(
                "Can't compare against a snapshot: {}",
                other_changeset_specifier
            )))?
        }
        let authz = AuthorizationContext::new(&ctx);
        let bubble_fetcher =
            self.bubble_fetcher_for_changeset(ctx.clone(), changeset_specifier.clone());
        let repo = self
            .repo_impl(ctx, &commit.repo, authz, bubble_fetcher)
            .await?;
        let (changeset, other_changeset) = try_join!(
            async {
                Ok::<_, scs_errors::ServiceError>(
                    repo.changeset(changeset_specifier)
                        .await
                        .context("failed to resolve target commit")?
                        .ok_or_else(|| scs_errors::commit_not_found(commit.description()))?,
                )
            },
            async {
                Ok::<_, scs_errors::ServiceError>(
                    repo.changeset(other_changeset_specifier)
                        .await
                        .context("failed to resolve other commit")?
                        .ok_or_else(|| {
                            scs_errors::commit_not_found(format!(
                                "repo={} commit={}",
                                commit.repo.name, other_commit
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
        repo: &RepoContext<Repo>,
        id: &thrift::CommitId,
    ) -> Result<ChangesetId, scs_errors::ServiceError> {
        let changeset_specifier = ChangesetSpecifier::from_request(id)?;
        Ok(repo
            .resolve_specifier(changeset_specifier)
            .await?
            .ok_or_else(|| {
                scs_errors::commit_not_found(format!("repo={} commit={}", repo.name(), id))
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
    ) -> Result<(RepoContext<Repo>, Option<TreeContext<Repo>>), scs_errors::ServiceError> {
        let (repo, tree) = match tree {
            thrift::TreeSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path_with_content(&commit_path.path).await?;
                (repo, path.tree().await?)
            }
            thrift::TreeSpecifier::by_id(tree_id) => {
                let repo = self.repo(ctx, &tree_id.repo).await?;
                let tree_id = TreeId::from_request(&tree_id.id)?;
                let tree = repo
                    .tree(tree_id)
                    .await?
                    .ok_or_else(|| scs_errors::tree_not_found(tree.description()))?;
                (repo, Some(tree))
            }
            thrift::TreeSpecifier::UnknownField(id) => {
                return Err(scs_errors::invalid_request(format!(
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
    ) -> Result<(RepoContext<Repo>, Option<FileContext<Repo>>), scs_errors::ServiceError> {
        let (repo, file) = match file {
            thrift::FileSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path_with_content(&commit_path.path).await?;
                (repo, path.file().await?)
            }
            thrift::FileSpecifier::by_id(file_id) => {
                let repo = self.repo(ctx, &file_id.repo).await?;
                let file_id = FileId::from_request(&file_id.id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| scs_errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha1_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo).await?;
                let file_sha1 = Sha1::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha1(file_sha1)
                    .await?
                    .ok_or_else(|| scs_errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha256_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo).await?;
                let file_sha256 = Sha256::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha256(file_sha256)
                    .await?
                    .ok_or_else(|| scs_errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::UnknownField(id) => {
                return Err(scs_errors::invalid_request(format!(
                    "file specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, file))
    }
}

fn should_log_memory_usage(method: &str) -> bool {
    justknobs::eval("scm/mononoke:scs_log_memory_usage", None, Some(method)).unwrap_or(false)
}

fn log_start(ctx: &CoreContext, method: &str) -> Option<MemoryStats> {
    let mut start_mem_stats = None;
    let mut scuba = ctx.scuba().clone();
    if should_log_memory_usage(method) {
        if let Ok(stats) = memory::get_stats() {
            scuba.add_memory_stats(&stats);
            start_mem_stats = Some(stats);
        }
    }
    scuba.log_with_msg("Request start", None);
    start_mem_stats
}

fn add_request_end_memory_stats(
    scuba: &mut MononokeScubaSampleBuilder,
    method: &str,
    start_mem_stats: Option<&MemoryStats>,
) {
    if should_log_memory_usage(method) {
        if let Ok(stats) = memory::get_stats() {
            scuba.add_memory_stats(&stats);
            if let Some(start_mem_stats) = start_mem_stats {
                let rss_used_delta =
                    start_mem_stats.rss_free_bytes as isize - stats.rss_free_bytes as isize;
                scuba.add("rss_used_delta", rss_used_delta);
            }
        }
    }
}

fn log_result<T: AddScubaResponse>(
    ctx: CoreContext,
    tag: &'static str,
    request_type: SCSRequestType,
    event: SCSRequestEvent,
    method: &str,
    repo: Option<String>,
    stats: &FutureStats,
    result: &Result<T, impl scs_errors::LoggableError>,
    start_mem_stats: Option<&MemoryStats>,
) {
    let mut scuba = ctx.scuba().clone();

    add_request_end_memory_stats(&mut scuba, method, start_mem_stats);

    let (status, outcome, error, invalid_request, internal_failure, overloaded) = match result {
        Ok(response) => {
            response.add_scuba_response(&mut scuba);
            ("SUCCESS", Outcome::Success, None, 0, 0, 0)
        }
        Err(err) => {
            let (status, desc) = err.status_and_description();
            match status {
                Status::RequestError => {
                    ("REQUEST_ERROR", Outcome::RequestError, Some(desc), 1, 0, 0)
                }
                Status::InternalError => (
                    "INTERNAL_ERROR",
                    Outcome::InternalError,
                    Some(desc),
                    0,
                    1,
                    0,
                ),
                Status::OverloadError => (
                    "OVERLOAD_ERROR",
                    Outcome::OverloadError,
                    Some(desc),
                    0,
                    0,
                    1,
                ),
                Status::PollError => ("POLL_ERROR", Outcome::PollError, Some(desc), 0, 1, 0),
            }
        }
    };

    if let Ok(true) = justknobs::eval("scm/mononoke:scs_alert_on_methods", None, Some(method)) {
        STATS::total_method_requests.add_value(1, (method.to_string(),));
        if status == "INTERNAL_ERROR" {
            STATS::total_method_internal_failure.add_value(1, (method.to_string(),));
        } else {
            STATS::total_method_internal_failure.add_value(0, (method.to_string(),));
        }
    }
    let success = if error.is_none() { 1 } else { 0 };

    STATS::total_request_success.add_value(success);
    STATS::total_request_internal_failure.add_value(internal_failure);
    STATS::total_request_invalid.add_value(invalid_request);
    STATS::total_request_cancelled.add_value(0);
    STATS::total_request_internal_failure_permille.add_value(internal_failure * 1000);
    STATS::total_request_invalid_permille.add_value(invalid_request * 1000);
    STATS::total_request_overloaded.add_value(overloaded);
    STATS::method_completion_time_ms.add_value(
        stats.completion_time.as_millis_unchecked() as i64,
        (method.to_string(),),
    );

    ctx.perf_counters().insert_perf_counters(&mut scuba);

    scuba.add_future_stats(stats);
    scuba.add("status", status);
    if let Some(error) = error {
        let scs_error_log_sampling =
            justknobs::eval("scm/mononoke:scs_error_log_sampling", None, None).unwrap_or(true);
        if !scs_error_log_sampling {
            scuba.unsampled();
        }
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg(tag, None);

    #[cfg(fbcode_build)]
    SCS_REQUEST_STATS_INSTRUMENT.observe(MononokeScsRequest {
        method: Some(method.to_string()),
        repo,
        request_type: Some(request_type),
        event: Some(event),
        outcome: Some(outcome),
        requests: Some(1.0),
        duration_ms: Some(stats.completion_time.as_millis_unchecked() as f64),
        ..Default::default()
    });
}

fn log_stream_chunk<T: AddScubaResponse>(
    ctx: CoreContext,
    method: &str,
    repo: Option<String>,
    result: &Result<T, impl scs_errors::LoggableError>,
    count: u64,
) {
    let mut scuba = ctx.scuba().clone();

    let (status, outcome, error, invalid_request, internal_failure, overloaded) = match result {
        Ok(response) => {
            response.add_scuba_response(&mut scuba);
            ("SUCCESS", Outcome::Success, None, 0, 0, 0)
        }
        Err(err) => {
            let (status, desc) = err.status_and_description();
            match status {
                Status::RequestError => {
                    ("REQUEST_ERROR", Outcome::RequestError, Some(desc), 1, 0, 0)
                }
                Status::InternalError => (
                    "INTERNAL_ERROR",
                    Outcome::InternalError,
                    Some(desc),
                    0,
                    1,
                    0,
                ),
                Status::OverloadError => (
                    "OVERLOAD_ERROR",
                    Outcome::OverloadError,
                    Some(desc),
                    0,
                    0,
                    1,
                ),
                Status::PollError => ("POLL_ERROR", Outcome::PollError, Some(desc), 0, 0, 1),
            }
        }
    };
    let success = if error.is_none() { 1 } else { 0 };

    STATS::total_chunk_success.add_value(success);
    STATS::total_chunk_internal_failure.add_value(internal_failure);
    STATS::total_chunk_invalid.add_value(invalid_request);
    STATS::total_chunk_cancelled.add_value(0);
    STATS::total_chunk_overloaded.add_value(overloaded);

    ctx.perf_counters().insert_perf_counters(&mut scuba);

    scuba.add("stream_chunk_count", count);
    scuba.add("status", status);
    if let Some(error) = error {
        scuba.add("error", error.as_str());
    }
    let sampling_rate = NonZeroU64::new(
        justknobs::get_as::<u64>(
            "scm/mononoke:scs_stream_chunk_scuba_sampling_rate",
            Some(method),
        )
        .ok()
        .unwrap_or(1000),
    ); // 1:1000 by default to avoid spamming scuba
    if let Some(sampling_rate) = sampling_rate {
        scuba.sampled(sampling_rate);
    }
    scuba.log_with_msg("Request stream chunk", None);

    #[cfg(fbcode_build)]
    SCS_REQUEST_STATS_INSTRUMENT.observe(MononokeScsRequest {
        method: Some(method.to_string()),
        repo,
        request_type: Some(SCSRequestType::Stream),
        event: Some(SCSRequestEvent::StreamChunk),
        outcome: Some(outcome),
        requests: Some(1.0),
        ..Default::default()
    });
}

fn log_stream_complete(
    ctx: CoreContext,
    method: &str,
    repo: Option<String>,
    initial_future_stats: &FutureStats,
    stream_stats: &TryStreamStats,
    mb_status_and_description: Option<(Status, String)>,
    start_mem_stats: Option<&MemoryStats>,
) {
    let mut scuba = ctx.scuba().clone();

    add_request_end_memory_stats(&mut scuba, method, start_mem_stats);

    let (status, outcome, error, invalid_request, internal_failure, overloaded) =
        match mb_status_and_description {
            Some((status, desc)) => match status {
                Status::RequestError => {
                    ("REQUEST_ERROR", Outcome::RequestError, Some(desc), 1, 0, 0)
                }
                Status::InternalError => (
                    "INTERNAL_ERROR",
                    Outcome::InternalError,
                    Some(desc),
                    0,
                    1,
                    0,
                ),
                Status::OverloadError => (
                    "OVERLOAD_ERROR",
                    Outcome::OverloadError,
                    Some(desc),
                    0,
                    0,
                    1,
                ),
                Status::PollError => ("POLL_ERROR", Outcome::PollError, Some(desc), 0, 1, 0),
            },
            None => ("SUCCESS", Outcome::Success, None, 0, 0, 0),
        };

    if let Ok(true) = justknobs::eval("scm/mononoke:scs_alert_on_methods", None, Some(method)) {
        STATS::total_method_requests.add_value(1, (method.to_string(),));
        if status == "INTERNAL_ERROR" {
            STATS::total_method_internal_failure.add_value(1, (method.to_string(),));
        } else {
            STATS::total_method_internal_failure.add_value(0, (method.to_string(),));
        }
    }
    let success = if stream_stats.error_count > 0 { 0 } else { 1 };

    STATS::total_request_success.add_value(success);
    STATS::total_request_internal_failure.add_value(internal_failure);
    STATS::total_request_invalid.add_value(invalid_request);
    STATS::total_request_cancelled.add_value(0);
    STATS::total_request_internal_failure_permille.add_value(internal_failure * 1000);
    STATS::total_request_invalid_permille.add_value(invalid_request * 1000);
    STATS::total_request_overloaded.add_value(overloaded);
    // Only accounts for the time to start the stream, not the overall time.
    STATS::method_completion_time_ms.add_value(
        initial_future_stats.completion_time.as_millis_unchecked() as i64,
        (method.to_string(),),
    );

    ctx.perf_counters().insert_perf_counters(&mut scuba);

    // This function combines the stats from the initial phase generating the stream
    // object with stats from stream polling.
    //
    // It might have been more obvious to log those separately but from experience
    // most scuba queries to out table are filtering by "Request complete" tag so
    // having aggregated stats encompassing entirety of request might be easier to
    // interpret and query.
    let mut combined_stats = stream_stats.clone();
    combined_stats.stream_stats.poll_count += initial_future_stats.poll_count;
    combined_stats.stream_stats.poll_time += initial_future_stats.poll_time;
    combined_stats.stream_stats.max_poll_time = std::cmp::max(
        combined_stats.stream_stats.max_poll_time,
        initial_future_stats.max_poll_time,
    );
    combined_stats.stream_stats.completion_time = Some(
        initial_future_stats.completion_time
            + combined_stats
                .stream_stats
                .completion_time
                .unwrap_or(Duration::ZERO),
    );
    // The initial processing counts towards first item time
    combined_stats.stream_stats.first_item_time = combined_stats
        .stream_stats
        .first_item_time
        .map(|t| t + initial_future_stats.completion_time);

    scuba.add_try_stream_stats(&combined_stats);
    scuba.add("status", status);
    if let Some(error) = error {
        let scs_error_log_sampling =
            justknobs::eval("scm/mononoke:scs_error_log_sampling", None, None).unwrap_or(true);
        if !scs_error_log_sampling {
            scuba.unsampled();
        }
        scuba.add("error", error.as_str());
    }
    scuba.log_with_msg("Request complete", None);

    #[cfg(fbcode_build)]
    SCS_REQUEST_STATS_INSTRUMENT.observe(MononokeScsRequest {
        method: Some(method.to_string()),
        repo,
        request_type: Some(SCSRequestType::Stream),
        event: Some(SCSRequestEvent::StreamComplete),
        outcome: Some(outcome),
        requests: Some(1.0),
        duration_ms: Some(initial_future_stats.completion_time.as_millis_unchecked() as f64),
        ..Default::default()
    });
}

fn log_cancelled(
    ctx: &CoreContext,
    method: &str,
    repo: Option<String>,
    stats: &FutureStats,
    start_mem_stats: Option<&MemoryStats>,
) {
    STATS::total_request_success.add_value(0);
    STATS::total_request_internal_failure.add_value(0);
    STATS::total_request_invalid.add_value(0);
    STATS::total_request_cancelled.add_value(1);

    let mut scuba = ctx.scuba().clone();
    add_request_end_memory_stats(&mut scuba, method, start_mem_stats);
    ctx.perf_counters().insert_perf_counters(&mut scuba);
    scuba.add_future_stats(stats);
    scuba.add("status", "CANCELLED");
    scuba.log_with_msg("Request cancelled", None);

    #[cfg(fbcode_build)]
    SCS_REQUEST_STATS_INSTRUMENT.observe(MononokeScsRequest {
        method: Some(method.to_string()),
        repo,
        request_type: Some(SCSRequestType::Stream),
        event: Some(SCSRequestEvent::Cancelled),
        requests: Some(1.0),
        ..Default::default()
    });
}

fn check_memory_usage(
    ctx: &CoreContext,
    method: &str,
    start_mem_stats: Option<&MemoryStats>,
) -> Result<(), scs_errors::ServiceError> {
    let stats = match start_mem_stats {
        Some(start_mem_stats) => Cow::Borrowed(start_mem_stats),
        None => match memory::get_stats() {
            Ok(stats) => Cow::Owned(stats),
            _ => return Ok(()),
        },
    };
    let rss_min_free_bytes =
        justknobs::get_as::<usize>("scm/mononoke:scs_rss_min_free_bytes", Some(method))
            .unwrap_or(0);
    let rss_min_free_pct =
        justknobs::get_as::<i32>("scm/mononoke:scs_rss_min_free_pct", Some(method)).unwrap_or(0);

    if rss_min_free_bytes > 0 || rss_min_free_pct > 0 {
        debug!(
            ctx.logger(),
            "{}: min free mem: {} {}%", method, rss_min_free_bytes, rss_min_free_pct
        );

        debug!(
            ctx.logger(),
            "{}: memory stats: free {} / total {} {:.1}%",
            method,
            stats.rss_free_bytes,
            stats.total_rss_bytes,
            stats.rss_free_pct
        );

        if stats.rss_free_bytes < rss_min_free_bytes {
            debug!(
                ctx.logger(),
                "{}: not enough memory free, need at least {} bytes free, only {} free right now",
                method,
                rss_min_free_bytes,
                stats.rss_free_bytes,
            );

            return Err(scs_errors::overloaded(format!(
                "Not enough memory free ({} < {})",
                stats.rss_free_bytes, rss_min_free_bytes
            ))
            .into());
        }
        if stats.rss_free_pct < rss_min_free_pct as f32 {
            debug!(
                ctx.logger(),
                "{}: not enough memory free, need at least {}% free, only {:.1}% free right now",
                method,
                rss_min_free_pct,
                stats.rss_free_pct,
            );

            return Err(scs_errors::overloaded(format!(
                "Not enough memory free ({:.0}% < {}%)",
                stats.rss_free_pct, rss_min_free_pct
            ))
            .into());
        }
    }
    Ok(())
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
                let fut = async move {
                    let svc = self.0.clone();
                    let watchdog_max_poll = self.0.watchdog_max_poll;
                    let (ctx, session_uuid, repo_name) = create_ctx!(svc, $method_name, req_ctxt, $( $param_name ),*).await?;
                    let handler = {
                        cloned!(ctx);
                        async move {
                            let start_mem_stats = log_start(&ctx, stringify!($method_name));
                            STATS::total_request_start.add_value(1);
                            let (stats, res) = async {
                                check_memory_usage(&ctx, stringify!($method_name), start_mem_stats.as_ref())?;
                                let f = svc.$method_name(ctx.clone(), $( $param_name ),* );
                                    f.watched(ctx.logger())
                                    .with_label(stringify!($method_name))
                                    .with_unique_id(&session_uuid)
                                    .with_max_poll(watchdog_max_poll).await
                            }
                            .timed()
                            .on_cancel_with_data(|stats| log_cancelled(&ctx, stringify!($method_name), repo_name.clone(), &stats, start_mem_stats.as_ref()))
                            .await;
                            log_result(ctx, "Request complete", SCSRequestType::Normal, SCSRequestEvent::Complete, stringify!($method_name), repo_name, &stats, &res, start_mem_stats.as_ref());
                            res.map_err(Into::into)
                        }
                    };

                    if let Some(factory_group) = &self.0.factory_group {
                        let group = factory_group.clone();
                        let queue: usize =
                            justknobs::get_as::<u64>("scm/mononoke:scs_factory_queue_for_method", Some(stringify!($method_name))).unwrap_or(0) as usize;
                        group.execute(queue, handler, None).await.map_err(|e| scs_errors::internal_error(e.to_string()))?
                    } else {
                        let res: Result<$ok_type, $err_type> = handler.await;
                        res
                    }
                };
                Box::pin(fut)
            }
        )*
    }
}

macro_rules! impl_thrift_stream_methods {
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
                let fut = async move {
                    let svc = self.0.clone();
                    let (ctx, session_uuid, repo_name) = create_ctx!(svc, $method_name, req_ctxt, $( $param_name ),*).await?;
                    let handler = {
                        cloned!(ctx, repo_name);
                        let span = tracing::info_span!("scs method", repo = tracing::field::Empty, method = %stringify!($method_name));
                        if let Some(repo_name) = &repo_name {
                            span.record("repo", repo_name);
                        }
                        async move {
                            let start_mem_stats = log_start(&ctx, stringify!($method_name));
                            STATS::total_request_start.add_value(1);
                            let (stats, res) = async {
                                check_memory_usage(&ctx, stringify!($method_name), start_mem_stats.as_ref())?;
                                let f = svc.$method_name(ctx.clone(), $( $param_name ),* );
                                    f.watched(ctx.logger())
                                    .with_label(stringify!($method_name))
                                    .with_unique_id(&session_uuid)
                                    .with_max_poll(50).await
                            }
                            .timed()
                            .on_cancel_with_data(|stats| log_cancelled(&ctx, stringify!($method_name), repo_name.clone(), &stats, start_mem_stats.as_ref()))
                            .await;
                            if res.is_ok() {
                                log_result(ctx.clone(), "Request stream started", SCSRequestType::Stream, SCSRequestEvent::StreamStarted, stringify!($method_name), repo_name.clone(), &stats, &res, start_mem_stats.as_ref());
                            } else {
                                log_result(ctx.clone(), "Request complete", SCSRequestType::Stream, SCSRequestEvent::Complete, stringify!($method_name), repo_name.clone(), &stats, &res, start_mem_stats.as_ref());
                            }
                            let first_error = Arc::new(OnceLock::new());
                            let chunk_counter = AtomicU64::new(0);
                            res.map_err(Into::into).map(move |(res, stream)| {
                                let stream = stream.inspect({
                                    cloned!(ctx, repo_name, first_error);
                                    move |res| {
                                        let count = chunk_counter.fetch_add(1, Ordering::Relaxed);
                                        log_stream_chunk(ctx.clone(), stringify!($method_name), repo_name.clone(), &res, count);
                                        if let Err(err) = res {
                                            let (status, desc) = err.status_and_description();
                                            let _ = first_error.set((status, desc));
                                        }
                                    }
                                }).boxed().try_timed({
                                    cloned!(ctx, repo_name, first_error);
                                    move |stream_stats| {
                                        log_stream_complete(ctx, stringify!($method_name), repo_name, &stats, &stream_stats, first_error.get().cloned() ,start_mem_stats.as_ref());
                                    }
                                }).map_err(Into::into).boxed();
                                (res, stream)
                            })
                        }
                        .instrument(span)
                    };

                    if let Some(factory_group) = &self.0.factory_group {
                        let group = factory_group.clone();
                        let queue: usize =
                            justknobs::get_as::<u64>("scm/mononoke:scs_factory_queue_for_method", Some(stringify!($method_name))).unwrap_or(0) as usize;
                        group.execute(queue, handler, None).await.map_err(|e| scs_errors::internal_error(e.to_string()))?
                    } else {
                        let res: Result<$ok_type, $err_type> = handler.await;
                        res
                    }
                };
                Box::pin(fut)
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

        async fn repo_info(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoInfoParams,
        ) -> Result<thrift::RepoInfo, service::RepoInfoExn>;

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

        async fn repo_multiple_commit_lookup(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoMultipleCommitLookupParams,
        ) -> Result<thrift::RepoMultipleCommitLookupResponse, service::RepoMultipleCommitLookupExn>;

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

        async fn commit_generation(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitGenerationParams,
        ) -> Result<i64, service::CommitGenerationExn>;

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

        async fn commit_linear_history(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitLinearHistoryParams,
        ) -> Result<thrift::CommitLinearHistoryResponse, service::CommitLinearHistoryExn>;

        async fn commit_hg_mutation_history(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitHgMutationHistoryParams,
        ) -> Result<thrift::CommitHgMutationHistoryResponse, service::CommitHgMutationHistoryExn>;

        async fn commit_list_descendant_bookmarks(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitListDescendantBookmarksParams,
        ) -> Result<thrift::CommitListDescendantBookmarksResponse, service::CommitListDescendantBookmarksExn>;

        async fn commit_run_hooks(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitRunHooksParams,
        ) -> Result<thrift::CommitRunHooksResponse, service::CommitRunHooksExn>;

        async fn commit_subtree_changes(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitSubtreeChangesParams,
        ) -> Result<thrift::CommitSubtreeChangesResponse, service::CommitSubtreeChangesExn>;

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

        async fn commit_sparse_profile_delta_async(
            params: thrift::CommitSparseProfileDeltaParamsV2,
        ) -> Result<thrift::CommitSparseProfileDeltaToken, service::CommitSparseProfileDeltaAsyncExn>;

        async fn commit_sparse_profile_delta_poll(
            token: thrift::CommitSparseProfileDeltaToken,
        ) -> Result<thrift::CommitSparseProfileDeltaPollResponse, service::CommitSparseProfileDeltaPollExn>;

        async fn commit_sparse_profile_size_async(
            params: thrift::CommitSparseProfileSizeParamsV2,
        ) -> Result<thrift::CommitSparseProfileSizeToken, service::CommitSparseProfileSizeAsyncExn>;

        async fn commit_sparse_profile_size_poll(
            token: thrift::CommitSparseProfileSizeToken,
        ) -> Result<thrift::CommitSparseProfileSizePollResponse, service::CommitSparseProfileSizePollExn>;

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

        async fn repo_create_stack(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoCreateStackParams,
        ) -> Result<thrift::RepoCreateStackResponse, service::RepoCreateStackExn>;

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

        async fn repo_prepare_commits(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoPrepareCommitsParams,
        ) -> Result<thrift::RepoPrepareCommitsResponse, service::RepoPrepareCommitsExn>;

        async fn repo_upload_file_content(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoUploadFileContentParams,
        ) -> Result<thrift::RepoUploadFileContentResponse, service::RepoUploadFileContentExn>;

        async fn create_repos(
            params: thrift::CreateReposParams,
        ) -> Result<thrift::CreateReposToken, service::CreateReposExn>;

        async fn create_repos_poll(
            params: thrift::CreateReposToken,
        ) -> Result<thrift::CreateReposPollResponse, service::CreateReposPollExn>;

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

        async fn repo_update_submodule_expansion(
            params: thrift::RepoUpdateSubmoduleExpansionParams,
        ) -> Result<thrift::RepoUpdateSubmoduleExpansionResponse, service::RepoUpdateSubmoduleExpansionExn>;

        async fn repo_upload_non_blob_git_object(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoUploadNonBlobGitObjectParams,
        ) -> Result<thrift::RepoUploadNonBlobGitObjectResponse, service::RepoUploadNonBlobGitObjectExn>;

        async fn create_git_tree(
            repo: thrift::RepoSpecifier,
            params: thrift::CreateGitTreeParams,
        ) -> Result<thrift::CreateGitTreeResponse, service::CreateGitTreeExn>;

        async fn create_git_tag(
            repo: thrift::RepoSpecifier,
            params: thrift::CreateGitTagParams,
        ) -> Result<thrift::CreateGitTagResponse, service::CreateGitTagExn>;

        async fn repo_stack_git_bundle_store(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoStackGitBundleStoreParams,
        ) -> Result<thrift::RepoStackGitBundleStoreResponse, service::RepoStackGitBundleStoreExn>;

        async fn repo_upload_packfile_base_item(
            repo: thrift::RepoSpecifier,
            params: thrift::RepoUploadPackfileBaseItemParams,
        ) -> Result<thrift::RepoUploadPackfileBaseItemResponse, service::RepoUploadPackfileBaseItemExn>;

        async fn cloud_workspace_info(
            params: thrift::CloudWorkspaceInfoParams,
        ) -> Result<thrift::CloudWorkspaceInfoResponse, service::CloudWorkspaceInfoExn>;

        async fn cloud_user_workspaces(
            params: thrift::CloudUserWorkspacesParams,
        ) -> Result<thrift::CloudUserWorkspacesResponse, service::CloudUserWorkspacesExn>;

        async fn cloud_workspace_smartlog(
            params: thrift::CloudWorkspaceSmartlogParams,
        ) -> Result<thrift::CloudWorkspaceSmartlogResponse, service::CloudWorkspaceSmartlogExn>;

        async fn async_ping(
            params: thrift::AsyncPingParams,
        ) -> Result<thrift::AsyncPingToken, service::AsyncPingExn>;

        async fn async_ping_poll(
            params: thrift::AsyncPingToken,
        ) -> Result<thrift::AsyncPingPollResponse, service::AsyncPingPollExn>;

    }

    impl_thrift_stream_methods! {
        async fn commit_find_files_stream(
            commit: thrift::CommitSpecifier,
            params: thrift::CommitFindFilesParams,
        ) -> Result<
            (
                thrift::CommitFindFilesStreamResponse,
                BoxStream<'static, Result<thrift::CommitFindFilesStreamItem, service::CommitFindFilesStreamStreamExn>>,
            ),
            service::CommitFindFilesStreamExn>;
    }
}
