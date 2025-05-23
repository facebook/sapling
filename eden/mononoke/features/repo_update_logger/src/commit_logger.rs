/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Loadable;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use borrowed::borrowed;
use chrono::DateTime;
use chrono::Utc;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use ephemeral_blobstore::BubbleId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use futures_ext::stream::FbStreamExt;
use futures_stats::TimedTryFutureExt;
use logger_ext::Loggable;
use metaconfig_types::RepoConfigRef;
#[cfg(fbcode_build)]
use mononoke_new_commit_rust_logger::MononokeNewCommitLogger;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use mononoke_types::Globalrev;
use once_cell::sync::Lazy;
use permission_checker::MononokeIdentitySet;
use phases::PhasesRef;
use regex::Regex;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use serde_derive::Serialize;
#[cfg(fbcode_build)]
use whence_logged::WhenceScribeLogged;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitInfo {
    changeset_id: ChangesetId,
    bubble_id: Option<NonZeroU64>,
    diff_id: Option<String>,
    changed_files_info: ChangedFilesInfo,
}

impl CommitInfo {
    pub fn new(bcs: &BonsaiChangeset, bubble_id: Option<BubbleId>) -> Self {
        CommitInfo {
            changeset_id: bcs.get_changeset_id(),
            bubble_id: bubble_id.map(Into::into),
            diff_id: extract_differential_revision(bcs.message()).map(ToString::to_string),
            changed_files_info: ChangedFilesInfo::new(bcs),
        }
    }

    pub fn update_changeset_id(
        &mut self,
        old_changeset_id: ChangesetId,
        new_changeset_id: ChangesetId,
    ) -> Result<()> {
        if self.changeset_id != old_changeset_id {
            return Err(anyhow!(
                concat!(
                    "programming error: attempting to update CommitInfo for incorrect changeset, ",
                    "expected {}, but modifying {}",
                ),
                old_changeset_id,
                self.changeset_id
            ));
        }
        self.changeset_id = new_changeset_id;
        Ok(())
    }

    pub fn changeset_id(&self) -> ChangesetId {
        self.changeset_id
    }
}

