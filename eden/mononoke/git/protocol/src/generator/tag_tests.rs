/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_tag_mapping::BonsaiTagMapping;
use bonsai_tag_mapping::BonsaiTagMappingEntry;
use bonsai_tag_mapping::Freshness;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Bookmarks;
use bookmarks_cache::BookmarksCache;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::StreamExt;
use git_ref_content_mapping::GitRefContentMapping;
use git_symbolic_refs::GitSymbolicRefs;
use git_types::CgdmChangesetDivider;
use git_types::CgdmChangesetDividerArc;
use gix_hash::ObjectId;
use metaconfig_types::RepoConfig;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use rustc_hash::FxHashSet;
use sql_ext::Transaction;

use super::tags_packfile_stream;
use crate::mapping::tagged_commits;
use crate::types::ChainBreakingMode;
use crate::types::DeltaInclusion;
use crate::types::FetchContainer;
use crate::types::FetchFilter;
use crate::types::PackfileConcurrency;
use crate::types::PackfileItemInclusion;
use crate::types::RefsSource;

#[facet::container]
struct TestRepo(
    RepoIdentity,
    RepoBlobstore,
    RepoDerivedData,
    dyn Bookmarks,
    dyn BonsaiGitMapping,
    dyn BonsaiTagMapping,
    dyn GitRefContentMapping,
    FilestoreConfig,
    dyn GitSymbolicRefs,
    CommitGraph,
    dyn CommitGraphWriter,
    dyn BookmarksCache,
    RepoConfig,
    dyn CgdmChangesetDivider,
);

struct ObservedBookmarks {
    entries: Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>,
    lists: AtomicUsize,
}

#[async_trait]
impl BookmarksCache for ObservedBookmarks {
    async fn get(&self, _ctx: &CoreContext, _key: &BookmarkKey) -> Result<Option<ChangesetId>> {
        anyhow::bail!("unexpected point lookup")
    }

    async fn list(
        &self,
        _ctx: &CoreContext,
        prefix: &BookmarkPrefix,
        pagination: &BookmarkPagination,
        limit: Option<u64>,
    ) -> Result<Vec<(BookmarkKey, (ChangesetId, BookmarkKind))>> {
        assert_eq!(prefix, &BookmarkPrefix::empty());
        assert!(matches!(pagination, BookmarkPagination::FromStart));
        assert_eq!(limit, None);
        self.lists.fetch_add(1, Ordering::Relaxed);
        Ok(self.entries.clone())
    }

    async fn sync(&self, _ctx: &CoreContext) {
        panic!("unexpected cache sync");
    }
}

struct ObservedTags {
    inner: Arc<dyn BonsaiTagMapping>,
    all_reads: AtomicUsize,
    hash_reads: AtomicUsize,
}

#[async_trait]
impl BonsaiTagMapping for ObservedTags {
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<BonsaiTagMappingEntry>> {
        self.all_reads.fetch_add(1, Ordering::Relaxed);
        self.inner.get_all_entries(ctx).await
    }

    async fn get_entries_by_changesets(
        &self,
        ctx: &CoreContext,
        ids: Vec<ChangesetId>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        self.inner.get_entries_by_changesets(ctx, ids).await
    }

    async fn get_entry_by_tag_name(
        &self,
        ctx: &CoreContext,
        name: String,
        freshness: Freshness,
    ) -> Result<Option<BonsaiTagMappingEntry>> {
        self.inner.get_entry_by_tag_name(ctx, name, freshness).await
    }

    async fn get_entries_by_tag_hashes(
        &self,
        ctx: &CoreContext,
        hashes: Vec<GitSha1>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        self.hash_reads.fetch_add(1, Ordering::Relaxed);
        self.inner.get_entries_by_tag_hashes(ctx, hashes).await
    }

    async fn get_entries_by_tag_names(
        &self,
        ctx: &CoreContext,
        names: Vec<String>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        self.inner.get_entries_by_tag_names(ctx, names).await
    }

    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<BonsaiTagMappingEntry>,
    ) -> Result<()> {
        self.inner.add_or_update_mappings(ctx, entries).await
    }

    async fn delete_mappings_by_name(&self, ctx: &CoreContext, names: Vec<String>) -> Result<()> {
        self.inner.delete_mappings_by_name(ctx, names).await
    }

    async fn add_or_update_mappings_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: Vec<BonsaiTagMappingEntry>,
        transaction: Transaction,
    ) -> Result<Transaction> {
        self.inner
            .add_or_update_mappings_in_transaction(ctx, entries, transaction)
            .await
    }

    async fn delete_mappings_by_name_in_transaction(
        &self,
        ctx: &CoreContext,
        names: Vec<String>,
        transaction: Transaction,
    ) -> Result<Transaction> {
        self.inner
            .delete_mappings_by_name_in_transaction(ctx, names, transaction)
            .await
    }
}

