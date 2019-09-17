// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

/// This library is used to efficiently store file and directory history.
/// For each unode we store a FastlogBatch - thrift structure that stores latest commits and their
/// parents that modified this file or directory. Commits are stored in BFS order.
/// All FastlogBatches are stored in blobstore.
///
/// Commits also store pointers to their parents, however they are stored as an offset to the
/// commit hash in batch. I.e. if we have two commits A and B and A is an ancestor of B, then
/// batch will look like:
/// B, vec![ParentOffset(1)]
/// A, vec![]
///
/// Note that commits where a file was deleted are not stored in FastlogBatch. It also doesn't
/// store a history across deletions i.e. if a file was added, then deleted then added again in
/// commit A, FastlogBatch in commit A will contain only one entry.
///
/// RootFastlog is a derived data which derives FastlogBatch for each unode
/// that was created or modified in this commit.
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreBytes, Loadable};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use failure_ext::{Error, Fail};
use futures::{
    future,
    stream::{self, FuturesUnordered},
    Future, Stream,
};
use futures_ext::{spawn_future, BoxFuture, FutureExt, StreamExt};
use manifest::{Diff, Entry, ManifestOps};
use mononoke_types::{BonsaiChangeset, ChangesetId, FileUnodeId, MPath, ManifestUnodeId};
use std::collections::HashMap;
use std::iter::FromIterator;
use std::sync::Arc;
use tracing::{trace_args, EventId, Traced};
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

mod fastlog_impl;
mod thrift {
    pub use mononoke_types_thrift::*;
}

use fastlog_impl::{
    create_new_batch, fetch_fastlog_batch_by_unode_id, fetch_flattened,
    save_fastlog_batch_by_unode_id,
};

/// Returns history for a given unode if it exists.
/// This is the public API of this crate i.e. what clients should use if they want to
/// fetch the history
pub fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error = Error> {
    let blobstore = Arc::new(repo.get_blobstore());
    fetch_fastlog_batch_by_unode_id(ctx.clone(), blobstore.clone(), unode_entry).and_then(
        move |maybe_fastlog_batch| {
            maybe_fastlog_batch.map(|fastlog_batch| fetch_flattened(&fastlog_batch, ctx, blobstore))
        },
    )
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum FastlogParent {
    /// Parent exists and it's stored in the batch
    Known(ChangesetId),
    /// Parent exists, but it's not stored in the batch (including previous_batches).
    /// It needs to be fetched separately
    Unknown,
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid Thrift structure '{}': {}", _0, _1)]
    InvalidThrift(String, String),
    #[fail(display = "Fastlog batch for {:?} unode not found", _0)]
    NotFound(Entry<ManifestUnodeId, FileUnodeId>),
    #[fail(display = "Failed to deserialize FastlogBatch for {}: {}", _0, _1)]
    DeserializationError(String, String),
}

#[derive(Clone, Debug)]
pub struct RootFastlog(ChangesetId);

impl BonsaiDerived for RootFastlog {
    const NAME: &'static str = "fastlog";

    fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        _parents: Vec<Self>,
    ) -> BoxFuture<Self, Error> {
        // TODO(stash): we shouldn't create a RootUnodeManifestMapping mapping here -
        // ideally we should create it once when Mononoke is initialized.
        // But for now this is a limitation of derived data trait - it requires explicit
        // passing of RootUnodeManifestMapping
        let unode_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
        let bcs_id = bonsai.get_changeset_id();
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), bcs_id)
            .join(fetch_parent_root_unodes(
                ctx.clone(),
                repo.clone(),
                bonsai,
                unode_mapping,
            ))
            .and_then(move |(root_unode_mf_id, parents)| {
                let blobstore = repo.get_blobstore().boxed();
                let unode_mf_id = root_unode_mf_id.manifest_unode_id().clone();

                let event_id = EventId::new();
                find_new_unodes(
                    ctx.clone(),
                    blobstore.clone(),
                    unode_mf_id,
                    parents,
                    Some(event_id),
                )
                .map(move |(_, entry)| {
                    let f = fetch_unode_parents(ctx.clone(), blobstore.clone(), entry).and_then({
                        cloned!(ctx, blobstore);
                        move |parents| {
                            create_new_batch(ctx.clone(), blobstore.clone(), parents, bcs_id)
                                .and_then({
                                    cloned!(ctx, blobstore);
                                    move |fastlog_batch| {
                                        save_fastlog_batch_by_unode_id(
                                            ctx,
                                            blobstore,
                                            entry,
                                            fastlog_batch,
                                        )
                                    }
                                })
                        }
                    });

                    spawn_future(f)
                })
                .buffered(100)
                .for_each(|_| Ok(()))
                .map(move |_| RootFastlog(bcs_id))
            })
            .boxify()
    }
}

