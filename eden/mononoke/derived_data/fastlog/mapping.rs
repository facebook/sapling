/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationContext;
use futures::stream::TryStreamExt;
use manifest::find_intersection_of_diffs;
use manifest::Entry;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use thiserror::Error;
use unodes::RootUnodeManifestId;

use crate::fastlog_impl::create_new_batch;
use crate::fastlog_impl::save_fastlog_batch_by_unode_id;

use derived_data_service_if::types as thrift;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum FastlogParent {
    /// Parent exists and it's stored in the batch
    Known(ChangesetId),
    /// Parent exists, but it's not stored in the batch (including previous_batches).
    /// It needs to be fetched separately
    Unknown,
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("invalid Thrift structure '{0}': {1}")]
    InvalidThrift(String, String),
    #[error("Fastlog batch for {0:?} unode not found")]
    NotFound(Entry<ManifestUnodeId, FileUnodeId>),
    #[error("Failed to deserialize FastlogBatch for {0}: {1}")]
    DeserializationError(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootFastlog(ChangesetId);

impl RootFastlog {
    pub fn changeset_id(&self) -> &ChangesetId {
        &self.0
    }
}

impl From<ChangesetId> for RootFastlog {
    fn from(csid: ChangesetId) -> RootFastlog {
        RootFastlog(csid)
    }
}

fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_rootfastlog.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootFastlog>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootFastlog {
    const NAME: &'static str = "fastlog";

    type Dependencies = dependencies![RootUnodeManifestId];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let bcs_id = bonsai.get_changeset_id();
        let unode_mf_id = derivation_ctx
            .derive_dependency::<RootUnodeManifestId>(ctx, bonsai.get_changeset_id())
            .await?
            .manifest_unode_id()
            .clone();
        let parents = derivation_ctx
            .fetch_parents::<RootUnodeManifestId>(ctx, &bonsai)
            .await?
            .into_iter()
            .map(|id| id.manifest_unode_id().clone())
            .collect::<Vec<_>>();

        let blobstore = derivation_ctx.blobstore();

        find_intersection_of_diffs(ctx.clone(), blobstore.clone(), unode_mf_id, parents)
            .map_ok(move |(_, entry)| {
                cloned!(blobstore, ctx);
                async move {
                    tokio::spawn(async move {
                        let parents = fetch_unode_parents(&ctx, &blobstore, entry).await?;

                        let fastlog_batch =
                            create_new_batch(&ctx, &blobstore, parents, bcs_id).await?;

                        save_fastlog_batch_by_unode_id(&ctx, &blobstore, entry, fastlog_batch).await
                    })
                    .await?
                }
            })
            .try_buffer_unordered(100)
            .try_for_each(|_| async { Ok(()) })
            .await?;

        Ok(RootFastlog(bcs_id))
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx
            .blobstore()
            .put(ctx, key, BlobstoreBytes::empty())
            .await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        match derivation_ctx.blobstore().get(ctx, &key).await? {
            Some(_) => Ok(Some(RootFastlog(changeset_id))),
            None => Ok(None),
        }
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::fastlog(thrift::DerivedDataFastlog::root_fastlog_id(id)) = data
        {
            ChangesetId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::fastlog(
            thrift::DerivedDataFastlog::root_fastlog_id(data.changeset_id().into_thrift()),
        ))
    }
}

async fn fetch_unode_parents<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    unode_entry_id: Entry<ManifestUnodeId, FileUnodeId>,
) -> Result<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error> {
    let unode_entry = unode_entry_id.load(ctx, blobstore).await?;

    let res = match unode_entry {
        Entry::Tree(tree) => tree
            .parents()
            .clone()
            .into_iter()
            .map(Entry::Tree)
            .collect(),
        Entry::Leaf(leaf) => leaf
            .parents()
            .clone()
            .into_iter()
            .map(Entry::Leaf)
            .collect(),
    };
    Ok(res)
}

