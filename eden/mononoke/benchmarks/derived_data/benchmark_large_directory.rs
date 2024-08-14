/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This benchmark generates a single initial commit that adds 100k files to
//! a single large directory, and then 10 more commits that add, modify, and
//! remove some of those files at random.
//!
//! It then benchmarks deriving one of the derived data types (fsnodes,
//! unodes, skeleton manifest or deleted manifests) for those commits.

use std::collections::BTreeSet;

use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use deleted_manifest::DeletedManifestOps;
use deleted_manifest::RootDeletedManifestIdCommon;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use manifest::ManifestOps;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_derivation::RootHgAugmentedManifestId;
use mononoke_types::ChangesetId;
use rand::distributions::Alphanumeric;
use rand::distributions::Uniform;
use rand::thread_rng;
use rand::Rng;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use skeleton_manifest::RootSkeletonManifestId;
use tests_utils::CreateCommitContext;
use unodes::RootUnodeManifestId;

#[facet::container]
#[derive(Clone)]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    filestore_config: FilestoreConfig,
}

fn gen_filename(rng: &mut impl Rng, len: usize) -> String {
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .take(len)
        .map(char::from)
        .collect()
}

async fn make_initial_large_directory(
    ctx: &CoreContext,
    repo: &Repo,
    count: usize,
) -> Result<(ChangesetId, BTreeSet<String>)> {
    let mut filenames = BTreeSet::new();
    let mut rng = thread_rng();
    let len_distr = Uniform::new(5, 50);
    while filenames.len() < count {
        let len = rng.sample(len_distr);
        let filename = gen_filename(&mut rng, len);
        filenames.insert(filename);
    }

    let mut create = CreateCommitContext::new_root(ctx, repo);
    for filename in filenames.iter() {
        create = create.add_file(
            format!("large_directory/{}", filename).as_str(),
            format!("content of {}", filename),
        );
    }
    let csid = create.commit().await?;

    Ok((csid, filenames))
}

async fn modify_large_directory(
    ctx: &CoreContext,
    repo: &Repo,
    filenames: &mut BTreeSet<String>,
    csid: ChangesetId,
    index: usize,
    add_count: usize,
    modify_count: usize,
    delete_count: usize,
) -> Result<ChangesetId> {
    let mut create = CreateCommitContext::new(ctx, repo, vec![csid]);
    let mut rng = thread_rng();
    let len_distr = Uniform::new(5, 50);

    let mut add_filenames = BTreeSet::new();
    while add_filenames.len() < add_count {
        let len = rng.sample(len_distr);
        let filename = gen_filename(&mut rng, len);
        if !filenames.contains(&filename) {
            add_filenames.insert(filename);
        }
    }

    let delete_count = delete_count.min(filenames.len());
    let modify_count = modify_count.min(filenames.len() - delete_count);
    let mut modify_filename_indexes = BTreeSet::new();
    let index_distr = Uniform::new(0, filenames.len());
    while modify_filename_indexes.len() < modify_count {
        let index = rng.sample(index_distr);
        modify_filename_indexes.insert(index);
    }
    let mut delete_filename_indexes = BTreeSet::new();
    while delete_filename_indexes.len() < delete_count {
        let index = rng.sample(index_distr);
        if !modify_filename_indexes.contains(&index) {
            delete_filename_indexes.insert(index);
        }
    }
    let mut modify_filenames = BTreeSet::new();
    let mut delete_filenames = BTreeSet::new();
    for (index, filename) in filenames.iter().enumerate() {
        if modify_filename_indexes.contains(&index) {
            modify_filenames.insert(filename);
        } else if delete_filename_indexes.contains(&index) {
            delete_filenames.insert(filename);
        }
    }

    for filename in add_filenames.iter().chain(modify_filenames) {
        create = create.add_file(
            format!("large_directory/{}", filename).as_str(),
            format!("content {} of {}", index, filename),
        );
    }
    for filename in delete_filenames.iter() {
        create = create.delete_file(format!("large_directory/{}", filename).as_str());
    }

    let csid = create.commit().await?;
    Ok(csid)
}