fn extract_differential_revision(message: &str) -> Option<&str> {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?m)^Differential Revision: [^\n]*/D([0-9]+)")
            .expect("Failed to compile differential revision regex")
    });

    Some(RE.captures(message)?.get(1)?.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChangedFilesInfo {
    changed_files_count: u64,
    changed_files_size: u64,
}

impl ChangedFilesInfo {
    pub fn new(bcs: &BonsaiChangeset) -> Self {
        let changed_files_count = bcs.file_changes_map().len() as u64;
        let changed_files_size = bcs
            .file_changes_map()
            .values()
            .map(|fc| fc.size().unwrap_or(0))
            .sum::<u64>();

        Self {
            changed_files_count,
            changed_files_size,
        }
    }
}

#[derive(Serialize)]
struct PlainCommitInfo {
    // Repo ID is logged to legacy scuba for compatibility, but should be
    // considered deprecated and not logged to Logger.
    repo_id: i32,
    repo_name: String,
    is_public: bool,
    changeset_id: ChangesetId,
    #[serde(skip_serializing_if = "Option::is_none")]
    bubble_id: Option<NonZeroU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff_id: Option<String>,
    changed_files_count: u64,
    changed_files_size: u64,
    parents: Vec<ChangesetId>,
    generation: Generation,
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_unix_name: Option<String>,
    #[serde(skip_serializing_if = "MononokeIdentitySet::is_empty")]
    user_identities: MononokeIdentitySet,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hostname: Option<String>,
    #[serde(with = "::chrono::serde::ts_seconds")]
    received_timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pusher_correlator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pusher_entry_point: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pusher_main_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    globalrev: Option<Globalrev>,
}

impl PlainCommitInfo {
    async fn new(
        ctx: &CoreContext,
        repo: &(impl BonsaiGlobalrevMappingRef + CommitGraphRef + RepoIdentityRef),
        received_timestamp: DateTime<Utc>,
        bookmark: Option<(&BookmarkKey, BookmarkKind)>,
        commit_info: CommitInfo,
    ) -> Result<PlainCommitInfo> {
        let CommitInfo {
            changeset_id,
            bubble_id,
            diff_id,
            changed_files_info:
                ChangedFilesInfo {
                    changed_files_count,
                    changed_files_size,
                },
        } = commit_info;
        let repo_id = repo.repo_identity().id().id();
        let repo_name = repo.repo_identity().name().to_string();
        let parents = repo
            .commit_graph()
            .changeset_parents(ctx, changeset_id)
            .await?
            .to_vec();
        let generation = repo
            .commit_graph()
            .changeset_generation(ctx, changeset_id)
            .await?;
        let globalrev = repo
            .bonsai_globalrev_mapping()
            .get_globalrev_from_bonsai(ctx, changeset_id)
            .await?;
        let user_unix_name = ctx.metadata().unix_name().map(|un| un.to_string());
        let user_identities = ctx.metadata().identities().clone();
        let source_hostname = ctx.metadata().client_hostname().map(|hn| hn.to_string());
        let (bookmark, is_public) = bookmark.map_or((None, false), |(name, kind)| {
            (Some(name.to_string()), kind.is_public())
        });

        let (mut pusher_correlator, mut pusher_entry_point, mut pusher_main_id) =
            (None, None, None);
        if let Some(cri) = ctx.client_request_info() {
            pusher_correlator = Some(cri.correlator.clone());
            pusher_entry_point = Some(cri.entry_point.to_string());
            pusher_main_id = cri.main_id.clone();
        }

        Ok(PlainCommitInfo {
            repo_id,
            repo_name,
            is_public,
            changeset_id,
            bubble_id,
            diff_id,
            changed_files_count,
            changed_files_size,
            parents,
            generation,
            bookmark,
            user_unix_name,
            user_identities,
            source_hostname,
            received_timestamp,
            pusher_correlator,
            pusher_entry_point,
            pusher_main_id,
            globalrev,
        })
    }
}

#[async_trait]
impl Loggable for PlainCommitInfo {
    #[cfg(fbcode_build)]
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()> {
        // Without override, WhenceScribeLogged is set to default which will cause
        // data being logged to "/sandbox" category if service is run from devserver.
        // But currently we use Logger only if we're in prod (as config implies), so
        // we should log to prod too, even from devserver.
        // For example, we can land a commit to prod from devserver, and logging for
        // this commit should go to prod, not to sandbox.
        MononokeNewCommitLogger::override_whence_scribe_logged(ctx.fb, WhenceScribeLogged::PROD);
        let mut logger = MononokeNewCommitLogger::new(ctx.fb);
        logger
            .set_repo_name(self.repo_name.clone())
            .set_is_public(self.is_public)
            .set_changeset_id(self.changeset_id.to_string())
            .set_parents(self.parents.iter().map(ToString::to_string).collect())
            .set_generation(self.generation.value() as i64)
            .set_changed_files_count(self.changed_files_count as i64)
            .set_changed_files_size(self.changed_files_size as i64)
            .set_pusher_identities(
                self.user_identities
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
            .set_received_timestamp(self.received_timestamp.timestamp());

        if let Some(bubble_id) = &self.bubble_id {
            logger.set_bubble_id(bubble_id.get() as i64);
        }
        if let Some(diff_id) = &self.diff_id {
            logger.set_diff_id(diff_id.clone());
        }
        if let Some(bookmark) = &self.bookmark {
            logger.set_bookmark_name(bookmark.to_string());
        }
        if let Some(source_hostname) = &self.source_hostname {
            logger.set_source_hostname(source_hostname.clone());
        }
        if let Some(correlator) = &self.pusher_correlator {
            logger.set_client_correlator(correlator.clone());
        }
        if let Some(entry_point) = &self.pusher_entry_point {
            logger.set_client_entry_point(entry_point.clone());
        }
        if let Some(main_id) = &self.pusher_main_id {
            logger.set_client_main_id(main_id.clone());
        }
        if let Some(globalrev) = &self.globalrev {
            logger.set_globalrev(globalrev.id() as i64);
        }

        logger.attach_raw_scribe_write_cat()?;
        logger.log_async()?;

        Ok(())
    }
}

pub async fn log_new_commits(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + BonsaiGlobalrevMappingRef + CommitGraphRef + RepoConfigRef),
    bookmark: Option<(&BookmarkKey, BookmarkKind)>,
    commit_infos: Vec<CommitInfo>,
) {
    let new_commit_logging_destination = repo
        .repo_config()
        .update_logging_config
        .new_commit_logging_destination
        .as_ref();

    // If nothing is going to be logged, we can exit early.
    if new_commit_logging_destination.is_none() {
        return;
    }

    let received_timestamp = Utc::now();

    let res = stream::iter(commit_infos)
        .map(Ok)
        .try_for_each_concurrent(100, |commit_info| async move {
            let plain_commit_info =
                PlainCommitInfo::new(ctx, repo, received_timestamp, bookmark, commit_info).await?;
            if let Some(new_commit_logging_destination) = new_commit_logging_destination {
                plain_commit_info
                    .log(ctx, new_commit_logging_destination)
                    .await;
            }
            anyhow::Ok(())
        })
        .await;

    if let Err(err) = res {
        ctx.scuba().clone().log_with_msg(
            "Failed to log new draft commit to scribe",
            Some(err.to_string()),
        );
    }
}

/// Helper function for finding all the newly public commit that should be logged after
/// the bookmark moves to to_cs_id. For public commits we allow them to be logged twice:
/// once when they're actually created, second time when they become public.
pub async fn find_draft_ancestors(
    ctx: &CoreContext,
    repo: &(
         impl RepoIdentityRef
         + RepoConfigRef
         + PhasesRef
         + BonsaiGlobalrevMappingRef
         + CommitGraphRef
         + RepoBlobstoreRef
         + std::marker::Sync
     ),
    to_cs_id: ChangesetId,
) -> Result<Vec<BonsaiChangeset>, Error> {
    ctx.scuba()
        .clone()
        .log_with_msg("Started finding draft ancestors", None);
    let (stats, drafts) = async move {
        let public_frontier: Vec<ChangesetId> = repo
            .commit_graph()
            .ancestors_frontier_with(ctx, vec![to_cs_id], |csid| {
                borrowed!(ctx, repo);
                async move {
                    Ok(repo
                        .phases()
                        .get_public(ctx, vec![csid], false)
                        .await?
                        .contains(&csid))
                }
            })
            .await?
            .into_iter()
            .collect();

        repo.commit_graph()
            .ancestors_difference_stream(ctx, vec![to_cs_id], public_frontier)
            .await?
            .yield_periodically()
            .map(move |res| async move {
                match res {
                    Ok(bcs_id) => Ok(bcs_id.load(ctx, repo.repo_blobstore()).await?),
                    Err(e) => Err(e),
                }
            })
            .buffered(1000)
            .boxed()
            .try_collect::<Vec<_>>()
            .await
    }
    .try_timed()
    .await?;

    ctx.scuba()
        .clone()
        .add_future_stats(&stats)
        .log_with_msg("Found draft ancestors", Some(format!("{}", drafts.len())));
    Ok(drafts)
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;

    use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use maplit::hashset;
    use metaconfig_types::RepoConfig;
    use mononoke_macros::mononoke;
    use phases::Phases;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;
    use tests_utils::bookmark;
    use tests_utils::drawdag::create_from_dag;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct Repo {
        #[facet]
        repo_identity: RepoIdentity,

        #[facet]
        repo_blobstore: RepoBlobstore,

        #[facet]
        repo_config: RepoConfig,

        #[facet]
        repo_derived_data: RepoDerivedData,

        #[facet]
        bookmarks: dyn Bookmarks,

        #[facet]
        bonsai_hg_mapping: dyn BonsaiHgMapping,

        #[facet]
        bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

        #[facet]
        phases: dyn Phases,

        #[facet]
        commit_graph: CommitGraph,

        #[facet]
        commit_graph_writer: dyn CommitGraphWriter,

        #[facet]
        filestore_config: FilestoreConfig,
    }

    #[mononoke::fbinit_test]
    async fn test_find_draft_ancestors_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(fb).await?;
        let mapping = create_from_dag(
            &ctx,
            &repo,
            r##"
            A-B-C-D
            "##,
        )
        .await?;

        let cs_id = mapping.get("A").unwrap();
        let to_cs_id = mapping.get("D").unwrap();
        bookmark(&ctx, &repo, "book").set_to(*cs_id).await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("B").unwrap(),
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );

        bookmark(&ctx, &repo, "book")
            .set_to(*mapping.get("B").unwrap())
            .await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_find_draft_ancestors_merge(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: Repo = test_repo_factory::build_empty(fb).await?;
        let mapping = create_from_dag(
            &ctx,
            &repo,
            r"
              B
             /  \
            A    D
             \  /
               C
            ",
        )
        .await?;

        let cs_id = mapping.get("B").unwrap();
        let to_cs_id = mapping.get("D").unwrap();
        bookmark(&ctx, &repo, "book").set_to(*cs_id).await?;
        let drafts = find_draft_ancestors(&ctx, &repo, *to_cs_id).await?;

        let drafts = drafts
            .into_iter()
            .map(|bcs| bcs.get_changeset_id())
            .collect::<HashSet<_>>();

        assert_eq!(
            drafts,
            hashset! {
                *mapping.get("C").unwrap(),
                *mapping.get("D").unwrap(),
            }
        );

        Ok(())
    }
}