impl_bonsai_derived_via_manager!(RootFastlog);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fastlog_impl::fetch_fastlog_batch_by_unode_id;
    use crate::fastlog_impl::fetch_flattened;
    use blobrepo::save_bonsai_changesets;
    use blobrepo::BlobRepo;
    use bookmarks::BookmarkName;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::create_bonsai_changeset;
    use fixtures::create_bonsai_changeset_with_author;
    use fixtures::create_bonsai_changeset_with_files;
    use fixtures::store_files;
    use fixtures::Linear;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use fixtures::UnsharedMergeUneven;
    use futures::compat::Stream01CompatExt;
    use futures::Stream;
    use futures::TryFutureExt;
    use manifest::ManifestOps;
    use maplit::btreemap;
    use mercurial_derived_data::DeriveHgChangeset;
    use mercurial_types::HgChangesetId;
    use mononoke_types::fastlog_batch::max_entries_in_fastlog_batch;
    use mononoke_types::fastlog_batch::MAX_BATCHES;
    use mononoke_types::fastlog_batch::MAX_LATEST_LEN;
    use mononoke_types::MPath;
    use mononoke_types::ManifestUnodeId;
    use pretty_assertions::assert_eq;
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use simulated_repo::GenManifest;
    use simulated_repo::GenSettings;
    use std::collections::BTreeMap;
    use std::collections::HashSet;
    use std::collections::VecDeque;
    use std::str::FromStr;
    use std::sync::Arc;

    #[fbinit::test]
    async fn test_derive_single_empty_commit_no_parents(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);
        let bcs = create_bonsai_changeset(vec![]);
        let bcs_id = bcs.get_changeset_id();
        save_bonsai_changesets(vec![bcs], ctx.clone(), &repo)
            .await
            .unwrap();

        let root_unode_mf_id = derive_fastlog_batch_and_unode(&ctx, bcs_id.clone(), &repo).await;

        let list = fetch_list(&ctx, &repo, Entry::Tree(root_unode_mf_id)).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[fbinit::test]
    async fn test_derive_single_commit_no_parents(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        // This is the initial diff with no parents
        // See tests/fixtures/src/lib.rs
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await
            .unwrap()
            .unwrap();

        let root_unode_mf_id = derive_fastlog_batch_and_unode(&ctx, bcs_id.clone(), &repo).await;
        let list = fetch_list(&ctx, &repo, Entry::Tree(root_unode_mf_id.clone())).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let blobstore = Arc::new(repo.get_blobstore());
        let path_1 = MPath::new(&"1").unwrap();
        let path_files = MPath::new(&"files").unwrap();
        let entries: Vec<_> = root_unode_mf_id
            .find_entries(ctx.clone(), blobstore.clone(), vec![path_1, path_files])
            .try_collect()
            .await
            .unwrap();

        let list = fetch_list(&ctx, &repo, entries.get(0).unwrap().1.clone()).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let list = fetch_list(&ctx, &repo, entries.get(1).unwrap().1.clone()).await;
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[fbinit::test]
    async fn test_derive_linear(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await
            .unwrap()
            .unwrap();

        let root_unode_mf_id = derive_fastlog_batch_and_unode(&ctx, bcs_id.clone(), &repo).await;

        let blobstore = Arc::new(repo.get_blobstore());
        let entries: Vec<_> = root_unode_mf_id
            .list_all_entries(ctx.clone(), blobstore)
            .map_ok(|(_, entry)| entry)
            .try_collect()
            .await
            .unwrap();

        for entry in entries {
            verify_list(&ctx, &repo, entry).await;
        }
    }

    #[fbinit::test]
    async fn test_derive_overflow(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut bonsais = vec![];
        let mut parents = vec![];
        for i in 1..max_entries_in_fastlog_batch() {
            let filename = String::from("1");
            let content = format!("{}", i);
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

        let latest = parents.get(0).unwrap();
        save_bonsai_changesets(bonsais, ctx.clone(), &repo)
            .await
            .unwrap();

        verify_all_entries_for_commit(&ctx, &repo, *latest).await;
    }

    #[fbinit::test]
    async fn test_random_repo(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng
        let gen_settings = GenSettings::default();
        let mut changes_count = vec![];
        changes_count.resize(200, 10);
        let latest = GenManifest::new()
            .gen_stack(
                ctx.clone(),
                repo.clone(),
                &mut rng,
                &gen_settings,
                None,
                changes_count,
            )
            .await
            .unwrap();

        verify_all_entries_for_commit(&ctx, &repo, latest).await;
    }

    #[fbinit::test]
    async fn test_derive_empty_commits(fb: FacebookInit) {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let mut bonsais = vec![];
        let mut parents = vec![];
        for _ in 1..max_entries_in_fastlog_batch() {
            let bcs = create_bonsai_changeset(parents);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.get(0).unwrap();
        save_bonsai_changesets(bonsais, ctx.clone(), &repo)
            .await
            .unwrap();

        verify_all_entries_for_commit(&ctx, &repo, *latest).await;
    }

    #[fbinit::test]
    async fn test_find_intersection_of_diffs_unodes_linear(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        // This commit creates file "1" and "files"
        // See eden/mononoke/tests/fixtures
        let parent_root_unode =
            derive_unode(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await?;

        // This commit creates file "2" and modifies "files"
        // See eden/mononoke/tests/fixtures
        let child_root_unode =
            derive_unode(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;

        let mut entries: Vec<_> = find_intersection_of_diffs(
            ctx,
            Arc::new(repo.get_blobstore()),
            child_root_unode,
            vec![parent_root_unode],
        )
        .map_ok(|(path, _)| match path {
            Some(path) => String::from_utf8(path.to_vec()).unwrap(),
            None => String::new(),
        })
        .try_collect()
        .await?;
        entries.sort();

        assert_eq!(
            entries,
            vec![String::new(), String::from("2"), String::from("files")]
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_find_intersection_of_diffs_merge(fb: FacebookInit) -> Result<(), Error> {
        async fn test_single_find_unodes_merge(
            fb: FacebookInit,
            parent_files: Vec<BTreeMap<&str, Option<&str>>>,
            merge_files: BTreeMap<&str, Option<&str>>,
            expected: Vec<String>,
        ) -> Result<(), Error> {
            let repo = Linear::getrepo(fb).await;
            let ctx = CoreContext::test_mock(fb);
            let manager = repo.repo_derived_data().manager();

            let mut bonsais = vec![];
            let mut parents = vec![];

            for (i, p) in parent_files.into_iter().enumerate() {
                println!("parent {}, {:?} ", i, p);
                let stored_files = store_files(&ctx, p, &repo).await;
                let bcs = create_bonsai_changeset_with_files(vec![], stored_files);
                parents.push(bcs.get_changeset_id());
                bonsais.push(bcs);
            }

            println!("merge {:?} ", merge_files);
            let merge_stored_files = store_files(&ctx, merge_files, &repo).await;
            let bcs = create_bonsai_changeset_with_files(parents.clone(), merge_stored_files);
            let merge_bcs_id = bcs.get_changeset_id();

            bonsais.push(bcs);
            save_bonsai_changesets(bonsais, ctx.clone(), &repo)
                .await
                .unwrap();

            let mut parent_unodes = vec![];

            for p in parents {
                let parent_unode = manager.derive::<RootUnodeManifestId>(&ctx, p, None).await?;
                let parent_unode = parent_unode.manifest_unode_id().clone();
                parent_unodes.push(parent_unode);
            }

            let merge_unode = manager
                .derive::<RootUnodeManifestId>(&ctx, merge_bcs_id, None)
                .await?;
            let merge_unode = merge_unode.manifest_unode_id().clone();

            let mut entries: Vec<_> = find_intersection_of_diffs(
                ctx,
                Arc::new(repo.get_blobstore()),
                merge_unode,
                parent_unodes,
            )
            .map_ok(|(path, _)| match path {
                Some(path) => String::from_utf8(path.to_vec()).unwrap(),
                None => String::new(),
            })
            .try_collect()
            .await?;
            entries.sort();

            assert_eq!(entries, expected);
            Ok(())
        }

        test_single_find_unodes_merge(
            fb,
            vec![
                btreemap! {
                    "1" => Some("1"),
                },
                btreemap! {
                    "2" => Some("2"),
                },
            ],
            btreemap! {},
            vec![String::new()],
        )
        .await?;

        test_single_find_unodes_merge(
            fb,
            vec![
                btreemap! {
                    "1" => Some("1"),
                },
                btreemap! {
                    "2" => Some("2"),
                },
                btreemap! {
                    "3" => Some("3"),
                },
            ],
            btreemap! {},
            vec![String::new()],
        )
        .await?;

        test_single_find_unodes_merge(
            fb,
            vec![
                btreemap! {
                    "1" => Some("1"),
                },
                btreemap! {
                    "2" => Some("2"),
                },
            ],
            btreemap! {
                "inmerge" => Some("1"),
            },
            vec![String::new(), String::from("inmerge")],
        )
        .await?;

        test_single_find_unodes_merge(
            fb,
            vec![
                btreemap! {
                    "file" => Some("contenta"),
                },
                btreemap! {
                    "file" => Some("contentb"),
                },
            ],
            btreemap! {
                "file" => Some("mergecontent"),
            },
            vec![String::new(), String::from("file")],
        )
        .await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_derive_merges(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

        {
            let repo = MergeUneven::getrepo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).try_collect().await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo = MergeEven::getrepo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).try_collect().await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo = UnsharedMergeEven::getrepo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).try_collect().await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        {
            let repo = UnsharedMergeUneven::getrepo(fb).await;
            let all_commits: Vec<_> = all_commits(ctx.clone(), repo.clone()).try_collect().await?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&ctx, &repo, bcs_id).await;
            }
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_bfs_order(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
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

        save_bonsai_changesets(bonsais, ctx.clone(), &repo).await?;

        verify_all_entries_for_commit(&ctx, &repo, e.get_changeset_id()).await;
        Ok(())
    }

    fn all_commits(
        ctx: CoreContext,
        repo: BlobRepo,
    ) -> impl Stream<Item = Result<(ChangesetId, HgChangesetId), Error>> {
        let master_book = BookmarkName::new("master").unwrap();
        repo.get_bonsai_bookmark(ctx.clone(), &master_book)
            .map_ok(move |maybe_bcs_id| {
                let bcs_id = maybe_bcs_id.unwrap();
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                    .compat()
                    .and_then(move |new_bcs_id| {
                        cloned!(ctx, repo);
                        async move {
                            let hg_cs_id = repo.derive_hg_changeset(&ctx, new_bcs_id).await?;
                            Ok((new_bcs_id, hg_cs_id))
                        }
                    })
            })
            .try_flatten_stream()
    }

    async fn verify_all_entries_for_commit(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bcs_id: ChangesetId,
    ) {
        let root_unode_mf_id = derive_fastlog_batch_and_unode(ctx, bcs_id.clone(), repo).await;

        let blobstore = Arc::new(repo.get_blobstore());
        let entries: Vec<_> = root_unode_mf_id
            .list_all_entries(ctx.clone(), blobstore.clone())
            .try_collect()
            .await
            .unwrap();

        for (path, entry) in entries {
            println!("verifying: path: {:?} unode: {:?}", path, entry);
            verify_list(ctx, repo, entry).await;
        }
    }

    async fn derive_unode(
        ctx: &CoreContext,
        repo: &BlobRepo,
        hg_cs: &str,
    ) -> Result<ManifestUnodeId, Error> {
        let manager = repo.repo_derived_data().manager();
        let hg_cs_id = HgChangesetId::from_str(hg_cs)?;
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, hg_cs_id)
            .await?
            .unwrap();
        let root_unode = manager
            .derive::<RootUnodeManifestId>(ctx, bcs_id, None)
            .await?;
        Ok(root_unode.manifest_unode_id().clone())
    }

    async fn derive_fastlog_batch_and_unode(
        ctx: &CoreContext,
        bcs_id: ChangesetId,
        repo: &BlobRepo,
    ) -> ManifestUnodeId {
        let manager = repo.repo_derived_data().manager();
        manager
            .derive::<RootFastlog>(ctx, bcs_id, None)
            .await
            .unwrap();

        let root_unode = manager
            .derive::<RootUnodeManifestId>(ctx, bcs_id, None)
            .await
            .unwrap();
        root_unode.manifest_unode_id().clone()
    }

    async fn verify_list(
        ctx: &CoreContext,
        repo: &BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) {
        let list = fetch_list(ctx, repo, entry).await;
        let actual_bonsais: Vec<_> = list.into_iter().map(|(bcs_id, _)| bcs_id).collect();

        let expected_bonsais = find_unode_history(ctx.fb, repo, entry).await;
        assert_eq!(actual_bonsais, expected_bonsais);
    }

    async fn fetch_list(
        ctx: &CoreContext,
        repo: &BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
        let blobstore = repo.blobstore();
        let batch = fetch_fastlog_batch_by_unode_id(ctx, blobstore, &entry)
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
        fetch_flattened(&batch, ctx, blobstore).await.unwrap()
    }

    async fn find_unode_history(
        fb: FacebookInit,
        repo: &BlobRepo,
        start: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Vec<ChangesetId> {
        let ctx = CoreContext::test_mock(fb);
        let mut q = VecDeque::new();
        q.push_back(start.clone());

        let mut visited = HashSet::new();
        visited.insert(start);
        let mut history = vec![];
        loop {
            let unode_entry = q.pop_front();
            let unode_entry = match unode_entry {
                Some(unode_entry) => unode_entry,
                None => {
                    break;
                }
            };
            let linknode = unode_entry.get_linknode(&ctx, repo).await.unwrap();
            history.push(linknode);
            if history.len() >= max_entries_in_fastlog_batch() {
                break;
            }
            let parents = unode_entry.get_parents(&ctx, repo).await.unwrap();
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        history
    }

    #[async_trait]
    trait UnodeHistory {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error>;

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<ChangesetId, Error>;
    }

    #[async_trait]
    impl UnodeHistory for Entry<ManifestUnodeId, FileUnodeId> {
        async fn get_parents<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error> {
            match self {
                Entry::Leaf(file_unode_id) => {
                    let unode_mf = file_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf
                        .parents()
                        .iter()
                        .cloned()
                        .map(Entry::Leaf)
                        .collect())
                }
                Entry::Tree(mf_unode_id) => {
                    let unode_mf = mf_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf
                        .parents()
                        .iter()
                        .cloned()
                        .map(Entry::Tree)
                        .collect())
                }
            }
        }

        async fn get_linknode<'a>(
            &'a self,
            ctx: &'a CoreContext,
            repo: &'a BlobRepo,
        ) -> Result<ChangesetId, Error> {
            match self {
                Entry::Leaf(file_unode_id) => {
                    let unode_file = file_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_file.linknode().clone())
                }
                Entry::Tree(mf_unode_id) => {
                    let unode_mf = mf_unode_id.load(ctx, repo.blobstore()).await?;
                    Ok(unode_mf.linknode().clone())
                }
            }
        }
    }
}
