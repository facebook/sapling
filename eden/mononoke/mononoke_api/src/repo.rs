/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use acl_regions::build_disabled_acl_regions;
use acl_regions::AclRegions;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Error;
use blobrepo::AsBlobRepo;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use blobstore_factory::make_metadata_sql_factory;
use blobstore_factory::ReadOnlyStorage;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::Bookmarks;
use bookmarks::BookmarksArc;
pub use bookmarks::Freshness as BookmarkFreshness;
use bookmarks::Freshness;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use changeset_fetcher::ChangesetFetcher;
use changeset_info::ChangesetInfo;
use changesets::Changesets;
use changesets::ChangesetsArc;
use changesets::ChangesetsRef;
use context::CoreContext;
use cross_repo_sync::types::Target;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use ephemeral_blobstore::ArcRepoEphemeralStore;
use ephemeral_blobstore::Bubble;
use ephemeral_blobstore::BubbleId;
use ephemeral_blobstore::RepoEphemeralStore;
use ephemeral_blobstore::RepoEphemeralStoreArc;
use ephemeral_blobstore::RepoEphemeralStoreRef;
use ephemeral_blobstore::StorageLocation;
use fbinit::FacebookInit;
use filestore::Alias;
use filestore::FetchKey;
use futures::compat::Stream01CompatExt;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::Future;
use hooks::HookManager;
use hooks::HookManagerArc;
use itertools::Itertools;
use live_commit_sync_config::LiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfig;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_mutation::HgMutationStore;
use mercurial_types::Globalrev;
use metaconfig_types::HookManagerParams;
use metaconfig_types::InfinitepushNamespace;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::LfsParams;
use metaconfig_types::RepoConfig;
use metaconfig_types::SourceControlServiceParams;
use mononoke_api_types::InnerRepo;
use mononoke_repos::MononokeRepos;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use mononoke_types::Svnrev;
use mononoke_types::Timestamp;
use mutable_counters::MutableCounters;
use mutable_renames::ArcMutableRenames;
use mutable_renames::MutableRenames;
use mutable_renames::MutableRenamesArc;
use mutable_renames::SqlMutableRenamesStore;
use phases::Phases;
use phases::PhasesArc;
use phases::PhasesRef;
use pushrebase_mutation_mapping::PushrebaseMutationMapping;
use reachabilityindex::LeastCommonAncestorsHint;
use regex::Regex;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreArc;
use repo_bookmark_attrs::RepoBookmarkAttrs;
use repo_cross_repo::RepoCrossRepo;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataArc;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityArc;
use repo_identity::RepoIdentityRef;
use repo_lock::RepoLock;
use repo_permission_checker::RepoPermissionChecker;
use repo_sparse_profiles::ArcRepoSparseProfiles;
use repo_sparse_profiles::RepoSparseProfiles;
use repo_sparse_profiles::RepoSparseProfilesArc;
use revset::AncestorsNodeStream;
use revset::DifferenceOfUnionsOfAncestorsNodeStream;
use segmented_changelog::CloneData;
use segmented_changelog::DisabledSegmentedChangelog;
use segmented_changelog::Location;
use segmented_changelog::SegmentedChangelog;
use segmented_changelog::SegmentedChangelogRef;
use skiplist::ArcSkiplistIndex;
use skiplist::SkiplistIndex;
use skiplist::SkiplistIndexArc;
use slog::debug;
use slog::error;
use sql_construct::SqlConstruct;
use sql_ext::facebook::MysqlOptions;
use stats::prelude::*;
use streaming_clone::StreamingClone;
use streaming_clone::StreamingCloneBuilder;
use synced_commit_mapping::ArcSyncedCommitMapping;
use synced_commit_mapping::SqlSyncedCommitMapping;
use test_repo_factory::TestRepoFactory;
use tunables::tunables;
use unbundle::PushRedirector;
use unbundle::PushRedirectorArgs;
use warm_bookmarks_cache::BookmarksCache;
use warm_bookmarks_cache::WarmBookmarksCacheBuilder;
use wireproto_handler::PushRedirectorBase;
use wireproto_handler::RepoHandlerBase;
use wireproto_handler::RepoHandlerBaseRef;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileContext;
use crate::file::FileId;
use crate::specifiers::ChangesetId;
use crate::specifiers::ChangesetPrefixSpecifier;
use crate::specifiers::ChangesetSpecifier;
use crate::specifiers::ChangesetSpecifierPrefixResolution;
use crate::specifiers::HgChangesetId;
use crate::tree::TreeContext;
use crate::tree::TreeId;
use crate::xrepo::CandidateSelectionHintArgs;

pub mod create_bookmark;
pub mod create_changeset;
pub mod delete_bookmark;
pub mod land_stack;
pub mod move_bookmark;
pub mod set_git_mapping;

define_stats! {
    prefix = "mononoke.api";
    staleness: dynamic_singleton_counter(
        "staleness.secs.{}.{}",
        (repoid: ::mononoke_types::RepositoryId, bookmark: String)
    ),
    missing_from_cache: dynamic_singleton_counter(
        "missing_from_cache.{}.{}",
        (repoid: ::mononoke_types::RepositoryId, bookmark: String)
    ),
    missing_from_repo: dynamic_singleton_counter(
        "missing_from_repo.{}.{}",
        (repoid: ::mononoke_types::RepositoryId, bookmark: String)
    ),
}

#[facet::container]
pub struct Repo {
    #[delegate(
        RepoBlobstore,
        RepoBookmarkAttrs,
        RepoDerivedData,
        RepoIdentity,
        dyn BonsaiGitMapping,
        dyn BonsaiGlobalrevMapping,
        dyn BonsaiHgMapping,
        dyn BookmarkUpdateLog,
        dyn Bookmarks,
        dyn ChangesetFetcher,
        dyn Changesets,
        dyn Phases,
        dyn PushrebaseMutationMapping,
        dyn HgMutationStore,
        dyn MutableCounters,
        dyn RepoPermissionChecker,
        dyn RepoLock,
        RepoConfig,
        SkiplistIndex,
        dyn SegmentedChangelog,
        RepoEphemeralStore,
        MutableRenames,
        RepoCrossRepo,
        dyn AclRegions,
        RepoSparseProfiles,
        StreamingClone,
    )]
    pub inner: InnerRepo,

    #[init(inner.repo_identity().name().to_string())]
    pub name: String,

    #[facet]
    pub warm_bookmarks_cache: dyn BookmarksCache,

    #[facet]
    pub hook_manager: HookManager,

    #[facet]
    pub repo_handler_base: RepoHandlerBase,
}

impl AsBlobRepo for Repo {
    fn as_blob_repo(&self) -> &BlobRepo {
        self.inner.as_blob_repo()
    }
}

#[derive(Clone)]
pub struct RepoContext {
    ctx: CoreContext,
    authz: Arc<AuthorizationContext>,
    repo: Arc<Repo>,
    push_redirector: Option<Arc<PushRedirector<Repo>>>,
}

impl fmt::Debug for RepoContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RepoContext(repo={:?})", self.name())
    }
}

