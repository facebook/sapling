/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use anyhow::Error;
use fbinit::FacebookInit;
use mononoke_types_mocks::{
    changesetid::{ONES_CSID, TWOS_CSID},
    hash::{ONES, TWOS},
};

#[fbinit::test]
async fn test_simple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);

    let dst_path = Some(MPath::new("dstpath")?);
    let src_path = Some(MPath::new("srcpath")?);
    let entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        src_path,
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(&ctx, vec![entry.clone()])
        .await?;
    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, dst_path)
        .await?;

    assert_eq!(Some(entry), res);
    Ok(())
}

#[fbinit::test]
async fn test_insert_multiple(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);

    let first_dst_path = Some(MPath::new("first_dstpath")?);
    let first_src_path = Some(MPath::new("second_srcpath")?);
    let first_entry = MutableRenameEntry::new(
        TWOS_CSID,
        first_dst_path.clone(),
        ONES_CSID,
        first_src_path,
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    let second_dst_path = Some(MPath::new("second_dstpath")?);
    let second_src_path = Some(MPath::new("second_srcpath")?);
    let second_entry = MutableRenameEntry::new(
        TWOS_CSID,
        second_dst_path.clone(),
        ONES_CSID,
        second_src_path,
        Entry::Leaf(FileUnodeId::new(TWOS)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(&ctx, vec![first_entry.clone(), second_entry.clone()])
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

#[fbinit::test]
async fn test_overwrite(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let store = SqlMutableRenamesStore::with_sqlite_in_memory()?;
    let mutable_renames = MutableRenames::new_test(RepositoryId::new(0), store);

    let dst_path = Some(MPath::new("first_dstpath")?);
    let first_src_path = Some(MPath::new("first_srcpath")?);
    let first_entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        first_src_path.clone(),
        Entry::Leaf(FileUnodeId::new(ONES)),
    )?;

    mutable_renames
        .add_or_overwrite_renames(&ctx, vec![first_entry.clone()])
        .await?;

    let second_src_path = Some(MPath::new("second_srcpath")?);
    let second_entry = MutableRenameEntry::new(
        TWOS_CSID,
        dst_path.clone(),
        ONES_CSID,
        second_src_path,
        Entry::Leaf(FileUnodeId::new(TWOS)),
    )?;

    assert_ne!(first_entry, second_entry);
    mutable_renames
        .add_or_overwrite_renames(&ctx, vec![second_entry.clone()])
        .await?;

    let res = mutable_renames
        .get_rename(&ctx, TWOS_CSID, dst_path)
        .await?;

    assert_eq!(Some(second_entry), res);
    Ok(())
}