async fn test_repo(
    fb: FacebookInit,
    names: Vec<String>,
) -> Result<(TestRepo, Arc<ObservedBookmarks>, Arc<ObservedTags>)> {
    let mut repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let bookmarks = Arc::new(ObservedBookmarks {
        entries: names
            .into_iter()
            .map(|name| {
                Ok((
                    name.parse()?,
                    (ONES_CSID, BookmarkKind::PullDefaultPublishing),
                ))
            })
            .collect::<Result<_>>()?,
        lists: AtomicUsize::new(0),
    });
    let tags = Arc::new(ObservedTags {
        inner: repo.5.clone(),
        all_reads: AtomicUsize::new(0),
        hash_reads: AtomicUsize::new(0),
    });
    repo.5 = tags.clone();
    repo.11 = bookmarks.clone();
    Ok((repo, bookmarks, tags))
}

fn fetch_container(
    ctx: &CoreContext,
    repo: &TestRepo,
    include_tags: bool,
) -> Result<FetchContainer> {
    let filter = (!include_tags).then(|| FetchFilter {
        max_blob_size: u64::MAX,
        max_tree_depth: u64::MAX,
        allowed_object_types: vec![
            gix_object::Kind::Commit,
            gix_object::Kind::Tree,
            gix_object::Kind::Blob,
        ],
    });
    FetchContainer::new(
        Arc::new(ctx.clone()),
        repo,
        DeltaInclusion::Exclude,
        Arc::new(filter),
        PackfileConcurrency::new(1, 1, 1, 1, None),
        PackfileItemInclusion::FetchOnly,
        Arc::new(None),
        ChainBreakingMode::None,
        repo.cgdm_changeset_divider_arc(),
    )
}