pub struct RepoContextBuilder {
    ctx: CoreContext,
    authz: Option<AuthorizationContext>,
    repo: Arc<Repo>,
    push_redirector: Option<Arc<PushRedirector<Repo>>>,
    bubble_id: Option<BubbleId>,
}

async fn maybe_push_redirector(
    ctx: &CoreContext,
    repo: &Arc<Repo>,
    repos: &MononokeRepos<Repo>,
) -> Result<Option<PushRedirector<Repo>>, MononokeError> {
    if tunables().get_disable_scs_pushredirect() {
        return Ok(None);
    }
    let base = match repo.repo_handler_base().maybe_push_redirector_base.as_ref() {
        None => return Ok(None),
        Some(base) => base,
    };
    let live_commit_sync_config = repo.live_commit_sync_config();
    let enabled = live_commit_sync_config.push_redirector_enabled_for_public(repo.repoid());
    if enabled {
        let large_repo_id = base.common_commit_sync_config.large_repo_id;
        let large_repo = repos.get_by_id(large_repo_id.id()).ok_or_else(|| {
            MononokeError::InvalidRequest(format!("Large repo '{}' not found", large_repo_id))
        })?;
        Ok(Some(
            PushRedirectorArgs::new(
                large_repo,
                repo.clone(),
                base.synced_commit_mapping.clone(),
                base.target_repo_dbs.clone(),
            )
            .into_push_redirector(
                ctx,
                live_commit_sync_config,
                repo.inner_repo().repo_cross_repo.sync_lease().clone(),
            )
            .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?,
        ))
    } else {
        Ok(None)
    }
}

impl RepoContextBuilder {
    pub(crate) async fn new(
        ctx: CoreContext,
        repo: Arc<Repo>,
        repos: &MononokeRepos<Repo>,
    ) -> Result<Self, MononokeError> {
        let push_redirector = maybe_push_redirector(&ctx, &repo, repos)
            .await?
            .map(Arc::new);
        Ok(RepoContextBuilder {
            ctx,
            authz: None,
            repo,
            push_redirector,
            bubble_id: None,
        })
    }

    pub async fn with_bubble<F, R>(mut self, bubble_fetcher: F) -> Result<Self, MononokeError>
    where
        F: FnOnce(RepoEphemeralStore) -> R,
        R: Future<Output = anyhow::Result<Option<BubbleId>>>,
    {
        self.bubble_id = bubble_fetcher(self.repo.repo_ephemeral_store().clone()).await?;
        Ok(self)
    }

    pub fn with_authorization_context(mut self, authz: AuthorizationContext) -> Self {
        self.authz = Some(authz);
        self
    }

    pub async fn build(self) -> Result<RepoContext, MononokeError> {
        let authz = Arc::new(
            self.authz
                .unwrap_or_else(|| AuthorizationContext::new(&self.ctx)),
        );
        RepoContext::new(
            self.ctx,
            authz,
            self.repo,
            self.bubble_id,
            self.push_redirector,
        )
        .await
    }
}

