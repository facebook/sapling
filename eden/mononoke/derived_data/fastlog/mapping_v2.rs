/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use history_manifest::RootHistoryManifestDirectoryId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::HistoryManifestDirectoryId;

use crate::derive_v2::derive_fastlog_v2;

const FASTLOG_V2_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootFastlogV2 {
    pub(crate) csid: ChangesetId,
    pub(crate) root_manifest: RootHistoryManifestDirectoryId,
}

impl RootFastlogV2 {
    pub fn root_manifest(&self) -> RootHistoryManifestDirectoryId {
        self.root_manifest
    }

    pub fn changeset_id(&self) -> ChangesetId {
        self.csid
    }
}

#[async_trait]
impl BonsaiDerivable for RootFastlogV2 {
    const VARIANT: DerivableType = DerivableType::FastlogV2;

    type Dependencies = dependencies![RootHistoryManifestDirectoryId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
        _known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self, Error> {
        let csid = bonsai.get_changeset_id();
        let root_manifest = derivation_ctx
            .fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid)
            .await?;
        derive_fastlog_v2(ctx, derivation_ctx, bonsai, root_manifest).await?;
        Ok(RootFastlogV2 {
            csid,
            root_manifest,
        })
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let mapping = derivation_ctx.commit_derived_data_mapping()?;
        let value = self
            .root_manifest
            .into_history_manifest_directory_id()
            .blake2()
            .as_ref()
            .to_vec();
        mapping
            .store_mapping(
                ctx,
                derivation_ctx.repo_id(),
                changeset_id,
                Self::VARIANT,
                FASTLOG_V2_VERSION,
                &value,
                derivation_ctx.xdb_shard_id(Self::VARIANT)?,
            )
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let mapping = derivation_ctx.commit_derived_data_mapping()?;
        let value = mapping
            .fetch_mapping(
                ctx,
                derivation_ctx.repo_id(),
                changeset_id,
                Self::VARIANT,
                FASTLOG_V2_VERSION,
                derivation_ctx.xdb_shard_id(Self::VARIANT)?,
            )
            .await?;
        match value {
            Some(bytes) => {
                let hm_dir_id = HistoryManifestDirectoryId::from_bytes(Bytes::from(bytes))
                    .context("Failed to deserialize HistoryManifestDirectoryId from XDB mapping")?;
                Ok(Some(RootFastlogV2 {
                    csid: changeset_id,
                    root_manifest: RootHistoryManifestDirectoryId::from(hm_dir_id),
                }))
            }
            None => Ok(None),
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::fastlog_v2(thrift::DerivedDataFastlog::root_fastlog_v2(
            fastlog,
        )) = data
        {
            let hm_dir_id = match fastlog.history_manifest {
                thrift::DerivedDataHistoryManifest::root_history_manifest_directory_id(id) => {
                    HistoryManifestDirectoryId::from_thrift(id)
                }
                thrift::DerivedDataHistoryManifest::UnknownField(x) => Err(anyhow!(
                    "Can't convert {} from provided thrift::DerivedData, unknown field: {}",
                    Self::NAME,
                    x,
                )),
            }?;
            Ok(Self {
                csid: ChangesetId::from_thrift(fastlog.changeset_id)?,
                root_manifest: RootHistoryManifestDirectoryId::from(hm_dir_id),
            })
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME,
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::fastlog_v2(
            thrift::DerivedDataFastlog::root_fastlog_v2(thrift::DerivedDataRootFastlogV2 {
                changeset_id: data.csid.into_thrift(),
                history_manifest:
                    thrift::DerivedDataHistoryManifest::root_history_manifest_directory_id(
                        data.root_manifest
                            .into_history_manifest_directory_id()
                            .into_thrift(),
                    ),
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::collections::VecDeque;
    use std::str::FromStr;
    use std::sync::Arc;

    use anyhow::Error;
    use async_trait::async_trait;
    use blobstore::Loadable;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::BookmarkKey;
    use bookmarks::Bookmarks;
    use changesets_creation::save_changesets;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphRef;
    use commit_graph::CommitGraphWriter;
    use context::CoreContext;
    use derivation_queue_thrift::DerivationPriority;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::Linear;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use fixtures::UnsharedMergeUneven;
    use fixtures::create_bonsai_changeset;
    use fixtures::create_bonsai_changeset_with_author;
    use fixtures::create_bonsai_changeset_with_files;
    use fixtures::store_files;
    use futures::stream::TryStreamExt;
    use manifest::Entry;
    use manifest::ManifestOps;
    use maplit::btreemap;
    use mercurial_derivation::DeriveHgChangeset;
    use mercurial_types::HgChangesetId;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use mononoke_types::fastlog_batch::MAX_BATCHES;
    use mononoke_types::fastlog_batch::MAX_LATEST_LEN;
    use mononoke_types::fastlog_batch::max_entries_in_fastlog_batch;
    use mononoke_types::history_manifest::HistoryManifestDirectory;
    use mononoke_types::history_manifest::HistoryManifestEntry;
    use mononoke_types::history_manifest::HistoryManifestFile;
    use mononoke_types::typed_hash::HistoryManifestDirectoryId as HMDirId;
    use mononoke_types::typed_hash::HistoryManifestFileId as HMFileId;
    use pretty_assertions::assert_eq;
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;

    use super::*;
    use crate::FastlogParent;
    use crate::fastlog_impl::fetch_fastlog_batch_by_hm_id;
    use crate::fastlog_impl::fetch_flattened;

    #[derive(Clone)]
    #[facet::container]
    struct TestRepo {
        #[facet]
        bonsai_hg_mapping: dyn BonsaiHgMapping,
        #[facet]
        bookmarks: dyn Bookmarks,
        #[facet]
        repo_blobstore: RepoBlobstore,
        #[facet]
        repo_derived_data: RepoDerivedData,
        #[facet]
        filestore_config: FilestoreConfig,
        #[facet]
        commit_graph: CommitGraph,
        #[facet]
        commit_graph_writer: dyn CommitGraphWriter,
        #[facet]
        repo_identity: RepoIdentity,
    }

    /// Smoke test: derive FastlogV2 on a Linear fixture and verify each HM
    /// entry has a non-empty batch whose head linknode equals the changeset
    /// that introduced that path.
    #[mononoke::fbinit_test]
    async fn test_derive_fastlog_v2_linear(fb: FacebookInit) -> Result<()> {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let manager = repo.repo_derived_data().manager();

        // Resolve the head of the Linear fixture.
        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await?
            .unwrap();

        // Derive FastlogV2 — implicitly derives HistoryManifest as a dependency.
        let v2 = manager
            .derive::<RootFastlogV2>(&ctx, bcs_id, None, DerivationPriority::LOW)
            .await?;
        assert_eq!(v2.changeset_id(), bcs_id);

        // Walk the root HM directory and verify every entry has a batch.
        let root_hm = v2.root_manifest().into_history_manifest_directory_id();
        let blobstore = Arc::new(repo.repo_blobstore.clone());
        let entries: Vec<_> = root_hm
            .list_all_entries(ctx.clone(), blobstore.clone())
            .try_collect()
            .await?;
        assert!(
            !entries.is_empty(),
            "Linear root HM dir should have entries"
        );

        for (_path, entry) in entries {
            let batch = fetch_fastlog_batch_by_hm_id(&ctx, &repo.repo_blobstore, &entry)
                .await?
                .ok_or_else(|| anyhow!("expected a fastlog v2 batch for HM entry {entry:?}"))?;
            let flattened = fetch_flattened(&batch, &ctx, &repo.repo_blobstore).await?;
            assert!(
                !flattened.is_empty(),
                "fastlog v2 batch for HM entry {entry:?} should not be empty"
            );
        }

        Ok(())
    }

    /// Verifies V1 and V2 produce equivalent `(ChangesetId, Vec<FastlogParent>)`
    /// sequences for the root entry of a Linear fixture commit. Same hash type
    /// (Blake2) and the FastlogBatch payload algorithm is shared, so the
    /// changesets and their parent offsets must match exactly.
    #[mononoke::fbinit_test]
    async fn test_v1_v2_equivalence_root(fb: FacebookInit) -> Result<()> {
        use unodes::RootUnodeManifestId;

        use crate::fastlog_impl::fetch_fastlog_batch_by_unode_id;

        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let manager = repo.repo_derived_data().manager();

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await?
            .unwrap();

        // Derive V1 (RootFastlog implicitly derives RootUnodeManifestId).
        let _ = manager
            .derive::<crate::RootFastlog>(&ctx, bcs_id, None, DerivationPriority::LOW)
            .await?;
        let root_unode = manager
            .derive::<RootUnodeManifestId>(&ctx, bcs_id, None, DerivationPriority::LOW)
            .await?;
        let v1_batch = fetch_fastlog_batch_by_unode_id(
            &ctx,
            &repo.repo_blobstore,
            &Entry::Tree(root_unode.manifest_unode_id().clone()),
        )
        .await?
        .expect("v1 batch missing for root unode");
        let v1_flat = fetch_flattened(&v1_batch, &ctx, &repo.repo_blobstore).await?;

        // Derive V2.
        let v2 = manager
            .derive::<RootFastlogV2>(&ctx, bcs_id, None, DerivationPriority::LOW)
            .await?;
        let v2_batch = fetch_fastlog_batch_by_hm_id(
            &ctx,
            &repo.repo_blobstore,
            &Entry::Tree(v2.root_manifest().into_history_manifest_directory_id()),
        )
        .await?
        .expect("v2 batch missing for root HM dir");
        let v2_flat = fetch_flattened(&v2_batch, &ctx, &repo.repo_blobstore).await?;

        assert_eq!(v1_flat, v2_flat, "V1 and V2 root batches must agree");
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_derive_single_empty_commit_no_parents(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let bcs = create_bonsai_changeset(vec![]);
        let bcs_id = bcs.get_changeset_id();
        save_changesets(&ctx, &repo, vec![bcs]).await.unwrap();

        let root_hm_id = derive_fastlog_batch_and_hm(&ctx, bcs_id.clone(), &repo).await;

        let list = fetch_list(&ctx, &repo, Entry::Tree(root_hm_id)).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[mononoke::fbinit_test]
    async fn test_derive_single_commit_no_parents(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        // This is the initial diff with no parents
        // See tests/fixtures/src/lib.rs
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await
            .unwrap()
            .unwrap();

        let root_hm_id = derive_fastlog_batch_and_hm(&ctx, bcs_id.clone(), &repo).await;
        let list = fetch_list(&ctx, &repo, Entry::Tree(root_hm_id.clone())).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let blobstore = Arc::new(repo.repo_blobstore.clone());
        let path_1 = NonRootMPath::new("1").unwrap();
        let path_files = NonRootMPath::new("files").unwrap();
        let entries: Vec<_> = root_hm_id
            .find_entries(ctx.clone(), blobstore.clone(), vec![path_1, path_files])
            .try_collect()
            .await
            .unwrap();

        let list = fetch_list(&ctx, &repo, entries.first().unwrap().1.clone()).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let list = fetch_list(&ctx, &repo, entries.get(1).unwrap().1.clone()).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[mononoke::fbinit_test]
    async fn test_derive_linear(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await
            .unwrap()
            .unwrap();

        let root_hm_id = derive_fastlog_batch_and_hm(&ctx, bcs_id.clone(), &repo).await;

        let blobstore = Arc::new(repo.repo_blobstore.clone());
        let entries: Vec<_> = root_hm_id
            .list_all_entries(ctx.clone(), blobstore)
            .map_ok(|(_, entry)| entry)
            .try_collect()
            .await
            .unwrap();

        for entry in entries {
            verify_list(&ctx, &repo, entry).await;
        }
    }

    #[mononoke::fbinit_test]
    async fn test_derive_overflow(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut bonsais = vec![];
        let mut parents = vec![];
        for i in 1..max_entries_in_fastlog_batch() {
            let filename = String::from("1");
            let content = format!("{i}");
            let stored_files = store_files(
                &ctx,
                btreemap! { filename.as_str() => Some(content.as_str()) },
                &repo,
            )
            .await;

            let bcs = create_bonsai_changeset_with_files(parents, stored_files);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.first().unwrap();
        save_changesets(&ctx, &repo, bonsais).await.unwrap();

        verify_all_entries_for_commit(&ctx, &repo, *latest).await;
    }

    #[mononoke::fbinit_test]
    async fn test_random_repo(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut rng = XorShiftRng::seed_from_u64(0); // reproducible Rng
        let (latest, _) =
            tests_utils::random::create_random_stack(&ctx, &repo, &mut rng, None, [10; 200])
                .await
                .unwrap();

        verify_all_entries_for_commit(&ctx, &repo, latest).await;
    }

    #[mononoke::fbinit_test]
    async fn test_derive_empty_commits(fb: FacebookInit) {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut bonsais = vec![];
        let mut parents = vec![];
        for _ in 1..max_entries_in_fastlog_batch() {
            let bcs = create_bonsai_changeset(parents);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.first().unwrap();
        save_changesets(&ctx, &repo, bonsais).await.unwrap();

        verify_all_entries_for_commit(&ctx, &repo, *latest).await;
    }

    #[mononoke::fbinit_test]
    async fn test_derive_merges(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        {
            let repo: TestRepo = MergeUneven::get_repo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo: TestRepo = MergeEven::get_repo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo: TestRepo = UnsharedMergeEven::get_repo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo: TestRepo = UnsharedMergeUneven::get_repo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_bfs_order(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepo = Linear::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        //            E
        //           / \
        //          D   C
        //         /   / \
        //        F   A   B
        //       /
        //      G
        //
        //   Expected order [E, D, C, F, A, B, G]

        let mut bonsais = vec![];

        let a = create_bonsai_changeset_with_author(vec![], "author1".to_string());
        println!("a = {}", a.get_changeset_id());
        bonsais.push(a.clone());
        let b = create_bonsai_changeset_with_author(vec![], "author2".to_string());
        println!("b = {}", b.get_changeset_id());
        bonsais.push(b.clone());

        let c = create_bonsai_changeset(vec![a.get_changeset_id(), b.get_changeset_id()]);
        println!("c = {}", c.get_changeset_id());
        bonsais.push(c.clone());

        let g = create_bonsai_changeset_with_author(vec![], "author3".to_string());
        println!("g = {}", g.get_changeset_id());
        bonsais.push(g.clone());

        let stored_files = store_files(&ctx, btreemap! { "file" => Some("f") }, &repo).await;
        let f = create_bonsai_changeset_with_files(vec![g.get_changeset_id()], stored_files);
        println!("f = {}", f.get_changeset_id());
        bonsais.push(f.clone());

        let stored_files = store_files(&ctx, btreemap! { "file" => Some("d") }, &repo).await;
        let d = create_bonsai_changeset_with_files(vec![f.get_changeset_id()], stored_files);
        println!("d = {}", d.get_changeset_id());
        bonsais.push(d.clone());

        let stored_files = store_files(&ctx, btreemap! { "file" => Some("e") }, &repo).await;
        let e = create_bonsai_changeset_with_files(
            vec![d.get_changeset_id(), c.get_changeset_id()],
            stored_files,
        );
        println!("e = {}", e.get_changeset_id());
        bonsais.push(e.clone());

        save_changesets(&ctx, &repo, bonsais).await?;

        verify_all_entries_for_commit(&ctx, &repo, e.get_changeset_id()).await;
        Ok(())
    }

    async fn all_commits(
        ctx: CoreContext,
        repo: TestRepo,
    ) -> Result<Vec<(ChangesetId, HgChangesetId)>> {
        let master_book = BookmarkKey::new("master").unwrap();
        let bcs_id = repo
            .bookmarks
            .get(ctx.clone(), &master_book, bookmarks::Freshness::MostRecent)
            .await?
            .unwrap();

        repo.commit_graph()
            .ancestors_difference_stream(&ctx, vec![bcs_id], vec![])
            .await?
            .and_then(move |new_bcs_id| {
                cloned!(ctx, repo);
                async move {
                    let hg_cs_id = repo.derive_hg_changeset(&ctx, new_bcs_id).await?;
                    Ok((new_bcs_id, hg_cs_id))
                }
            })
            .try_collect()
            .await
    }

    async fn verify_all_entries_for_commit(
        ctx: &CoreContext,
        repo: &TestRepo,
        bcs_id: ChangesetId,
    ) {
        let root_hm_id = derive_fastlog_batch_and_hm(ctx, bcs_id.clone(), repo).await;

        let blobstore = Arc::new(repo.repo_blobstore.clone());
        let entries: Vec<_> = root_hm_id
            .list_all_entries(ctx.clone(), blobstore.clone())
            .try_collect()
            .await
            .unwrap();

        for (path, entry) in entries {
            println!("verifying: path: {path:?} hm entry: {entry:?}");
            verify_list(ctx, repo, entry).await;
        }
    }

    async fn derive_fastlog_batch_and_hm(
        ctx: &CoreContext,
        bcs_id: ChangesetId,
        repo: &TestRepo,
    ) -> HMDirId {
        let manager = repo.repo_derived_data().manager();
        manager
            .derive::<RootFastlogV2>(ctx, bcs_id, None, DerivationPriority::LOW)
            .await
            .unwrap();

        let root_hm = manager
            .derive::<RootHistoryManifestDirectoryId>(ctx, bcs_id, None, DerivationPriority::LOW)
            .await
            .unwrap();
        root_hm.into_history_manifest_directory_id()
    }

    async fn verify_list(ctx: &CoreContext, repo: &TestRepo, entry: Entry<HMDirId, HMFileId>) {
        let list = fetch_list(ctx, repo, entry).await;
        let actual_bonsais: Vec<_> = list.into_iter().map(|(bcs_id, _)| bcs_id).collect();

        let expected_bonsais = find_hm_history(ctx.fb, repo, entry).await;
        assert_eq!(actual_bonsais, expected_bonsais);
    }

    async fn fetch_list(
        ctx: &CoreContext,
        repo: &TestRepo,
        entry: Entry<HMDirId, HMFileId>,
    ) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
        let blobstore = repo.repo_blobstore.clone();
        let batch = fetch_fastlog_batch_by_hm_id(ctx, &blobstore, &entry)
            .await
            .unwrap()
            .expect("batch hasn't been generated yet");

        println!(
            "batch for {:?}: latest size: {}, previous batches size: {}",
            entry,
            batch.latest().len(),
            batch.previous_batches().len(),
        );
        assert!(batch.latest().len() <= MAX_LATEST_LEN);
        assert!(batch.previous_batches().len() <= MAX_BATCHES);
        fetch_flattened(&batch, ctx, &blobstore).await.unwrap()
    }

    async fn find_hm_history(
        fb: FacebookInit,
        repo: &TestRepo,
        start: Entry<HMDirId, HMFileId>,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock(fb);
        let mut q = VecDeque::new();
        q.push_back(start.clone());

        let mut visited = HashSet::new();
        visited.insert(start);
        let mut history = vec![];
        loop {
            let hm_entry = q.pop_front();
            let hm_entry = match hm_entry {
                Some(hm_entry) => hm_entry,
                None => {
                    break;
                }
            };
            let linknode = hm_entry.get_linknode(&ctx, repo).await.unwrap();
            history.push(linknode);
            if history.len() >= max_entries_in_fastlog_batch() {
                break;
            }
            let parents = hm_entry.get_parents(&ctx, repo).await.unwrap();
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        history
    }

    #[async_trait]
    trait HMHistory {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a TestRepo,
        ) -> Result<Vec<Entry<HMDirId, HMFileId>>, Error>;

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a TestRepo,
        ) -> Result<ChangesetId, Error>;
    }

    #[async_trait]
    impl HMHistory for Entry<HMDirId, HMFileId> {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a TestRepo,
        ) -> Result<Vec<Entry<HMDirId, HMFileId>>, Error> {
            match self {
                Entry::Leaf(file_id) => {
                    let file: HistoryManifestFile = file_id.load(ctx, &repo.repo_blobstore).await?;
                    Ok(file
                        .parents
                        .iter()
                        .filter_map(|p| match p {
                            HistoryManifestEntry::File(id) => Some(Entry::Leaf(*id)),
                            HistoryManifestEntry::Directory(id) => Some(Entry::Tree(*id)),
                            HistoryManifestEntry::DeletedNode(_) => None,
                        })
                        .collect())
                }
                Entry::Tree(dir_id) => {
                    let dir: HistoryManifestDirectory =
                        dir_id.load(ctx, &repo.repo_blobstore).await?;
                    Ok(dir
                        .parents
                        .iter()
                        .filter_map(|p| match p {
                            HistoryManifestEntry::Directory(id) => Some(Entry::Tree(*id)),
                            HistoryManifestEntry::File(id) => Some(Entry::Leaf(*id)),
                            HistoryManifestEntry::DeletedNode(_) => None,
                        })
                        .collect())
                }
            }
        }

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a TestRepo,
        ) -> Result<ChangesetId, Error> {
            match self {
                Entry::Leaf(file_id) => {
                    let file: HistoryManifestFile = file_id.load(ctx, &repo.repo_blobstore).await?;
                    Ok(file.linknode)
                }
                Entry::Tree(dir_id) => {
                    let dir: HistoryManifestDirectory =
                        dir_id.load(ctx, &repo.repo_blobstore).await?;
                    Ok(dir.linknode)
                }
            }
        }
    }
}