async fn derive(ctx: &CoreContext, repo: &Repo, data: &str, csid: ChangesetId) -> String {
    match data {
        MappedHgChangesetId::NAME => repo
            .repo_derived_data()
            .derive::<MappedHgChangesetId>(ctx, csid)
            .await
            .unwrap()
            .hg_changeset_id()
            .to_string(),
        RootSkeletonManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootSkeletonManifestId>(ctx, csid)
            .await
            .unwrap()
            .skeleton_manifest_id()
            .to_string(),
        RootUnodeManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootUnodeManifestId>(ctx, csid)
            .await
            .unwrap()
            .manifest_unode_id()
            .to_string(),
        RootDeletedManifestV2Id::NAME => repo
            .repo_derived_data()
            .derive::<RootDeletedManifestV2Id>(ctx, csid)
            .await
            .unwrap()
            .id()
            .to_string(),
        RootFsnodeId::NAME => repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(ctx, csid)
            .await
            .unwrap()
            .fsnode_id()
            .to_string(),
        RootHgAugmentedManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootHgAugmentedManifestId>(ctx, csid)
            .await
            .unwrap()
            .hg_augmented_manifest_id()
            .to_string(),
        _ => panic!("invalid derived data type: {}", data),
    }
}

async fn iterate(ctx: &CoreContext, repo: &Repo, data: &str, csid: ChangesetId) -> u64 {
    match data {
        MappedHgChangesetId::NAME => repo
            .repo_derived_data()
            .derive::<MappedHgChangesetId>(ctx, csid)
            .await
            .unwrap()
            .hg_changeset_id()
            .load(ctx, repo.repo_blobstore())
            .await
            .unwrap()
            .manifestid()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        RootSkeletonManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootSkeletonManifestId>(ctx, csid)
            .await
            .unwrap()
            .skeleton_manifest_id()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        RootUnodeManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootUnodeManifestId>(ctx, csid)
            .await
            .unwrap()
            .manifest_unode_id()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        RootDeletedManifestV2Id::NAME => repo
            .repo_derived_data()
            .derive::<RootDeletedManifestV2Id>(ctx, csid)
            .await
            .unwrap()
            .list_all_entries(ctx, repo.repo_blobstore())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        RootFsnodeId::NAME => repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(ctx, csid)
            .await
            .unwrap()
            .fsnode_id()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        RootHgAugmentedManifestId::NAME => repo
            .repo_derived_data()
            .derive::<RootHgAugmentedManifestId>(ctx, csid)
            .await
            .unwrap()
            .hg_augmented_manifest_id()
            .list_all_entries(ctx.clone(), repo.repo_blobstore().clone())
            .try_fold(0u64, |acc, _| future::ok(acc + 1))
            .await
            .unwrap(),
        _ => panic!("invalid derived data type: {}", data),
    }
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let mut args = std::env::args();
    let _ = args.next();
    let data = args.next().unwrap_or_else(|| String::from("fsnodes"));
    println!("Deriving: {}", data);

    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;

    let (mut csid, mut filenames) = make_initial_large_directory(&ctx, &repo, 100_000).await?;

    println!("First commit: {}", csid);
    let (stats, derived_id) = derive(&ctx, &repo, &data, csid).timed().await;
    println!("Derived id: {}  stats: {:?}", derived_id, stats);

    let commit_count = 10;

    for commit in 0..commit_count {
        csid =
            modify_large_directory(&ctx, &repo, &mut filenames, csid, commit, 25, 100, 25).await?;
    }

    println!("Last commit: {}", csid);
    let (stats, derived_id) = derive(&ctx, &repo, &data, csid).timed().await;
    println!("Derived id: {}  stats: {:?}", derived_id, stats);

    let (stats, count) = iterate(&ctx, &repo, &data, csid).timed().await;
    println!("Iterated count: {}  stats: {:?}", count, stats);

    Ok(())
}
