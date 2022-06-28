/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use filenodes::PreparedFilenode;
use mercurial_types_mocks::nodehash::ONES_CSID;
use mercurial_types_mocks::nodehash::ONES_FNID;
use mercurial_types_mocks::nodehash::TWOS_CSID;
use mercurial_types_mocks::nodehash::TWOS_FNID;
use mononoke_types::RepoPath;
use mononoke_types_mocks::repo::REPO_ZERO;
use path_hash::PathWithHash;
use std::sync::Arc;

use super::util::build_reader_writer;
use super::util::build_shard;
use crate::local_cache::test::HashMapCache;
use crate::local_cache::LocalCache;
use crate::reader::filenode_cache_key;
use crate::reader::history_cache_key;
use crate::remote_cache::test::make_test_cache;
use crate::remote_cache::test::wait_for_filenode;
use crate::remote_cache::test::wait_for_history;

fn filenode() -> FilenodeInfo {
    FilenodeInfo {
        filenode: ONES_FNID,
        p1: None,
        p2: None,
        copyfrom: None,
        linknode: ONES_CSID,
    }
}

fn second_filenode() -> FilenodeInfo {
    FilenodeInfo {
        filenode: TWOS_FNID,
        p1: None,
        p2: None,
        copyfrom: None,
        linknode: TWOS_CSID,
    }
}

#[fbinit::test]
async fn test_filenode_fill(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mut reader, writer) = build_reader_writer(vec![build_shard()?]);

    reader.local_cache = LocalCache::Test(HashMapCache::new());
    reader.remote_cache = make_test_cache();
    let mut reader = Arc::new(reader);

    let path = RepoPath::file("file")?;
    let info = filenode();

    writer
        .insert_filenodes(
            &ctx,
            REPO_ZERO,
            vec![PreparedFilenode {
                path: path.clone(),
                info: info.clone(),
            }],
            false,
        )
        .await?
        .do_not_handle_disabled_filenodes()?;

    let key = filenode_cache_key(
        REPO_ZERO,
        &PathWithHash::from_repo_path(&path),
        &info.filenode,
    );

    // A local miss should fill the remote cache:
    reader
        .clone()
        .get_filenode(&ctx, REPO_ZERO, &path, info.filenode)
        .await?
        .do_not_handle_disabled_filenodes()?;
    wait_for_filenode(&reader.remote_cache, &key).await?;

    // A local hit should not fill the remote cache:
    Arc::get_mut(&mut reader).unwrap().remote_cache = make_test_cache();
    reader
        .clone()
        .get_filenode(&ctx, REPO_ZERO, &path, info.filenode)
        .await?
        .do_not_handle_disabled_filenodes()?;
    let r = wait_for_filenode(&reader.remote_cache, &key).await;
    assert!(r.is_err());

    Ok(())
}

#[fbinit::test]
async fn test_history_fill(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mut reader, writer) = build_reader_writer(vec![build_shard()?]);

    reader.local_cache = LocalCache::Test(HashMapCache::new());
    reader.remote_cache = make_test_cache();
    let mut reader = Arc::new(reader);

    let path = RepoPath::file("file")?;
    let info = filenode();

    writer
        .insert_filenodes(
            &ctx,
            REPO_ZERO,
            vec![PreparedFilenode {
                path: path.clone(),
                info: info.clone(),
            }],
            false,
        )
        .await?
        .do_not_handle_disabled_filenodes()?;

    let limit = None;
    // A local miss should fill the remote cache:
    reader
        .clone()
        .get_all_filenodes_for_path(&ctx, REPO_ZERO, &path, limit)
        .await?
        .do_not_handle_disabled_filenodes()?;

    let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), limit);
    wait_for_history(&reader.remote_cache, &key).await?;

    // A local hit should not fill the remote cache:
    Arc::get_mut(&mut reader).unwrap().remote_cache = make_test_cache();
    reader
        .clone()
        .get_all_filenodes_for_path(&ctx, REPO_ZERO, &path, limit)
        .await?
        .do_not_handle_disabled_filenodes()?;
    let r = wait_for_history(&reader.remote_cache, &key).await;
    assert!(r.is_err());

    Ok(())
}

#[fbinit::test]
async fn test_too_big_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (mut reader, writer) = build_reader_writer(vec![build_shard()?]);

    reader.local_cache = LocalCache::Test(HashMapCache::new());
    reader.remote_cache = make_test_cache();
    let reader = Arc::new(reader);

    let path = RepoPath::file("file")?;
    let info = filenode();
    let second_info = second_filenode();

    writer
        .insert_filenodes(
            &ctx,
            REPO_ZERO,
            vec![
                PreparedFilenode {
                    path: path.clone(),
                    info: info.clone(),
                },
                PreparedFilenode {
                    path: path.clone(),
                    info: second_info.clone(),
                },
            ],
            false,
        )
        .await?
        .do_not_handle_disabled_filenodes()?;

    let limit = Some(1);
    reader
        .clone()
        .get_all_filenodes_for_path(&ctx, REPO_ZERO, &path, limit)
        .await?
        .do_not_handle_disabled_filenodes()?;

    let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), limit);
    let res = reader
        .local_cache
        .get(&key)
        .ok_or(anyhow!("key not found"))?;

    assert!(res.is_none());

    // Make sure we get a cache miss if another limit parameter is used
    let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), None);
    assert!(reader.local_cache.get(&key).is_none());

    let key = history_cache_key(REPO_ZERO, &PathWithHash::from_repo_path(&path), Some(2));
    assert!(reader.local_cache.get(&key).is_none());

    Ok(())
}
