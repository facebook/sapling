/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use commit_graph::BaseCommitGraphWriter;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use fbinit::FacebookInit;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use mononoke_macros::mononoke;
use mononoke_types_mocks::changesetid::ONES_CSID;
use mononoke_types_mocks::changesetid::TWOS_CSID;
use mononoke_types_mocks::hash::ONES;
use mononoke_types_mocks::hash::TWOS;
use mononoke_types_mocks::repo::REPO_ZERO;

use super::*;

async fn setup_commit_graph(ctx: &CoreContext) -> Result<CommitGraph, Error> {
    let storage = InMemoryCommitGraphStorage::new(REPO_ZERO);
    let commit_graph = CommitGraph::new(Arc::new(storage));
    let commit_graph_writer = BaseCommitGraphWriter::new(commit_graph.clone());

    commit_graph_writer
        .add(ctx, ONES_CSID, vec![].into())
        .await?;
    commit_graph_writer
        .add(ctx, TWOS_CSID, vec![ONES_CSID].into())
        .await?;

    Ok(commit_graph)
}

#[mononoke::fbinit_test]
async fn test_simple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);
    let commit_graph = setup_commit_graph(&ctx).await?;

    let dst_path = MPath::new("dstpath")?;
    let src_path = MPath::new("srcpath")?;
    let entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        src_path,
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(&ctx, &commit_graph, vec![entry.clone()])
        .await?;
    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, dst_path)
        .await?;

    assert_eq!(Some(entry), res);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_insert_multiple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);
    let commit_graph = setup_commit_graph(&ctx).await?;

    let first_dst_path = MPath::new("first_dstpath")?;
    let first_src_path = MPath::new("second_srcpath")?;
    let first_entry = MutableRenameEntry::new(
        TWOS_CSID,
        first_dst_path.clone(),
        ONES_CSID,
        first_src_path,
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    let second_dst_path = MPath::new("second_dstpath")?;
    let second_src_path = MPath::new("second_srcpath")?;
    let second_entry = MutableRenameEntry::new(
        TWOS_CSID,
        second_dst_path.clone(),
        ONES_CSID,
        second_src_path,
        Entry::Leaf(FileUnodeId::new(TWOS)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(
            &ctx,
            &commit_graph,
            vec![first_entry.clone(), second_entry.clone()],
        )
        .await?;
    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, second_dst_path)
        .await?;

    assert_eq!(Some(second_entry), res);

    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, first_dst_path)
        .await?;

    assert_eq!(Some(first_entry), res);
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_overwrite(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);
    let commit_graph = setup_commit_graph(&ctx).await?;

    let dst_path = MPath::new("first_dstpath")?;
    let first_src_path = MPath::new("first_srcpath")?;
    let first_entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        first_src_path.clone(),
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(&ctx, &commit_graph, vec![first_entry.clone()])
        .await?;

    let second_src_path = MPath::new("second_srcpath")?;
    let second_entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        second_src_path,
        Entry::Leaf(FileUnodeId::new(TWOS)),
    )?;

    assert_ne!(first_entry, second_entry);
    mutable_renames
        .add_or_overwrite_renames(&ctx, &commit_graph, vec![second_entry.clone()])
        .await?;

    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, dst_path)
        .await?;

    assert_eq!(Some(second_entry), res);
    Ok(())
}