pub async fn open_synced_commit_mapping(
    fb: FacebookInit,
    config: RepoConfig,
    mysql_options: &MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> Result<Arc<SqlSyncedCommitMapping>, Error> {
    let sql_factory = make_metadata_sql_factory(
        fb,
        config.storage_config.metadata,
        mysql_options.clone(),
        readonly_storage,
    )
    .await?;

    Ok(Arc::new(sql_factory.open::<SqlSyncedCommitMapping>()?))
}

impl Repo {
    /// Construct a new Repo based on an existing one with a bubble opened.
    pub fn with_bubble(&self, bubble: Bubble) -> Self {
        let blob_repo = self.inner.blob_repo.with_bubble(bubble);
        let inner = InnerRepo {
            blob_repo,
            ..self.inner.clone()
        };
        Self {
            name: self.name.clone(),
            inner,
            warm_bookmarks_cache: self.warm_bookmarks_cache.clone(),
            hook_manager: self.hook_manager.clone(),
            repo_handler_base: self.repo_handler_base.clone(),
        }
    }

    /// Construct a Repo from a test BlobRepo
    pub async fn new_test(ctx: CoreContext, blob_repo: BlobRepo) -> Result<Self, Error> {
        Self::new_test_common(
            ctx,
            blob_repo,
            None,
            Arc::new(SqlSyncedCommitMapping::with_sqlite_in_memory()?),
            Default::default(),
        )
        .await
    }

    /// Construct a Repo from a test BlobRepo and LFS config
    pub async fn new_test_lfs(
        ctx: CoreContext,
        blob_repo: BlobRepo,
        lfs: LfsParams,
    ) -> Result<Self, Error> {
        Self::new_test_common(
            ctx,
            blob_repo,
            None,
            Arc::new(SqlSyncedCommitMapping::with_sqlite_in_memory()?),
            lfs,
        )
        .await
    }

    /// Construct a Repo from a test BlobRepo and commit_sync_config
    pub async fn new_test_xrepo(
        ctx: CoreContext,
        blob_repo: BlobRepo,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
        synced_commit_mapping: ArcSyncedCommitMapping,
    ) -> Result<Self, Error> {
        Self::new_test_common(
            ctx,
            blob_repo,
            Some(live_commit_sync_config),
            synced_commit_mapping,
            Default::default(),
        )
        .await
    }

    /// Construct a Repo from a test BlobRepo and commit_sync_config
    async fn new_test_common(
        ctx: CoreContext,
        blob_repo: BlobRepo,
        live_commit_sync_config: Option<Arc<dyn LiveCommitSyncConfig>>,
        synced_commit_mapping: ArcSyncedCommitMapping,
        lfs: LfsParams,
    ) -> Result<Self, Error> {
        // TODO: Migrate more of this code to use the TestRepoFactory so that we can eventually
        // replace these test methods.
        let repo_factory: TestRepoFactory = TestRepoFactory::new(ctx.fb)?;

        let repo_id = blob_repo.get_repoid();

        let config = RepoConfig {
            lfs,
            infinitepush: InfinitepushParams {
                namespace: Some(InfinitepushNamespace::new(
                    Regex::new("scratch/.+").unwrap(),
                )),
                ..Default::default()
            },
            source_control_service: SourceControlServiceParams {
                permit_writes: true,
                ..Default::default()
            },
            hook_manager_params: Some(HookManagerParams {
                disable_acl_checker: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        let name = blob_repo.name().clone();
        let repo_blobstore = blob_repo.repo_blobstore_arc();
        let hook_manager = repo_factory.hook_manager(
            &blob_repo.repo_identity_arc(),
            &blob_repo.repo_derived_data_arc(),
            &blob_repo.bookmarks_arc(),
            &blob_repo.repo_blobstore_arc(),
        );
        let repo_cross_repo = Arc::new(RepoCrossRepo::new(
            synced_commit_mapping,
            live_commit_sync_config
                .unwrap_or_else(|| Arc::new(TestLiveCommitSyncConfig::new_empty())),
            Arc::new(InProcessLease::new()),
        ));
        let mutable_counters = repo_factory.mutable_counters(&blob_repo.repo_identity_arc())?;
        let repo_handler_base = repo_factory.repo_handler_base(
            &Arc::new(config.clone()),
            &repo_cross_repo,
            &blob_repo.repo_identity_arc(),
            blob_repo.bookmarks(),
            blob_repo.bookmark_update_log(),
            &mutable_counters,
        )?;

        let inner = InnerRepo {
            blob_repo,
            repo_config: Arc::new(config.clone()),
            skiplist_index: Arc::new(SkiplistIndex::new()),
            segmented_changelog: Arc::new(DisabledSegmentedChangelog::new()),
            ephemeral_store: Arc::new(RepoEphemeralStore::disabled(repo_id)),
            mutable_renames: Arc::new(MutableRenames::new_test(
                repo_id,
                SqlMutableRenamesStore::with_sqlite_in_memory()?,
            )),
            repo_cross_repo,
            acl_regions: build_disabled_acl_regions(),
            sparse_profiles: Arc::new(RepoSparseProfiles::new(None)),
            streaming_clone: Arc::new(
                StreamingCloneBuilder::with_sqlite_in_memory()?.build(repo_id, repo_blobstore),
            ),
        };

        let mut warm_bookmarks_cache_builder = WarmBookmarksCacheBuilder::new(
            ctx.clone(),
            inner.bookmarks_arc(),
            inner.bookmark_update_log_arc(),
            inner.repo_identity_arc(),
        );
        warm_bookmarks_cache_builder
            .add_all_warmers(&inner.repo_derived_data_arc(), &inner.phases_arc())?;
        // We are constructing a test repo, so ensure the warm bookmark cache
        // is fully warmed, so that tests see up-to-date bookmarks.
        warm_bookmarks_cache_builder.wait_until_warmed();
        let warm_bookmarks_cache = warm_bookmarks_cache_builder.build().await?;

        Ok(Self {
            name: name.clone(),
            inner,
            warm_bookmarks_cache: Arc::new(warm_bookmarks_cache),
            hook_manager,
            repo_handler_base,
        })
    }

    /// The name of the underlying repo.
    pub fn name(&self) -> &String {
        &self.name
    }

    /// The internal id of the repo. Used for comparing the repo objects with each other.
    pub fn repoid(&self) -> RepositoryId {
        self.blob_repo().get_repoid()
    }

    /// The underlying `InnerRepo`.
    pub fn inner_repo(&self) -> &InnerRepo {
        &self.inner
    }

    /// The underlying `BlobRepo`.
    pub fn blob_repo(&self) -> &BlobRepo {
        &self.inner.blob_repo
    }

    /// `LiveCommitSyncConfig` instance to query current state of sync configs.
    pub fn live_commit_sync_config(&self) -> Arc<dyn LiveCommitSyncConfig> {
        self.inner.repo_cross_repo.live_commit_sync_config().clone()
    }

    /// The commit sync mapping for the referenced repository.
    pub fn synced_commit_mapping(&self) -> &ArcSyncedCommitMapping {
        self.inner.repo_cross_repo.synced_commit_mapping()
    }

    /// The commit sync lease for the referenced repository.
    pub fn x_repo_sync_lease(&self) -> &Arc<dyn LeaseOps> {
        self.inner.repo_cross_repo.sync_lease()
    }

    /// The warm bookmarks cache for the referenced repository.
    pub fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache + Send + Sync> {
        &self.warm_bookmarks_cache
    }

    /// The configuration for the referenced repository.
    pub fn config(&self) -> &RepoConfig {
        &self.inner.repo_config
    }

    pub async fn report_monitoring_stats(&self, ctx: &CoreContext) -> Result<(), MononokeError> {
        match self.config().source_control_service_monitoring.as_ref() {
            None => {}
            Some(monitoring_config) => {
                for bookmark in monitoring_config.bookmarks_to_report_age.iter() {
                    self.report_bookmark_age_difference(ctx, bookmark).await?;
                }
            }
        }

        Ok(())
    }

    fn report_bookmark_missing_from_cache(&self, ctx: &CoreContext, bookmark: &BookmarkName) {
        error!(
            ctx.logger(),
            "Monitored bookmark does not exist in the cache: {}, repo: {}",
            bookmark,
            self.repo_identity().name()
        );

        STATS::missing_from_cache.set_value(
            ctx.fb,
            1,
            (self.repo_identity().id(), bookmark.to_string()),
        );
    }

    fn report_bookmark_missing_from_repo(&self, ctx: &CoreContext, bookmark: &BookmarkName) {
        error!(
            ctx.logger(),
            "Monitored bookmark does not exist in the repo: {}", bookmark
        );

        STATS::missing_from_repo.set_value(
            ctx.fb,
            1,
            (self.repo_identity().id(), bookmark.to_string()),
        );
    }

    fn report_bookmark_staleness(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
        staleness: i64,
    ) {
        // Don't log if staleness is 0 to make output less spammy
        if staleness > 0 {
            debug!(
                ctx.logger(),
                "Reporting staleness of {} in repo {} to be {}s",
                bookmark,
                self.repo_identity().id(),
                staleness
            );
        }

        STATS::staleness.set_value(
            ctx.fb,
            staleness,
            (self.repo_identity().id(), bookmark.to_string()),
        );
    }

    async fn report_bookmark_age_difference(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
    ) -> Result<(), MononokeError> {
        let repo = self.blob_repo();

        let maybe_bcs_id_from_service = self.warm_bookmarks_cache.get(ctx, bookmark).await?;
        let maybe_bcs_id_from_blobrepo = repo.bookmarks().get(ctx.clone(), bookmark).await?;

        if maybe_bcs_id_from_blobrepo.is_none() {
            self.report_bookmark_missing_from_repo(ctx, bookmark);
        }

        if maybe_bcs_id_from_service.is_none() {
            self.report_bookmark_missing_from_cache(ctx, bookmark);
        }

        if let (Some(service_bcs_id), Some(blobrepo_bcs_id)) =
            (maybe_bcs_id_from_service, maybe_bcs_id_from_blobrepo)
        {
            // We report the difference between current time (i.e. SystemTime::now())
            // and timestamp of the first child of bookmark value from cache (see graph below)
            //
            //       O <- bookmark value from blobrepo
            //       |
            //      ...
            //       |
            //       O <- first child of bookmark value from cache.
            //       |
            //       O <- bookmark value from cache, it's outdated
            //
            // This way of reporting shows for how long the oldest commit not in cache hasn't been
            // imported, and it should work correctly both for high and low commit rates.

            // Do not log if there's no lag to make output less spammy
            if blobrepo_bcs_id != service_bcs_id {
                debug!(
                    ctx.logger(),
                    "Reporting bookmark age difference for {}: latest {} value is {}, cache points to {}",
                    repo.get_repoid(),
                    bookmark,
                    blobrepo_bcs_id,
                    service_bcs_id,
                );
            }

            let difference = if blobrepo_bcs_id == service_bcs_id {
                0
            } else {
                let limit = 100;
                let maybe_child = self
                    .try_find_child(ctx, service_bcs_id, blobrepo_bcs_id, limit)
                    .await?;

                // If we can't find a child of a bookmark value from cache, then it might mean
                // that either cache is too far behind or there was a non-forward bookmark move.
                // Either way, we can't really do much about it here, so let's just find difference
                // between current timestamp and bookmark value from cache.
                let compare_bcs_id = maybe_child.unwrap_or(service_bcs_id);

                let compare_timestamp = compare_bcs_id
                    .load(ctx, repo.blobstore())
                    .await?
                    .author_date()
                    .timestamp_secs();

                let current_timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(Error::from)?;
                let current_timestamp = current_timestamp.as_secs() as i64;
                current_timestamp - compare_timestamp
            };
            self.report_bookmark_staleness(ctx, bookmark, difference);
        }

        Ok(())
    }

    /// Try to find a changeset that's ancestor of `descendant` and direct child of
    /// `ancestor`. Returns None if this commit doesn't exist (for example if `ancestor` is not
    /// actually an ancestor of `descendant`) or if child is too far away from descendant.
    async fn try_find_child(
        &self,
        ctx: &CoreContext,
        ancestor: ChangesetId,
        descendant: ChangesetId,
        limit: u64,
    ) -> Result<Option<ChangesetId>, Error> {
        // This is a generation number beyond which we don't need to traverse
        let min_gen_num = self.fetch_gen_num(ctx, &ancestor).await?;

        let mut ancestors = AncestorsNodeStream::new(
            ctx.clone(),
            &self.blob_repo().get_changeset_fetcher(),
            descendant,
        )
        .compat();

        let mut traversed = 0;
        while let Some(cs_id) = ancestors.next().await {
            traversed += 1;
            if traversed > limit {
                return Ok(None);
            }

            let cs_id = cs_id?;
            let parents = self
                .blob_repo()
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .await?;

            if parents.contains(&ancestor) {
                return Ok(Some(cs_id));
            } else {
                let gen_num = self.fetch_gen_num(ctx, &cs_id).await?;
                if gen_num < min_gen_num {
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    async fn fetch_gen_num(
        &self,
        ctx: &CoreContext,
        cs_id: &ChangesetId,
    ) -> Result<Generation, Error> {
        let maybe_gen_num = self
            .blob_repo()
            .get_generation_number(ctx.clone(), *cs_id)
            .await?;
        maybe_gen_num.ok_or_else(|| format_err!("gen num for {} not found", cs_id))
    }
}

#[derive(Default)]
pub struct Stack {
    pub draft: Vec<ChangesetId>,
    pub public: Vec<ChangesetId>,
    pub leftover_heads: Vec<ChangesetId>,
}

pub struct BookmarkInfo {
    pub warm_changeset: ChangesetContext,
    pub fresh_changeset: ChangesetContext,
    pub last_update_timestamp: Timestamp,
}

/// A context object representing a query to a particular repo.
impl RepoContext {
    pub async fn new(
        ctx: CoreContext,
        authz: Arc<AuthorizationContext>,
        repo: Arc<Repo>,
        bubble_id: Option<BubbleId>,
        push_redirector: Option<Arc<PushRedirector<Repo>>>,
    ) -> Result<Self, MononokeError> {
        let ctx = ctx.with_mutated_scuba(|mut scuba| {
            scuba.add("permissions_model", format!("{:?}", authz));
            scuba
        });

        // Check the user is permitted to access this repo.
        authz.require_repo_metadata_read(&ctx, &repo.inner).await?;

        // Open the bubble if necessary.
        let repo = if let Some(bubble_id) = bubble_id {
            let bubble = repo.repo_ephemeral_store().open_bubble(bubble_id).await?;
            Arc::new(repo.with_bubble(bubble))
        } else {
            repo
        };

        Ok(Self {
            ctx,
            authz,
            repo,
            push_redirector,
        })
    }

    pub async fn new_test(ctx: CoreContext, repo: Arc<Repo>) -> Result<Self, MononokeError> {
        let authz = Arc::new(AuthorizationContext::new_bypass_access_control());
        RepoContext::new(ctx, authz, repo, None, None).await
    }

    /// The context for this query.
    pub fn ctx(&self) -> &CoreContext {
        &self.ctx
    }

    /// The name of the underlying repo.
    pub fn name(&self) -> &str {
        self.repo.name()
    }

    /// The internal id of the repo. Used for comparing the repo objects with each other.
    pub fn repoid(&self) -> RepositoryId {
        self.repo.repoid()
    }

    /// The authorization context of the request.
    pub fn authorization_context(&self) -> &AuthorizationContext {
        &self.authz
    }

    pub fn mononoke_api_repo(&self) -> Arc<Repo> {
        self.repo.clone()
    }

    /// The underlying `InnerRepo`.
    pub fn inner_repo(&self) -> &InnerRepo {
        self.repo.inner_repo()
    }

    /// The underlying `BlobRepo`.
    pub fn blob_repo(&self) -> &BlobRepo {
        self.repo.blob_repo()
    }

    /// `LiveCommitSyncConfig` instance to query current state of sync configs.
    pub fn live_commit_sync_config(&self) -> Arc<dyn LiveCommitSyncConfig> {
        self.repo.live_commit_sync_config()
    }

    /// The skiplist index for the referenced repository.
    pub fn skiplist_index_arc(&self) -> ArcSkiplistIndex {
        self.repo.skiplist_index_arc()
    }

    /// The ephemeral store for the referenced repository
    pub fn repo_ephemeral_store_arc(&self) -> ArcRepoEphemeralStore {
        self.repo.repo_ephemeral_store_arc()
    }

    /// The segmeneted changelog for the referenced repository.
    pub fn segmented_changelog(&self) -> &dyn SegmentedChangelog {
        self.repo.segmented_changelog()
    }

    /// The commit sync mapping for the referenced repository
    pub fn synced_commit_mapping(&self) -> &ArcSyncedCommitMapping {
        self.repo.synced_commit_mapping()
    }

    /// The warm bookmarks cache for the referenced repository.
    pub fn warm_bookmarks_cache(&self) -> &Arc<dyn BookmarksCache + Send + Sync> {
        self.repo.warm_bookmarks_cache()
    }

    /// The hook manager for the referenced repository.
    pub fn hook_manager(&self) -> Arc<HookManager> {
        self.repo.hook_manager_arc()
    }

    /// The base for push redirection logic for this repo
    pub fn maybe_push_redirector_base(&self) -> Option<&PushRedirectorBase> {
        self.repo
            .repo_handler_base
            .maybe_push_redirector_base
            .as_ref()
            .map(AsRef::as_ref)
    }

    /// The configuration for the referenced repository.
    pub fn config(&self) -> &RepoConfig {
        self.repo.config()
    }

    pub fn mutable_renames(&self) -> ArcMutableRenames {
        self.repo.mutable_renames_arc()
    }

    pub fn sparse_profiles(&self) -> ArcRepoSparseProfiles {
        self.repo.repo_sparse_profiles_arc()
    }

    pub fn derive_changeset_info_enabled(&self) -> bool {
        self.blob_repo()
            .get_derived_data_config()
            .is_enabled(ChangesetInfo::NAME)
    }

    pub fn derive_hgchangesets_enabled(&self) -> bool {
        self.blob_repo()
            .get_derived_data_config()
            .is_enabled(MappedHgChangesetId::NAME)
    }

    /// Load bubble from id
    pub async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble, MononokeError> {
        Ok(self
            .repo
            .repo_ephemeral_store()
            .open_bubble(bubble_id)
            .await?)
    }

    // pub(crate) for testing
    pub(crate) async fn changesets(
        &self,
        bubble_id: Option<BubbleId>,
    ) -> Result<Arc<dyn Changesets>, MononokeError> {
        Ok(match bubble_id {
            Some(id) => Arc::new(self.open_bubble(id).await?.changesets(self.blob_repo())),
            None => self.blob_repo().changesets_arc(),
        })
    }

    /// Test whether a changeset exists in a particular storage location.
    pub async fn changeset_exists(
        &self,
        changeset_id: ChangesetId,
        storage_location: StorageLocation,
    ) -> Result<bool, MononokeError> {
        use StorageLocation::*;
        let bubble_id = match storage_location {
            Persistent => None,
            Bubble(id) => Some(id),
            UnknownBubble => match self
                .repo
                .repo_ephemeral_store()
                .bubble_from_changeset(&changeset_id)
                .await?
            {
                Some(id) => Some(id),
                None => return Ok(false),
            },
        };
        Ok(self
            .changesets(bubble_id)
            .await?
            .exists(&self.ctx, changeset_id)
            .await?)
    }

    /// Look up a changeset specifier to find the canonical bonsai changeset
    /// ID for a changeset.
    pub async fn resolve_specifier(
        &self,
        specifier: ChangesetSpecifier,
    ) -> Result<Option<ChangesetId>, MononokeError> {
        let id = match specifier {
            ChangesetSpecifier::Bonsai(cs_id) => self
                .changeset_exists(cs_id, StorageLocation::Persistent)
                .await?
                .then_some(cs_id),
            ChangesetSpecifier::EphemeralBonsai(cs_id, bubble_id) => self
                .changeset_exists(cs_id, StorageLocation::ephemeral(bubble_id))
                .await?
                .then_some(cs_id),
            ChangesetSpecifier::Hg(hg_cs_id) => {
                self.blob_repo()
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(&self.ctx, hg_cs_id)
                    .await?
            }
            ChangesetSpecifier::Globalrev(rev) => {
                self.blob_repo()
                    .bonsai_globalrev_mapping()
                    .get_bonsai_from_globalrev(&self.ctx, rev)
                    .await?
            }
            ChangesetSpecifier::Svnrev(rev) => {
                self.blob_repo()
                    .bonsai_svnrev_mapping()
                    .get_bonsai_from_svnrev(&self.ctx, rev)
                    .await?
            }
            ChangesetSpecifier::GitSha1(git_sha1) => {
                self.blob_repo()
                    .bonsai_git_mapping()
                    .get_bonsai_from_git_sha1(&self.ctx, git_sha1)
                    .await?
            }
        };
        Ok(id)
    }

    /// Resolve a bookmark to a changeset.
    pub async fn resolve_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        freshness: BookmarkFreshness,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        // a non ascii bookmark name is an invalid request
        let bookmark = BookmarkName::new(bookmark.as_ref())
            .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?;

        let mut cs_id = match freshness {
            BookmarkFreshness::MaybeStale => {
                self.warm_bookmarks_cache()
                    .get(&self.ctx, &bookmark)
                    .await?
            }
            BookmarkFreshness::MostRecent => None,
        };

        // If the bookmark wasn't found in the warm bookmarks cache, it might
        // be a scratch bookmark, so always do the look-up.
        if cs_id.is_none() {
            cs_id = self
                .blob_repo()
                .bookmarks()
                .get(self.ctx.clone(), &bookmark)
                .await?
        }

        Ok(cs_id.map(|cs_id| ChangesetContext::new(self.clone(), cs_id)))
    }

    /// Resolve a changeset id by its prefix
    pub async fn resolve_changeset_id_prefix(
        &self,
        prefix: ChangesetPrefixSpecifier,
    ) -> Result<ChangesetSpecifierPrefixResolution, MononokeError> {
        const MAX_LIMIT_AMBIGUOUS_IDS: usize = 10;
        let resolved = match prefix {
            ChangesetPrefixSpecifier::Hg(prefix) => ChangesetSpecifierPrefixResolution::from(
                self.blob_repo()
                    .bonsai_hg_mapping()
                    .get_many_hg_by_prefix(&self.ctx, prefix, MAX_LIMIT_AMBIGUOUS_IDS)
                    .await?,
            ),
            ChangesetPrefixSpecifier::Bonsai(prefix) => ChangesetSpecifierPrefixResolution::from(
                self.blob_repo()
                    .changesets()
                    .get_many_by_prefix(self.ctx.clone(), prefix, MAX_LIMIT_AMBIGUOUS_IDS)
                    .await?,
            ),
            ChangesetPrefixSpecifier::Globalrev(prefix) => {
                ChangesetSpecifierPrefixResolution::from(
                    self.blob_repo()
                        .bonsai_globalrev_mapping()
                        .get_closest_globalrev(&self.ctx, prefix)
                        .await?,
                )
            }
        };
        Ok(resolved)
    }

    /// Look up a changeset by specifier.
    pub async fn changeset(
        &self,
        specifier: impl Into<ChangesetSpecifier>,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let specifier = specifier.into();
        let changeset = self
            .resolve_specifier(specifier)
            .await?
            .map(|cs_id| ChangesetContext::new(self.clone(), cs_id));
        Ok(changeset)
    }

    pub fn difference_of_unions_of_ancestors(
        &self,
        includes: Vec<ChangesetId>,
        excludes: Vec<ChangesetId>,
    ) -> impl Stream<Item = Result<ChangesetContext, MononokeError>> {
        let repo = self.clone();
        DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
            self.ctx.clone(),
            self.blob_repo().changeset_fetcher(),
            self.skiplist_index_arc(),
            includes,
            excludes,
        )
        .compat()
        .map_ok(move |cs_id| ChangesetContext::new(repo.clone(), cs_id))
        .map_err(|err| err.into())
    }

    /// Get Mercurial ID for multiple changesets
    ///
    /// This is a more efficient version of:
    /// ```ignore
    /// let ids: Vec<ChangesetId> = ...;
    /// ids.into_iter().map(|id| {
    ///     let hg_id = repo
    ///         .changeset(ChangesetSpecifier::Bonsai(id))
    ///         .await
    ///         .hg_id();
    ///     (id, hg_id)
    /// });
    /// ```
    pub async fn many_changeset_hg_ids(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, HgChangesetId)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx.clone(), changesets)
            .await?
            .into_iter()
            .map(|(hg_cs_id, cs_id)| (cs_id, hg_cs_id))
            .collect();
        Ok(mapping)
    }

    /// Get changeset ID from Mercurial ID for multiple changesets
    pub async fn many_changeset_ids_from_hg(
        &self,
        changesets: Vec<HgChangesetId>,
    ) -> Result<Vec<(HgChangesetId, ChangesetId)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx.clone(), changesets)
            .await?;
        Ok(mapping)
    }

    /// Similar to many_changeset_hg_ids, but returning Git-SHA1s.
    pub async fn many_changeset_git_sha1s(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, GitSha1)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .bonsai_git_mapping()
            .get(&self.ctx, changesets.into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.git_sha1))
            .collect();
        Ok(mapping)
    }

    /// Get changeset ID from Git-SHA1 for multiple changesets
    pub async fn many_changeset_ids_from_git_sha1(
        &self,
        changesets: Vec<GitSha1>,
    ) -> Result<Vec<(GitSha1, ChangesetId)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .bonsai_git_mapping()
            .get(&self.ctx, changesets.into())
            .await?
            .into_iter()
            .map(|entry| (entry.git_sha1, entry.bcs_id))
            .collect();
        Ok(mapping)
    }

    /// Similar to many_changeset_hg_ids, but returning Globalrevs.
    pub async fn many_changeset_globalrev_ids(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, Globalrev)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .bonsai_globalrev_mapping()
            .get(&self.ctx, changesets.into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.globalrev))
            .collect();
        Ok(mapping)
    }

    /// Get changeset ID from Globalrev for multiple changesets
    pub async fn many_changeset_ids_from_globalrev(
        &self,
        changesets: Vec<Globalrev>,
    ) -> Result<Vec<(Globalrev, ChangesetId)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .bonsai_globalrev_mapping()
            .get(&self.ctx, changesets.into())
            .await?
            .into_iter()
            .map(|entry| (entry.globalrev, entry.bcs_id))
            .collect();
        Ok(mapping)
    }

    /// Similar to many_changeset_hg_ids, but returning Svnrevs.
    pub async fn many_changeset_svnrev_ids(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, Svnrev)>, MononokeError> {
        let mapping = self
            .blob_repo()
            .bonsai_svnrev_mapping()
            .get(&self.ctx, changesets.into())
            .await?
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.svnrev))
            .collect();
        Ok(mapping)
    }

    pub async fn many_changeset_parents(
        &self,
        changesets: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, MononokeError> {
        let parents = self
            .blob_repo()
            .changesets()
            .get_many(self.ctx.clone(), changesets)
            .await?
            .into_iter()
            .map(|entry| (entry.cs_id, entry.parents))
            .collect();
        Ok(parents)
    }

    /// Return comprehensive bookmark info including last update time
    /// Currently works only for public bookmarks.
    pub async fn bookmark_info(
        &self,
        bookmark: impl AsRef<str>,
    ) -> Result<Option<BookmarkInfo>, MononokeError> {
        // a non ascii bookmark name is an invalid request
        let bookmark = BookmarkName::new(bookmark.as_ref())
            .map_err(|e| MononokeError::InvalidRequest(e.to_string()))?;

        let (maybe_warm_cs_id, maybe_log_entry) = try_join!(
            self.warm_bookmarks_cache().get(&self.ctx, &bookmark),
            async {
                let mut entries_stream = self
                    .repo
                    .blob_repo()
                    .bookmark_update_log()
                    .list_bookmark_log_entries(
                        self.ctx.clone(),
                        bookmark.clone(),
                        1,
                        None,
                        Freshness::MaybeStale,
                    );
                entries_stream.next().await.transpose()
            }
        )?;

        let maybe_warm_changeset =
            maybe_warm_cs_id.map(|cs_id| ChangesetContext::new(self.clone(), cs_id));

        let (_id, maybe_fresh_cs_id, _reason, timestamp) = maybe_log_entry
            .ok_or_else(|| anyhow!("Bookmark update log has no entries for queried bookmark!"))?;

        let fresh_cs_id = match maybe_fresh_cs_id {
            Some(cs_id) => cs_id,
            None => {
                return Ok(None);
            }
        };
        let fresh_changeset = ChangesetContext::new(self.clone(), fresh_cs_id);

        let last_update_timestamp = timestamp;

        // If the bookmark wasn't found in the warm bookmarks cache return
        // the fresh value for simplicity.
        let warm_changeset = maybe_warm_changeset.unwrap_or_else(|| fresh_changeset.clone());

        Ok(Some(BookmarkInfo {
            warm_changeset,
            fresh_changeset,
            last_update_timestamp,
        }))
    }

    /// Get a list of bookmarks.
    pub async fn list_bookmarks(
        &self,
        include_scratch: bool,
        prefix: Option<&str>,
        after: Option<&str>,
        limit: Option<u64>,
    ) -> Result<impl Stream<Item = Result<(String, ChangesetId), MononokeError>> + '_, MononokeError>
    {
        if include_scratch {
            if prefix.is_none() {
                return Err(MononokeError::InvalidRequest(
                    "prefix required to list scratch bookmarks".to_string(),
                ));
            }
            if limit.is_none() {
                return Err(MononokeError::InvalidRequest(
                    "limit required to list scratch bookmarks".to_string(),
                ));
            }
        }

        let prefix = match prefix {
            Some(prefix) => BookmarkPrefix::new(prefix).map_err(|e| {
                MononokeError::InvalidRequest(format!(
                    "invalid bookmark prefix '{}': {}",
                    prefix, e
                ))
            })?,
            None => BookmarkPrefix::empty(),
        };

        let pagination = match after {
            Some(after) => {
                let name = BookmarkName::new(after).map_err(|e| {
                    MononokeError::InvalidRequest(format!(
                        "invalid bookmark name '{}': {}",
                        after, e
                    ))
                })?;
                BookmarkPagination::After(name)
            }
            None => BookmarkPagination::FromStart,
        };

        if include_scratch {
            // Scratch bookmarks must be queried directly from the blobrepo as
            // they are not stored in the cache.  To maintain ordering with
            // public bookmarks, query all the bookmarks we are interested in.
            let blob_repo = self.blob_repo();
            let cache = self.warm_bookmarks_cache();
            let bookmarks = blob_repo
                .bookmarks()
                .list(
                    self.ctx.clone(),
                    BookmarkFreshness::MaybeStale,
                    &prefix,
                    BookmarkKind::ALL,
                    &pagination,
                    limit.unwrap_or(std::u64::MAX),
                )
                .try_filter_map(move |(bookmark, cs_id)| async move {
                    if bookmark.kind() == &BookmarkKind::Scratch {
                        Ok(Some((bookmark.into_name().into_string(), cs_id)))
                    } else {
                        // For non-scratch bookmarks, always return the value
                        // from the cache so that clients only ever see the
                        // warm value.  If the bookmark is newly created and
                        // has no warm value, this might mean we have to
                        // filter this bookmark out.
                        let bookmark_name = bookmark.into_name();
                        let maybe_cs_id = cache.get(&self.ctx, &bookmark_name).await?;
                        Ok(maybe_cs_id.map(|cs_id| (bookmark_name.into_string(), cs_id)))
                    }
                })
                .map_err(MononokeError::from)
                .boxed();
            Ok(bookmarks)
        } else {
            // Public bookmarks can be fetched from the warm bookmarks cache.
            let cache = self.warm_bookmarks_cache();
            Ok(
                stream::iter(cache.list(&self.ctx, &prefix, &pagination, limit).await?)
                    .map(|(bookmark, (cs_id, _kind))| Ok((bookmark.into_string(), cs_id)))
                    .boxed(),
            )
        }
    }

    /// Get a stack for the list of heads (up to the first public commit).
    ///
    /// Limit constrains the number of draft commits returned.
    /// Algo is designed to minimize number of db queries.
    /// Missing changesets are skipped.
    /// Changesets are returned in topological order (requested heads first)
    ///
    /// When the limit is reached returns the heads of which children were not processed to allow
    /// for continuation of the processing.
    pub async fn stack(
        &self,
        changesets: Vec<ChangesetId>,
        limit: usize,
    ) -> Result<Stack, MononokeError> {
        if limit == 0 {
            return Ok(Default::default());
        }

        // initialize visited
        let mut visited: HashSet<_> = changesets.iter().cloned().collect();

        let phases = self.blob_repo().phases();

        // get phases
        let public_phases = phases
            .get_public(&self.ctx, changesets.clone(), false)
            .await?;

        // partition
        let (mut public, mut draft): (Vec<_>, Vec<_>) = changesets
            .into_iter()
            .partition(|cs_id| public_phases.contains(cs_id));

        // initialize the queue
        let mut queue: Vec<_> = draft.to_vec();

        while !queue.is_empty() {
            // get the unique parents for all changesets in the queue & skip visited & update visited
            let parents: Vec<_> = self
                .blob_repo()
                .changesets()
                .get_many(self.ctx.clone(), queue.clone())
                .await?
                .into_iter()
                .flat_map(|cs_entry| cs_entry.parents)
                .filter(|cs_id| !visited.contains(cs_id))
                .unique()
                .collect();

            visited.extend(parents.iter().cloned());

            // get phases for the parents
            let public_phases = phases.get_public(&self.ctx, parents.clone(), false).await?;

            // partition
            let (new_public, new_draft): (Vec<_>, Vec<_>) = parents
                .into_iter()
                .partition(|cs_id| public_phases.contains(cs_id));

            // respect the limit
            if draft.len() + new_draft.len() > limit {
                break;
            }

            // update queue and level
            queue = new_draft.clone();

            // update draft & public
            public.extend(new_public.into_iter());
            draft.extend(new_draft.into_iter());
        }

        Ok(Stack {
            draft,
            public,
            leftover_heads: queue,
        })
    }

    /// Get a Tree by id.  Returns `None` if the tree doesn't exist.
    pub async fn tree(&self, tree_id: TreeId) -> Result<Option<TreeContext>, MononokeError> {
        TreeContext::new_check_exists(self.clone(), tree_id).await
    }

    /// Get a File by id.  Returns `None` if the file doesn't exist.
    pub async fn file(&self, file_id: FileId) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Canonical(file_id)).await
    }

    /// Get a File by content sha-1.  Returns `None` if the file doesn't exist.
    pub async fn file_by_content_sha1(
        &self,
        hash: Sha1,
    ) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Aliased(Alias::Sha1(hash))).await
    }

    /// Get a File by content sha-256.  Returns `None` if the file doesn't exist.
    pub async fn file_by_content_sha256(
        &self,
        hash: Sha256,
    ) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Aliased(Alias::Sha256(hash))).await
    }

    /// Get a File by content git-sha-1.  Returns `None` if the file doesn't exist.
    pub async fn file_by_content_gitsha1(
        &self,
        hash: GitSha1,
    ) -> Result<Option<FileContext>, MononokeError> {
        FileContext::new_check_exists(self.clone(), FetchKey::Aliased(Alias::GitSha1(hash))).await
    }

    fn get_target_repo_and_lca_hint(
        &self,
    ) -> (Target<BlobRepo>, Target<Arc<dyn LeastCommonAncestorsHint>>) {
        let blob_repo = self.blob_repo().clone();
        let lca_hint = self.repo.skiplist_index_arc();
        (Target(blob_repo), Target(lca_hint))
    }

    async fn build_candidate_selection_hint(
        &self,
        maybe_args: Option<CandidateSelectionHintArgs>,
        other_repo_context: &Self,
    ) -> Result<CandidateSelectionHint, MononokeError> {
        let args = match maybe_args {
            None => return Ok(CandidateSelectionHint::Only),
            Some(args) => args,
        };

        use CandidateSelectionHintArgs::*;
        match args {
            OnlyOrAncestorOfBookmark(bookmark) => {
                let (blob_repo, lca_hint) = other_repo_context.get_target_repo_and_lca_hint();
                Ok(CandidateSelectionHint::OnlyOrAncestorOfBookmark(
                    Target(bookmark),
                    blob_repo,
                    lca_hint,
                ))
            }
            OnlyOrDescendantOfBookmark(bookmark) => {
                let (blob_repo, lca_hint) = other_repo_context.get_target_repo_and_lca_hint();
                Ok(CandidateSelectionHint::OnlyOrDescendantOfBookmark(
                    Target(bookmark),
                    blob_repo,
                    lca_hint,
                ))
            }
            OnlyOrAncestorOfCommit(specifier) => {
                let (blob_repo, lca_hint) = other_repo_context.get_target_repo_and_lca_hint();
                let cs_id = other_repo_context
                    .resolve_specifier(specifier)
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "unknown commit specifier {}",
                            specifier
                        ))
                    })?;
                Ok(CandidateSelectionHint::OnlyOrAncestorOfCommit(
                    Target(cs_id),
                    blob_repo,
                    lca_hint,
                ))
            }
            OnlyOrDescendantOfCommit(specifier) => {
                let (blob_repo, lca_hint) = other_repo_context.get_target_repo_and_lca_hint();
                let cs_id = other_repo_context
                    .resolve_specifier(specifier)
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "unknown commit specifier {}",
                            specifier
                        ))
                    })?;
                Ok(CandidateSelectionHint::OnlyOrDescendantOfCommit(
                    Target(cs_id),
                    blob_repo,
                    lca_hint,
                ))
            }
            Exact(specifier) => {
                let cs_id = other_repo_context
                    .resolve_specifier(specifier)
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "unknown commit specifier {}",
                            specifier
                        ))
                    })?;
                Ok(CandidateSelectionHint::Exact(Target(cs_id)))
            }
        }
    }

    /// Get the equivalent changeset from another repo - it will sync it if needed
    pub async fn xrepo_commit_lookup(
        &self,
        other: &Self,
        specifier: impl Into<ChangesetSpecifier>,
        maybe_candidate_selection_hint_args: Option<CandidateSelectionHintArgs>,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let common_config = self
            .live_commit_sync_config()
            .get_common_config(self.blob_repo().get_repoid())
            .map_err(|e| {
                MononokeError::InvalidRequest(format!(
                    "Commits from {} are not configured to be remapped to another repo: {}",
                    self.repo.name, e
                ))
            })?;

        let candidate_selection_hint: CandidateSelectionHint = self
            .build_candidate_selection_hint(maybe_candidate_selection_hint_args, other)
            .await?;

        let commit_sync_repos = CommitSyncRepos::new(
            self.blob_repo().clone(),
            other.blob_repo().clone(),
            &common_config,
        )?;

        let specifier = specifier.into();
        let changeset = self.resolve_specifier(specifier).await?.ok_or_else(|| {
            MononokeError::InvalidRequest(format!("unknown commit specifier {}", specifier))
        })?;

        let commit_syncer = CommitSyncer::new(
            &self.ctx,
            self.synced_commit_mapping().clone(),
            commit_sync_repos,
            self.live_commit_sync_config(),
            self.repo.x_repo_sync_lease().clone(),
        );

        let maybe_cs_id = commit_syncer
            .sync_commit(
                &self.ctx,
                changeset,
                candidate_selection_hint,
                CommitSyncContext::ScsXrepoLookup,
                false,
            )
            .await?;
        Ok(maybe_cs_id.map(|cs_id| ChangesetContext::new(other.clone(), cs_id)))
    }

    /// Start a write to the repo.
    pub fn start_write(&self) -> Result<(), MononokeError> {
        if self.authz.is_service() {
            if !self.config().source_control_service.permit_service_writes {
                return Err(MononokeError::InvalidRequest(String::from(
                    "Service writes are disabled in configuration for this repo",
                )));
            }
        } else if !self.config().source_control_service.permit_writes {
            return Err(MononokeError::InvalidRequest(String::from(
                "Writes are disabled in configuration for this repo",
            )));
        }

        self.ctx
            .scuba()
            .clone()
            .log_with_msg("Write request start", None);

        Ok(())
    }

    /// Reads a value out of the underlying config, indicating if we support writes without parents in this repo.
    pub fn allow_no_parent_writes(&self) -> bool {
        return self
            .config()
            .source_control_service
            .permit_commits_without_parents;
    }

    /// A SegmentedChangelog client repository has a compressed shape of the commit graph but
    /// doesn't know the identifiers for all the commits in the graph. It only knows the
    /// identifiers for select commits called "known" commits. These repositories can query
    /// the server to get the identifiers of the commits they don't have using the location
    /// of the desired commit relative to one of the "known" commits.
    /// The current version has all parents of merge commits downloaded to clients so that
    /// locations can be expressed using only the unique descendant distance to one of these
    /// commits. The heads of the repo are also known.
    /// Let's assume our graph is `0 - a - b - c`.
    /// In this example our initial commit is `0`, then we have `a` the first commit, `b` second,
    /// `c` third.
    /// For `descendant = c` and `distance = 2` we want to return `a`.
    pub async fn location_to_changeset_id(
        &self,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>, MononokeError> {
        let segmented_changelog = self.repo.segmented_changelog();
        let ancestor = segmented_changelog
            .location_to_many_changeset_ids(&self.ctx, location, count)
            .await
            .map_err(MononokeError::from)?;
        Ok(ancestor)
    }

    /// A Segmented Changelog client needs to know how to translate between a commit hash,
    /// for example one that is provided by the user, and the information that it has locally,
    /// the shape of the graph, i.e. a location in the graph.
    pub async fn many_changeset_ids_to_locations(
        &self,
        master_heads: Vec<ChangesetId>,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Result<Location<ChangesetId>, MononokeError>>, MononokeError>
    {
        let segmented_changelog = self.repo.segmented_changelog();
        let result = segmented_changelog
            .many_changeset_ids_to_locations(&self.ctx, master_heads, cs_ids)
            .await
            .map(|ok| {
                ok.into_iter()
                    .map(|(k, v)| (k, v.map_err(Into::into)))
                    .collect::<HashMap<ChangesetId, Result<_, MononokeError>>>()
            })
            .map_err(MononokeError::from)?;
        Ok(result)
    }

    pub async fn segmented_changelog_clone_data(
        &self,
    ) -> Result<(CloneData<ChangesetId>, HashMap<ChangesetId, HgChangesetId>), MononokeError> {
        let segmented_changelog = self.repo.segmented_changelog();
        let clone_data = segmented_changelog
            .clone_data(&self.ctx)
            .await
            .map_err(MononokeError::from)?;
        Ok(clone_data)
    }

    pub async fn segmented_changelog_disabled(&self) -> Result<bool, MononokeError> {
        let segmented_changelog = self.repo.segmented_changelog();
        let disabled = segmented_changelog
            .disabled(&self.ctx)
            .await
            .map_err(MononokeError::from)?;
        Ok(disabled)
    }

    pub async fn segmented_changelog_pull_data(
        &self,
        common: Vec<ChangesetId>,
        missing: Vec<ChangesetId>,
    ) -> Result<CloneData<ChangesetId>, MononokeError> {
        let segmented_changelog = self.repo.segmented_changelog();
        let pull_data = segmented_changelog
            .pull_data(&self.ctx, common, missing)
            .await
            .map_err(MononokeError::from)?;
        Ok(pull_data)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use fixtures::Linear;
    use fixtures::MergeEven;
    use fixtures::TestRepoFixture;

    use super::*;

    #[fbinit::test]
    async fn test_try_find_child(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Repo::new_test(ctx.clone(), Linear::getrepo(fb).await).await?;

        let ancestor = ChangesetId::from_str(
            "c9f9a2a39195a583d523a4e5f6973443caeb0c66a315d5bf7db1b5775c725310",
        )?;
        let descendant = ChangesetId::from_str(
            "7785606eb1f26ff5722c831de402350cf97052dc44bc175da6ac0d715a3dbbf6",
        )?;

        let maybe_child = repo.try_find_child(&ctx, ancestor, descendant, 100).await?;
        let child = maybe_child.ok_or_else(|| format_err!("didn't find child"))?;
        assert_eq!(
            child,
            ChangesetId::from_str(
                "98ef3234c2f1acdbb272715e8cfef4a6378e5443120677e0d87d113571280f79"
            )?
        );

        let maybe_child = repo.try_find_child(&ctx, ancestor, descendant, 1).await?;
        assert!(maybe_child.is_none());

        Ok(())
    }

    #[fbinit::test]
    async fn test_try_find_child_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Repo::new_test(ctx.clone(), MergeEven::getrepo(fb).await).await?;

        let ancestor = ChangesetId::from_str(
            "35fb4e0fb3747b7ca4d18281d059be0860d12407dc5dce5e02fb99d1f6a79d2a",
        )?;
        let descendant = ChangesetId::from_str(
            "567a25d453cafaef6550de955c52b91bf9295faf38d67b6421d5d2e532e5adef",
        )?;

        let maybe_child = repo.try_find_child(&ctx, ancestor, descendant, 100).await?;
        let child = maybe_child.ok_or_else(|| format_err!("didn't find child"))?;
        assert_eq!(child, descendant);
        Ok(())
    }
}

impl PartialEq for RepoContext {
    fn eq(&self, other: &Self) -> bool {
        self.repoid() == other.repoid()
    }
}
impl Eq for RepoContext {}

impl Hash for RepoContext {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.repoid().hash(state);
    }
}