fn find_new_unodes(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_mf_id: ManifestUnodeId,
    parent_unodes: Vec<ManifestUnodeId>,
    event_id: Option<EventId>,
) -> impl Stream<Item = (Option<MPath>, Entry<ManifestUnodeId, FileUnodeId>), Error = Error> {
    match parent_unodes.get(0) {
        Some(parent) => (*parent)
            .diff(ctx.clone(), blobstore.clone(), unode_mf_id)
            .filter_map(|diff_entry| match diff_entry {
                Diff::Added(path, entry) => Some((path, entry)),
                Diff::Removed(..) => None,
                Diff::Changed(path, _, entry) => Some((path, entry)),
            })
            .collect()
            .and_then({
                cloned!(ctx);
                move |new_unodes| {
                    let paths: Vec<_> = new_unodes
                        .clone()
                        .into_iter()
                        .map(|(path, _)| path)
                        .collect();

                    let futs: Vec<_> = parent_unodes
                        .into_iter()
                        .skip(1)
                        .map(|p| {
                            p.find_entries(ctx.clone(), blobstore.clone(), paths.clone())
                                .collect_to::<HashMap<_, _>>()
                        })
                        .collect();

                    future::join_all(futs).map(move |entries_in_parents| {
                        let mut res = vec![];

                        for (path, unode) in new_unodes {
                            let mut new_entry = true;
                            for p in &entries_in_parents {
                                if p.get(&path) == Some(&unode) {
                                    new_entry = false;
                                    break;
                                }
                            }

                            if new_entry {
                                res.push((path, unode));
                            }
                        }

                        res
                    })
                }
            })
            .traced_with_id(
                &ctx.trace(),
                "derive_fastlog::find_new_unodes",
                trace_args! {},
                event_id.unwrap_or_else(|| EventId::new()),
            )
            .map(|entries| stream::iter_ok(entries.into_iter()))
            .flatten_stream()
            .boxify(),
        None => unode_mf_id
            .list_all_entries(ctx.clone(), blobstore.clone())
            .boxify(),
    }
}

fn fetch_parent_root_unodes(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: BonsaiChangeset,
    unode_mapping: Arc<RootUnodeManifestMapping>,
) -> impl Future<Item = Vec<ManifestUnodeId>, Error = Error> {
    let parents: Vec<_> = bonsai.parents().collect();
    future::join_all(parents.into_iter().map(move |p| {
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), p)
            .map(|root_unode_mf_id| root_unode_mf_id.manifest_unode_id().clone())
    }))
}

fn fetch_unode_parents(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry_id: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error = Error> {
    unode_entry_id
        .load(ctx, &blobstore)
        .from_err()
        .map(|unode_entry| match unode_entry {
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
        })
}

#[derive(Clone)]
pub struct RootFastlogMapping {
    blobstore: Arc<dyn Blobstore>,
}

impl RootFastlogMapping {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: &ChangesetId) -> String {
        format!("derived_rootfastlog.{}", cs_id)
    }
}

impl BonsaiDerivedMapping for RootFastlogMapping {
    type Value = RootFastlog;

    fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let gets = csids.into_iter().map(|cs_id| {
            self.blobstore
                .get(ctx.clone(), self.format_key(&cs_id))
                .map(move |maybe_val| maybe_val.map(|_| (cs_id.clone(), RootFastlog(cs_id))))
        });
        FuturesUnordered::from_iter(gets)
            .filter_map(|x| x) // Remove None
            .collect_to()
            .boxify()
    }

    fn put(&self, ctx: CoreContext, csid: ChangesetId, _id: Self::Value) -> BoxFuture<(), Error> {
        self.blobstore.put(
            ctx,
            self.format_key(&csid),
            // Value doesn't matter here, so just put empty Value
            BlobstoreBytes::from_bytes(Bytes::new()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benchmark_lib::{GenManifest, GenSettings};
    use blobrepo::save_bonsai_changesets;
    use bookmarks::BookmarkName;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::{
        create_bonsai_changeset, create_bonsai_changeset_with_author,
        create_bonsai_changeset_with_files, linear, merge_even, merge_uneven, store_files,
        unshared_merge_even, unshared_merge_uneven,
    };
    use maplit::btreemap;
    use mercurial_types::HgChangesetId;
    use mononoke_types::fastlog_batch::{
        max_entries_in_fastlog_batch, MAX_BATCHES, MAX_LATEST_LEN,
    };
    use mononoke_types::{MPath, ManifestUnodeId};
    use pretty_assertions::assert_eq;
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use revset::AncestorsNodeStream;
    use std::collections::{BTreeMap, HashSet, VecDeque};
    use std::str::FromStr;
    use tokio::runtime::Runtime;

    #[fbinit::test]
    fn test_derive_single_empty_commit_no_parents(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);
        let bcs = create_bonsai_changeset(vec![]);
        let bcs_id = bcs.get_changeset_id();
        rt.block_on(save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone()))
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            Entry::Tree(root_unode_mf_id),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[fbinit::test]
    fn test_derive_single_commit_no_parents(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);

        // This is the initial diff with no parents
        // See tests/fixtures/src/lib.rs
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
        let bcs_id = rt
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());
        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            Entry::Tree(root_unode_mf_id.clone()),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let blobstore = Arc::new(repo.get_blobstore());
        let path_1 = MPath::new(&"1").unwrap();
        let path_files = MPath::new(&"files").unwrap();
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .find_entries(ctx.clone(), blobstore.clone(), vec![path_1, path_files])
                    .collect(),
            )
            .unwrap();

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            entries.get(0).unwrap().1.clone(),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);

        let list = fetch_list(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            entries.get(1).unwrap().1.clone(),
        );
        assert_eq!(list, vec![(bcs_id, vec![])]);
    }

    #[fbinit::test]
    fn test_derive_linear(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let bcs_id = rt
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))
            .unwrap()
            .unwrap();

        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(&mut rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let blobstore = Arc::new(repo.get_blobstore());
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .list_all_entries(ctx.clone(), blobstore)
                    .map(|(_, entry)| entry)
                    .collect(),
            )
            .unwrap();

        for entry in entries {
            verify_list(&mut rt, ctx.clone(), repo.clone(), entry);
        }
    }

    #[fbinit::test]
    fn test_derive_overflow(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);

        let mut bonsais = vec![];
        let mut parents = vec![];
        for i in 1..max_entries_in_fastlog_batch() {
            let filename = String::from("1");
            let content = format!("{}", i);
            let stored_files = store_files(
                ctx.clone(),
                btreemap! { filename.as_str() => Some(content.as_str()) },
                repo.clone(),
            );

            let bcs = create_bonsai_changeset_with_files(parents, stored_files);
            let bcs_id = bcs.get_changeset_id();
            bonsais.push(bcs);
            parents = vec![bcs_id];
        }

        let latest = parents.get(0).unwrap();
        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, *latest);
    }

    #[fbinit::test]
    fn test_random_repo(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);

        let mut rng = XorShiftRng::seed_from_u64(0); // reproducable Rng
        let gen_settings = GenSettings::default();
        let mut changes_count = vec![];
        changes_count.resize(200, 10);
        let latest = rt
            .block_on(GenManifest::new().gen_stack(
                ctx.clone(),
                repo.clone(),
                &mut rng,
                &gen_settings,
                None,
                changes_count,
            ))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, latest);
    }

    #[fbinit::test]
    fn test_derive_empty_commits(fb: FacebookInit) {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
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
        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
            .unwrap();

        verify_all_entries_for_commit(&mut rt, ctx, repo, *latest);
    }

    #[fbinit::test]
    fn test_find_new_unodes_linear(fb: FacebookInit) -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
        let ctx = CoreContext::test_mock(fb);

        // This commit creates file "1" and "files"
        // See scm/mononoke/tests/fixtures
        let parent_root_unode = derive_unode(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
        )?;

        // This commit creates file "2" and modifies "files"
        // See scm/mononoke/tests/fixtures
        let child_root_unode = derive_unode(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            "3e0e761030db6e479a7fb58b12881883f9f8c63f",
        )?;

        let mut entries = rt.block_on(
            find_new_unodes(
                ctx,
                Arc::new(repo.get_blobstore()),
                child_root_unode,
                vec![parent_root_unode],
                None,
            )
            .map(|(path, _)| match path {
                Some(path) => String::from_utf8(path.to_vec()).unwrap(),
                None => String::new(),
            })
            .collect(),
        )?;
        entries.sort();

        assert_eq!(
            entries,
            vec![String::new(), String::from("2"), String::from("files")]
        );
        Ok(())
    }

    #[fbinit::test]
    fn test_find_new_unodes_merge(fb: FacebookInit) -> Result<(), Error> {
        fn test_single_find_unodes_merge(
            fb: FacebookInit,
            parent_files: Vec<BTreeMap<&str, Option<&str>>>,
            merge_files: BTreeMap<&str, Option<&str>>,
            expected: Vec<String>,
        ) -> Result<(), Error> {
            let mut rt = Runtime::new().unwrap();
            let repo = linear::getrepo(fb);
            let ctx = CoreContext::test_mock(fb);

            let mut bonsais = vec![];
            let mut parents = vec![];

            for (i, p) in parent_files.into_iter().enumerate() {
                println!("parent {}, {:?} ", i, p);
                let stored_files = store_files(ctx.clone(), p, repo.clone());
                let bcs = create_bonsai_changeset_with_files(vec![], stored_files);
                parents.push(bcs.get_changeset_id());
                bonsais.push(bcs);
            }

            println!("merge {:?} ", merge_files);
            let merge_stored_files = store_files(ctx.clone(), merge_files, repo.clone());
            let bcs = create_bonsai_changeset_with_files(parents.clone(), merge_stored_files);
            let merge_bcs_id = bcs.get_changeset_id();

            bonsais.push(bcs);
            rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))
                .unwrap();

            let unode_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
            let mut parent_unodes = vec![];

            for p in parents {
                let parent_unode = RootUnodeManifestId::derive(
                    ctx.clone(),
                    repo.clone(),
                    unode_mapping.clone(),
                    p,
                );
                let parent_unode = rt.block_on(parent_unode)?;
                let parent_unode = parent_unode.manifest_unode_id().clone();
                parent_unodes.push(parent_unode);
            }

            let merge_unode = RootUnodeManifestId::derive(
                ctx.clone(),
                repo.clone(),
                unode_mapping.clone(),
                merge_bcs_id,
            );
            let merge_unode = rt.block_on(merge_unode)?;
            let merge_unode = merge_unode.manifest_unode_id().clone();

            let mut entries = rt.block_on(
                find_new_unodes(
                    ctx,
                    Arc::new(repo.get_blobstore()),
                    merge_unode,
                    parent_unodes,
                    None,
                )
                .map(|(path, _)| match path {
                    Some(path) => String::from_utf8(path.to_vec()).unwrap(),
                    None => String::new(),
                })
                .collect(),
            )?;
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
        )?;

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
        )?;

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
        )?;

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
        )?;
        Ok(())
    }

    #[fbinit::test]
    fn test_derive_merges(fb: FacebookInit) -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();
        let ctx = CoreContext::test_mock(fb);

        {
            let repo = merge_uneven::getrepo(fb);
            let all_commits = rt.block_on(all_commits(ctx.clone(), repo.clone()).collect())?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&mut rt, ctx.clone(), repo.clone(), bcs_id);
            }
        }

        {
            let repo = merge_even::getrepo(fb);
            let all_commits = rt.block_on(all_commits(ctx.clone(), repo.clone()).collect())?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&mut rt, ctx.clone(), repo.clone(), bcs_id);
            }
        }

        {
            let repo = unshared_merge_even::getrepo(fb);
            let all_commits = rt.block_on(all_commits(ctx.clone(), repo.clone()).collect())?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&mut rt, ctx.clone(), repo.clone(), bcs_id);
            }
        }

        {
            let repo = unshared_merge_uneven::getrepo(fb);
            let all_commits = rt.block_on(all_commits(ctx.clone(), repo.clone()).collect())?;

            for (bcs_id, _hg_cs_id) in all_commits {
                verify_all_entries_for_commit(&mut rt, ctx.clone(), repo.clone(), bcs_id);
            }
        }

        Ok(())
    }

    #[fbinit::test]
    fn test_bfs_order(fb: FacebookInit) -> Result<(), Error> {
        let mut rt = Runtime::new().unwrap();
        let repo = linear::getrepo(fb);
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

        let stored_files =
            store_files(ctx.clone(), btreemap! { "file" => Some("f") }, repo.clone());
        let f = create_bonsai_changeset_with_files(vec![g.get_changeset_id()], stored_files);
        println!("f = {}", f.get_changeset_id());
        bonsais.push(f.clone());

        let stored_files =
            store_files(ctx.clone(), btreemap! { "file" => Some("d") }, repo.clone());
        let d = create_bonsai_changeset_with_files(vec![f.get_changeset_id()], stored_files);
        println!("d = {}", d.get_changeset_id());
        bonsais.push(d.clone());

        let stored_files =
            store_files(ctx.clone(), btreemap! { "file" => Some("e") }, repo.clone());
        let e = create_bonsai_changeset_with_files(
            vec![d.get_changeset_id(), c.get_changeset_id()],
            stored_files,
        );
        println!("e = {}", e.get_changeset_id());
        bonsais.push(e.clone());

        rt.block_on(save_bonsai_changesets(bonsais, ctx.clone(), repo.clone()))?;

        verify_all_entries_for_commit(&mut rt, ctx, repo, e.get_changeset_id());
        Ok(())
    }

    fn all_commits(
        ctx: CoreContext,
        repo: BlobRepo,
    ) -> impl Stream<Item = (ChangesetId, HgChangesetId), Error = Error> {
        let master_book = BookmarkName::new("master").unwrap();
        repo.get_bonsai_bookmark(ctx.clone(), &master_book)
            .map(move |maybe_bcs_id| {
                let bcs_id = maybe_bcs_id.unwrap();
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                    .and_then(move |new_bcs_id| {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), new_bcs_id)
                            .map(move |hg_cs_id| (new_bcs_id, hg_cs_id))
                    })
            })
            .flatten_stream()
    }

    fn verify_all_entries_for_commit(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        bcs_id: ChangesetId,
    ) {
        let root_unode_mf_id =
            derive_fastlog_batch_and_unode(rt, ctx.clone(), bcs_id.clone(), repo.clone());

        let blobstore = Arc::new(repo.get_blobstore());
        let entries = rt
            .block_on(
                root_unode_mf_id
                    .list_all_entries(ctx.clone(), blobstore.clone())
                    .collect(),
            )
            .unwrap();

        for (path, entry) in entries {
            println!("verifying: path: {:?} unode: {:?}", path, entry);
            verify_list(rt, ctx.clone(), repo.clone(), entry);
        }
    }

    fn derive_unode(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        hg_cs: &str,
    ) -> Result<ManifestUnodeId, Error> {
        let hg_cs_id = HgChangesetId::from_str(hg_cs)?;
        let bcs_id = rt.block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))?;
        let bcs_id = bcs_id.unwrap();

        let unode_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));

        let root_unode =
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unode_mapping.clone(), bcs_id);
        let root_unode = rt.block_on(root_unode)?;
        Ok(root_unode.manifest_unode_id().clone())
    }

    fn derive_fastlog_batch_and_unode(
        rt: &mut Runtime,
        ctx: CoreContext,
        bcs_id: ChangesetId,
        repo: BlobRepo,
    ) -> ManifestUnodeId {
        let blobstore = Arc::new(repo.get_blobstore());
        let mapping = RootFastlogMapping::new(blobstore.clone());
        rt.block_on(RootFastlog::derive(
            ctx.clone(),
            repo.clone(),
            mapping,
            bcs_id,
        ))
        .unwrap();

        let unode_mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
        let root_unode =
            RootUnodeManifestId::derive(ctx.clone(), repo.clone(), Arc::new(unode_mapping), bcs_id);
        let root_unode = rt.block_on(root_unode).unwrap();
        root_unode.manifest_unode_id().clone()
    }

    fn verify_list(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) {
        let list = fetch_list(rt, ctx.clone(), repo.clone(), entry);
        let actual_bonsais: Vec<_> = list.into_iter().map(|(bcs_id, _)| bcs_id).collect();

        let expected_bonsais = find_unode_history(ctx.fb, rt, repo, entry);
        assert_eq!(actual_bonsais, expected_bonsais);
    }

    fn fetch_list(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        entry: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
        let blobstore = Arc::new(repo.get_blobstore());
        let batch = rt
            .block_on(fetch_fastlog_batch_by_unode_id(
                ctx.clone(),
                blobstore.clone(),
                entry,
            ))
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
        rt.block_on(fetch_flattened(&batch, ctx, blobstore))
            .unwrap()
    }

    fn find_unode_history(
        fb: FacebookInit,
        runtime: &mut Runtime,
        repo: BlobRepo,
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
            let linknode = runtime
                .block_on(unode_entry.get_linknode(ctx.clone(), repo.clone()))
                .unwrap();
            history.push(linknode);
            if history.len() >= max_entries_in_fastlog_batch() {
                break;
            }
            let parents = runtime
                .block_on(unode_entry.get_parents(ctx.clone(), repo.clone()))
                .unwrap();
            q.extend(parents.into_iter().filter(|x| visited.insert(x.clone())));
        }

        history
    }

    trait UnodeHistory {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error>;

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error>;
    }

    impl UnodeHistory for Entry<ManifestUnodeId, FileUnodeId> {
        fn get_parents(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
        ) -> BoxFuture<Vec<Entry<ManifestUnodeId, FileUnodeId>>, Error> {
            match self {
                Entry::Leaf(file_unode_id) => file_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(Entry::Leaf)
                            .collect()
                    })
                    .boxify(),
                Entry::Tree(mf_unode_id) => mf_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| {
                        unode_mf
                            .parents()
                            .into_iter()
                            .cloned()
                            .map(Entry::Tree)
                            .collect()
                    })
                    .boxify(),
            }
        }

        fn get_linknode(&self, ctx: CoreContext, repo: BlobRepo) -> BoxFuture<ChangesetId, Error> {
            match self {
                Entry::Leaf(file_unode_id) => file_unode_id
                    .clone()
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_file| unode_file.linknode().clone())
                    .boxify(),
                Entry::Tree(mf_unode_id) => mf_unode_id
                    .load(ctx, &repo.get_blobstore())
                    .from_err()
                    .map(|unode_mf| unode_mf.linknode().clone())
                    .boxify(),
            }
        }
    }
}