#[mononoke::fbinit_test]
async fn object_only_fetch_skips_repository_metadata(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let oid = ObjectId::empty_blob(gix_hash::Kind::Sha1);
    let sha = GitSha1::from_object_id(oid.as_ref())?;
    for unrelated_refs in [0, 100, 10_000] {
        let names = (0..unrelated_refs)
            .map(|i| format!("heads/unrelated-{i}"))
            .collect();
        let (repo, bookmarks, tags) = test_repo(fb, names).await?;
        let mappings =
            tagged_commits(&ctx, &repo, vec![sha, sha], RefsSource::WarmBookmarksCache).await?;
        assert!(mappings.tagged_commits.is_empty());
        assert!(mappings.tag_names.is_empty());
        assert_eq!(mappings.non_tag_oids, vec![oid]);
        let (mut stream, count) = tags_packfile_stream(
            fetch_container(&ctx, &repo, true)?,
            &repo,
            mappings.tagged_commits,
            mappings.tag_names,
            RefsSource::WarmBookmarksCache,
            None,
        )
        .await?;
        assert_eq!(count, 0);
        assert!(stream.next().await.is_none());
        assert_eq!(tags.hash_reads.load(Ordering::Relaxed), 1);
        let bookmark_lists = bookmarks.lists.load(Ordering::Relaxed);
        let full_tag_reads = tags.all_reads.load(Ordering::Relaxed);
        eprintln!(
            "object-only fetch: unrelated_refs={unrelated_refs} bookmark_lists={bookmark_lists} full_tag_reads={full_tag_reads}"
        );
        assert_eq!(bookmark_lists, 0);
        assert_eq!(full_tag_reads, 0);
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn no_resolved_tags_preserves_non_tag_oids_for_each_refs_source(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let oid = ObjectId::empty_blob(gix_hash::Kind::Sha1);
    let sha = GitSha1::from_object_id(oid.as_ref())?;
    let (repo, _, _) = test_repo(fb, Vec::new()).await?;
    for source in [
        RefsSource::WarmBookmarksCache,
        RefsSource::DatabaseMaster,
        RefsSource::DatabaseFollower,
    ] {
        let mappings = tagged_commits(&ctx, &repo, vec![sha], source).await?;
        assert!(mappings.tagged_commits.is_empty());
        assert!(mappings.tag_names.is_empty());
        assert_eq!(mappings.non_tag_oids, vec![oid]);
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn resolved_tag_still_uses_bookmarks_with_mixed_object_wants(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let name = "custom/annotated-tag".to_owned();
    let (repo, bookmarks, tags) = test_repo(fb, vec![name.clone()]).await?;
    let tag = GitSha1::from_object_id(ObjectId::empty_tree(gix_hash::Kind::Sha1).as_ref())?;
    let blob_oid = ObjectId::empty_blob(gix_hash::Kind::Sha1);
    let blob = GitSha1::from_object_id(blob_oid.as_ref())?;
    tags.add_or_update_mappings(
        &ctx,
        vec![BonsaiTagMappingEntry::new(
            TWOS_CSID,
            name.clone(),
            tag,
            false,
        )],
    )
    .await?;
    let mappings =
        tagged_commits(&ctx, &repo, vec![tag, blob], RefsSource::WarmBookmarksCache).await?;
    assert_eq!(mappings.tagged_commits, vec![ONES_CSID]);
    assert_eq!(*mappings.tag_names, FxHashSet::from_iter([name]));
    assert_eq!(mappings.non_tag_oids, vec![blob_oid]);
    assert_eq!(bookmarks.lists.load(Ordering::Relaxed), 1);
    Ok(())
}

#[mononoke::fbinit_test]
async fn excluded_implicit_tags_skip_metadata_but_explicit_tags_remain(
    fb: FacebookInit,
) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let name = "custom/explicit-tag".to_owned();
    let (repo, bookmarks, tags) = test_repo(fb, vec![name.clone()]).await?;
    let tag = GitSha1::from_object_id(ObjectId::empty_tree(gix_hash::Kind::Sha1).as_ref())?;
    tags.add_or_update_mappings(
        &ctx,
        vec![BonsaiTagMappingEntry::new(
            TWOS_CSID,
            name.clone(),
            tag,
            false,
        )],
    )
    .await?;
    let (mut empty, count) = tags_packfile_stream(
        fetch_container(&ctx, &repo, false)?,
        &repo,
        vec![ONES_CSID],
        Arc::new(FxHashSet::default()),
        RefsSource::WarmBookmarksCache,
        None,
    )
    .await?;
    assert_eq!(count, 0);
    assert!(empty.next().await.is_none());
    assert_eq!(tags.all_reads.load(Ordering::Relaxed), 0);
    let (_explicit, count) = tags_packfile_stream(
        fetch_container(&ctx, &repo, false)?,
        &repo,
        Vec::new(),
        Arc::new(FxHashSet::from_iter([name])),
        RefsSource::WarmBookmarksCache,
        None,
    )
    .await?;
    assert_eq!(count, 1);
    assert_eq!(bookmarks.lists.load(Ordering::Relaxed), 0);
    assert_eq!(tags.all_reads.load(Ordering::Relaxed), 1);
    Ok(())
}

#[mononoke::fbinit_test]
async fn commits_without_matching_bookmarks_skip_tag_mapping(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, bookmarks, tags) = test_repo(fb, vec!["heads/other".to_owned()]).await?;
    let (mut stream, count) = tags_packfile_stream(
        fetch_container(&ctx, &repo, true)?,
        &repo,
        vec![TWOS_CSID],
        Arc::new(FxHashSet::default()),
        RefsSource::WarmBookmarksCache,
        None,
    )
    .await?;
    assert_eq!(count, 0);
    assert!(stream.next().await.is_none());
    assert_eq!(bookmarks.lists.load(Ordering::Relaxed), 1);
    assert_eq!(tags.all_reads.load(Ordering::Relaxed), 0);
    Ok(())
}

#[mononoke::fbinit_test]
async fn implicit_tags_for_requested_commits_are_preserved(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let name = "custom/implicit-tag".to_owned();
    let (repo, bookmarks, tags) = test_repo(fb, vec![name.clone()]).await?;
    let tag = GitSha1::from_object_id(ObjectId::empty_tree(gix_hash::Kind::Sha1).as_ref())?;
    tags.add_or_update_mappings(
        &ctx,
        vec![BonsaiTagMappingEntry::new(TWOS_CSID, name, tag, false)],
    )
    .await?;
    let (_stream, count) = tags_packfile_stream(
        fetch_container(&ctx, &repo, true)?,
        &repo,
        vec![ONES_CSID],
        Arc::new(FxHashSet::default()),
        RefsSource::WarmBookmarksCache,
        None,
    )
    .await?;
    assert_eq!(count, 1);
    assert_eq!(bookmarks.lists.load(Ordering::Relaxed), 1);
    assert_eq!(tags.all_reads.load(Ordering::Relaxed), 1);
    Ok(())
}
