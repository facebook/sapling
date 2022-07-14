/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use context::CoreContext;
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRangeResult;
use filenodes::FilenodeResult;
use filenodes::PreparedFilenode;
use maplit::hashmap;
use mercurial_types::HgFileNodeId;
use mercurial_types_mocks::nodehash::ONES_CSID;
use mercurial_types_mocks::nodehash::ONES_FNID;
use mercurial_types_mocks::nodehash::THREES_CSID;
use mercurial_types_mocks::nodehash::THREES_FNID;
use mercurial_types_mocks::nodehash::TWOS_CSID;
use mercurial_types_mocks::nodehash::TWOS_FNID;
use mononoke_types::MPath;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use mononoke_types_mocks::repo::REPO_ONE;
use mononoke_types_mocks::repo::REPO_ZERO;
use sql::queries;
use sql::Connection;
use std::sync::Arc;
use tunables::with_tunables;
use tunables::MononokeTunables;

use crate::builder::SQLITE_INSERT_CHUNK_SIZE;
use crate::local_cache::test::HashMapCache;
use crate::local_cache::LocalCache;
use crate::reader::FilenodesReader;
use crate::writer::FilenodesWriter;

use super::util::build_reader_writer;
use super::util::build_shard;

async fn check_roundtrip(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    reader: Arc<FilenodesReader>,
    writer: &FilenodesWriter,
    payload: PreparedFilenode,
) -> Result<(), Error> {
    assert_eq!(
        async {
            let res = reader
                .clone()
                .get_filenode(ctx, repo_id, &payload.path, payload.info.filenode)
                .await?;
            res.do_not_handle_disabled_filenodes()
        }
        .await?,
        None
    );

    writer
        .insert_filenodes(ctx, repo_id, vec![payload.clone()], false)
        .await?
        .do_not_handle_disabled_filenodes()?;

    assert_eq!(
        async {
            let res = reader
                .get_filenode(ctx, repo_id, &payload.path, payload.info.filenode)
                .await?;
            res.do_not_handle_disabled_filenodes()
        }
        .await?,
        Some(payload.info),
    );

    Ok(())
}

#[fbinit::test]
async fn test_basic(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let shard = build_shard()?;
    let (reader, writer) = build_reader_writer(vec![shard]);
    let reader = Arc::new(reader);

    let payload = PreparedFilenode {
        path: RepoPath::FilePath(MPath::new(b"test")?),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: Some(TWOS_FNID),
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    };

    check_roundtrip(&ctx, REPO_ZERO, reader, &writer, payload).await?;

    Ok(())
}

#[fbinit::test]
async fn read_copy_info(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let shard = build_shard()?;
    let (reader, writer) = build_reader_writer(vec![shard]);
    let reader = Arc::new(reader);

    let from = PreparedFilenode {
        path: RepoPath::FilePath(MPath::new(b"from")?),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    };

    writer
        .insert_filenodes(&ctx, REPO_ZERO, vec![from.clone()], false)
        .await?
        .do_not_handle_disabled_filenodes()?;

    let payload = PreparedFilenode {
        path: RepoPath::FilePath(MPath::new(b"test")?),
        info: FilenodeInfo {
            filenode: TWOS_FNID,
            p1: None,
            p2: None,
            copyfrom: Some((from.path.clone(), from.info.filenode)),
            linknode: TWOS_CSID,
        },
    };

    check_roundtrip(&ctx, REPO_ZERO, reader, &writer, payload).await?;

    Ok(())
}

#[fbinit::test]
async fn test_repo_ids(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let shard = build_shard()?;
    let (reader, writer) = build_reader_writer(vec![shard]);
    let reader = Arc::new(reader);

    let payload = root_first_filenode();

    writer
        .insert_filenodes(&ctx, REPO_ZERO, vec![payload.clone()], false)
        .await?
        .do_not_handle_disabled_filenodes()?;

    assert_filenode(
        &ctx,
        reader.clone(),
        &payload.path,
        payload.info.filenode,
        REPO_ZERO,
        payload.info.clone(),
    )
    .await?;

    assert_no_filenode(&ctx, reader, &payload.path, payload.info.filenode, REPO_ONE).await?;

    Ok(())
}

queries! {
    write DeleteCopyInfo() {
        none,
        "DELETE FROM fixedcopyinfo"
    }

    write DeletePaths() {
        none,
        "DELETE FROM paths"
    }
}

#[fbinit::test]
async fn test_fallback_on_missing_copy_info(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let master = build_shard()?;
    let replica = build_shard()?;

    // Populate both master and replica with the same filenodes (to simulate replication).
    FilenodesWriter::new(
        SQLITE_INSERT_CHUNK_SIZE,
        vec![master.clone()],
        vec![master.clone()],
    )
    .insert_filenodes(
        &ctx,
        REPO_ZERO,
        vec![copied_from_filenode(), copied_filenode()],
        false,
    )
    .await?
    .do_not_handle_disabled_filenodes()?;

    FilenodesWriter::new(
        SQLITE_INSERT_CHUNK_SIZE,
        vec![replica.clone()],
        vec![replica.clone()],
    )
    .insert_filenodes(
        &ctx,
        REPO_ZERO,
        vec![copied_from_filenode(), copied_filenode()],
        false,
    )
    .await?
    .do_not_handle_disabled_filenodes()?;

    // Now, delete the copy info from the replica.
    DeleteCopyInfo::query(&replica).await?;

    let reader = Arc::new(FilenodesReader::new(vec![replica], vec![master]));
    let prepared = copied_filenode();
    assert_filenode(
        &ctx,
        reader,
        &prepared.path,
        prepared.info.filenode,
        REPO_ZERO,
        prepared.info.clone(),
    )
    .await?;

    Ok(())
}

#[fbinit::test]
async fn test_fallback_on_missing_paths(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let master = build_shard()?;
    let replica = build_shard()?;

    // Populate both master and replica with the same filenodes (to simulate replication).
    FilenodesWriter::new(
        SQLITE_INSERT_CHUNK_SIZE,
        vec![master.clone()],
        vec![master.clone()],
    )
    .insert_filenodes(
        &ctx,
        REPO_ZERO,
        vec![copied_from_filenode(), copied_filenode()],
        false,
    )
    .await?
    .do_not_handle_disabled_filenodes()?;

    FilenodesWriter::new(
        SQLITE_INSERT_CHUNK_SIZE,
        vec![replica.clone()],
        vec![replica.clone()],
    )
    .insert_filenodes(
        &ctx,
        REPO_ZERO,
        vec![copied_from_filenode(), copied_filenode()],
        false,
    )
    .await?
    .do_not_handle_disabled_filenodes()?;

    // Now, delete the copy info from the replica.
    DeletePaths::query(&replica).await?;

    let reader = Arc::new(FilenodesReader::new(vec![replica], vec![master]));
    let prepared = copied_filenode();
    assert_filenode(
        &ctx,
        reader,
        &prepared.path,
        prepared.info.filenode,
        REPO_ZERO,
        prepared.info.clone(),
    )
    .await?;

    Ok(())
}

fn root_first_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::root(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    }
}

fn root_second_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::root(),
        info: FilenodeInfo {
            filenode: TWOS_FNID,
            p1: Some(ONES_FNID),
            p2: None,
            copyfrom: None,
            linknode: TWOS_CSID,
        },
    }
}

fn root_merge_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::root(),
        info: FilenodeInfo {
            filenode: THREES_FNID,
            p1: Some(ONES_FNID),
            p2: Some(TWOS_FNID),
            copyfrom: None,
            linknode: THREES_CSID,
        },
    }
}

fn dir_a_first_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::dir("a").unwrap(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    }
}

fn file_a_first_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::file("a").unwrap(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    }
}

fn file_b_first_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::file("b").unwrap(),
        info: FilenodeInfo {
            filenode: TWOS_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: TWOS_CSID,
        },
    }
}

fn copied_from_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::file("copiedfrom").unwrap(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: TWOS_CSID,
        },
    }
}

fn copied_filenode() -> PreparedFilenode {
    PreparedFilenode {
        path: RepoPath::file("copiedto").unwrap(),
        info: FilenodeInfo {
            filenode: TWOS_FNID,
            p1: None,
            p2: None,
            copyfrom: Some((RepoPath::file("copiedfrom").unwrap(), ONES_FNID)),
            linknode: TWOS_CSID,
        },
    }
}

async fn do_add_filenodes(
    ctx: &CoreContext,
    writer: &FilenodesWriter,
    to_insert: Vec<PreparedFilenode>,
    repo_id: RepositoryId,
) -> Result<(), Error> {
    writer
        .insert_filenodes(ctx, repo_id, to_insert, false)
        .await?
        .do_not_handle_disabled_filenodes()?;
    Ok(())
}

async fn do_add_filenode(
    ctx: &CoreContext,
    writer: &FilenodesWriter,
    node: PreparedFilenode,
    repo_id: RepositoryId,
) -> Result<(), Error> {
    do_add_filenodes(ctx, writer, vec![node], repo_id).await?;
    Ok(())
}

async fn assert_no_filenode(
    ctx: &CoreContext,
    reader: Arc<FilenodesReader>,
    path: &RepoPath,
    hash: HgFileNodeId,
    repo_id: RepositoryId,
) -> Result<(), Error> {
    let res = reader.get_filenode(ctx, repo_id, path, hash).await?;
    let res = res.do_not_handle_disabled_filenodes()?;
    assert!(res.is_none());
    Ok(())
}

async fn assert_filenode(
    ctx: &CoreContext,
    reader: Arc<FilenodesReader>,
    path: &RepoPath,
    hash: HgFileNodeId,
    repo_id: RepositoryId,
    expected: FilenodeInfo,
) -> Result<(), Error> {
    let res = reader
        .get_filenode(ctx, repo_id, path, hash)
        .await?
        .do_not_handle_disabled_filenodes()?
        .ok_or(format_err!("not found: {}", hash))?;
    assert_eq!(res, expected);
    Ok(())
}

async fn assert_all_filenodes(
    ctx: &CoreContext,
    reader: Arc<FilenodesReader>,
    path: &RepoPath,
    repo_id: RepositoryId,
    expected: &Vec<FilenodeInfo>,
    limit: Option<u64>,
) -> Result<(), Error> {
    let res = reader
        .get_all_filenodes_for_path(ctx, repo_id, path, limit)
        .await?;
    let res = res.do_not_handle_disabled_filenodes()?;
    assert_eq!(res.as_ref(), Some(expected));
    Ok(())
}

macro_rules! filenodes_tests {
    ($test_suite_name:ident, $create_db:ident, $enable_caching:ident) => {
        mod $test_suite_name {
            use super::*;
            use fbinit::FacebookInit;

            #[fbinit::test]
            async fn test_simple_filenode_insert_and_get(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);

                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::root(),
                    ONES_FNID,
                    REPO_ZERO,
                    root_first_filenode().info,
                )
                .await?;

                assert_no_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::root(),
                    TWOS_FNID,
                    REPO_ZERO,
                )
                .await?;

                assert_no_filenode(&ctx, reader, &RepoPath::root(), ONES_FNID, REPO_ONE).await?;

                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_identical_in_batch(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (_reader, writer) = build_reader_writer($create_db()?);
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![root_first_filenode(), root_first_filenode()],
                    REPO_ZERO,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_filenode_insert_twice(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (_reader, writer) = build_reader_writer($create_db()?);
                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_filenode_with_parent(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, root_second_filenode(), REPO_ZERO).await?;
                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::root(),
                    ONES_FNID,
                    REPO_ZERO,
                    root_first_filenode().info,
                )
                .await?;
                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::root(),
                    TWOS_FNID,
                    REPO_ZERO,
                    root_second_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_root_filenode_with_two_parents(
                fb: FacebookInit,
            ) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, root_second_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, root_merge_filenode(), REPO_ZERO).await?;
                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::root(),
                    THREES_FNID,
                    REPO_ZERO,
                    root_merge_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_file_filenode(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenode(&ctx, &writer, file_a_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, file_b_first_filenode(), REPO_ZERO).await?;

                assert_no_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("non-existent").unwrap(),
                    ONES_FNID,
                    REPO_ZERO,
                )
                .await?;
                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("a").unwrap(),
                    ONES_FNID,
                    REPO_ZERO,
                    file_a_first_filenode().info,
                )
                .await?;
                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::file("b").unwrap(),
                    TWOS_FNID,
                    REPO_ZERO,
                    file_b_first_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_different_repo(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenode(&ctx, &writer, root_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, root_second_filenode(), REPO_ONE).await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::root(),
                    ONES_FNID,
                    REPO_ZERO,
                    root_first_filenode().info,
                )
                .await?;

                assert_no_filenode(&ctx, reader.clone(), &RepoPath::root(), ONES_FNID, REPO_ONE)
                    .await?;

                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::root(),
                    TWOS_FNID,
                    REPO_ONE,
                    root_second_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn test_insert_parent_and_child_in_same_batch(
                fb: FacebookInit,
            ) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);

                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![root_first_filenode(), root_second_filenode()],
                    REPO_ZERO,
                )
                .await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::root(),
                    ONES_FNID,
                    REPO_ZERO,
                    root_first_filenode().info,
                )
                .await?;

                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::root(),
                    TWOS_FNID,
                    REPO_ZERO,
                    root_second_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn insert_copied_file(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);

                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![copied_from_filenode(), copied_filenode()],
                    REPO_ZERO,
                )
                .await?;
                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::file("copiedto").unwrap(),
                    TWOS_FNID,
                    REPO_ZERO,
                    copied_filenode().info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn insert_same_copied_file(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (_reader, writer) = build_reader_writer($create_db()?);

                do_add_filenodes(&ctx, &writer, vec![copied_from_filenode()], REPO_ZERO).await?;
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![copied_filenode(), copied_filenode()],
                    REPO_ZERO,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn insert_copied_file_to_different_repo(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);

                let copied = PreparedFilenode {
                    path: RepoPath::file("copiedto").unwrap(),
                    info: FilenodeInfo {
                        filenode: TWOS_FNID,
                        p1: None,
                        p2: None,
                        copyfrom: Some((RepoPath::file("copiedfrom").unwrap(), ONES_FNID)),
                        linknode: TWOS_CSID,
                    },
                };

                let notcopied = PreparedFilenode {
                    path: RepoPath::file("copiedto").unwrap(),
                    info: FilenodeInfo {
                        filenode: TWOS_FNID,
                        p1: None,
                        p2: None,
                        copyfrom: None,
                        linknode: TWOS_CSID,
                    },
                };

                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![copied_from_filenode(), copied.clone()],
                    REPO_ZERO,
                )
                .await?;

                do_add_filenodes(&ctx, &writer, vec![notcopied.clone()], REPO_ONE).await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("copiedto").unwrap(),
                    TWOS_FNID,
                    REPO_ZERO,
                    copied.info,
                )
                .await?;

                assert_filenode(
                    &ctx,
                    reader,
                    &RepoPath::file("copiedto").unwrap(),
                    TWOS_FNID,
                    REPO_ONE,
                    notcopied.info,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn get_all_filenodes_maybe_stale(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![
                        root_first_filenode(),
                        root_second_filenode(),
                        root_merge_filenode(),
                    ],
                    REPO_ZERO,
                )
                .await?;
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![file_a_first_filenode(), file_b_first_filenode()],
                    REPO_ZERO,
                )
                .await?;

                let root_filenodes = vec![
                    root_first_filenode().info,
                    root_second_filenode().info,
                    root_merge_filenode().info,
                ];

                assert_all_filenodes(
                    &ctx,
                    reader.clone(),
                    &RepoPath::RootPath,
                    REPO_ZERO,
                    &root_filenodes,
                    None,
                )
                .await?;

                assert_all_filenodes(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("a").unwrap(),
                    REPO_ZERO,
                    &vec![file_a_first_filenode().info],
                    None,
                )
                .await?;

                assert_all_filenodes(
                    &ctx,
                    reader,
                    &RepoPath::file("b").unwrap(),
                    REPO_ZERO,
                    &vec![file_b_first_filenode().info],
                    None,
                )
                .await?;
                Ok(())
            }

            #[fbinit::test]
            async fn get_all_filenodes_maybe_stale_limited(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![
                        root_first_filenode(),
                        root_second_filenode(),
                        root_merge_filenode(),
                    ],
                    REPO_ZERO,
                )
                .await?;
                do_add_filenodes(
                    &ctx,
                    &writer,
                    vec![file_a_first_filenode(), file_b_first_filenode()],
                    REPO_ZERO,
                )
                .await?;

                let root_filenodes = vec![
                    root_first_filenode().info,
                    root_second_filenode().info,
                    root_merge_filenode().info,
                ];

                assert_all_filenodes(
                    &ctx,
                    reader.clone(),
                    &RepoPath::RootPath,
                    REPO_ZERO,
                    &root_filenodes,
                    Some(3),
                )
                .await?;

                let res = reader
                    .clone()
                    .get_all_filenodes_for_path(&ctx, REPO_ZERO, &RepoPath::RootPath, Some(1))
                    .await?;
                let res = res.do_not_handle_disabled_filenodes()?;
                assert_eq!(None, res);

                let res = reader
                    .get_all_filenodes_for_path(&ctx, REPO_ZERO, &RepoPath::RootPath, Some(2))
                    .await?;
                let res = res.do_not_handle_disabled_filenodes()?;
                assert_eq!(None, res);

                Ok(())
            }

            #[fbinit::test]
            async fn test_mixed_path_insert_and_get(fb: FacebookInit) -> Result<(), Error> {
                let ctx = CoreContext::test_mock(fb);
                let (mut reader, writer) = build_reader_writer($create_db()?);
                $enable_caching(&mut reader);
                let reader = Arc::new(reader);

                do_add_filenode(&ctx, &writer, file_a_first_filenode(), REPO_ZERO).await?;
                do_add_filenode(&ctx, &writer, dir_a_first_filenode(), REPO_ZERO).await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("a").unwrap(),
                    ONES_FNID,
                    REPO_ZERO,
                    file_a_first_filenode().info,
                )
                .await?;

                assert_filenode(
                    &ctx,
                    reader.clone(),
                    &RepoPath::dir("a").unwrap(),
                    ONES_FNID,
                    REPO_ZERO,
                    dir_a_first_filenode().info,
                )
                .await?;

                assert_all_filenodes(
                    &ctx,
                    reader.clone(),
                    &RepoPath::file("a").unwrap(),
                    REPO_ZERO,
                    &vec![file_a_first_filenode().info],
                    None,
                )
                .await?;

                assert_all_filenodes(
                    &ctx,
                    reader,
                    &RepoPath::dir("a").unwrap(),
                    REPO_ZERO,
                    &vec![dir_a_first_filenode().info],
                    None,
                )
                .await?;

                Ok(())
            }
        }
    };
}

fn create_unsharded() -> Result<Vec<Connection>, Error> {
    Ok(vec![build_shard()?])
}

fn create_sharded() -> Result<Vec<Connection>, Error> {
    (0..16).into_iter().map(|_| build_shard()).collect()
}

fn no_caching(_reader: &mut FilenodesReader) {}

fn with_caching(reader: &mut FilenodesReader) {
    reader.local_cache = LocalCache::Test(HashMapCache::new());
}

filenodes_tests!(uncached_unsharded_test, create_unsharded, no_caching);
filenodes_tests!(uncached_sharded_test, create_sharded, no_caching);

filenodes_tests!(cached_unsharded_test, create_unsharded, with_caching);
filenodes_tests!(cached_sharded_test, create_sharded, with_caching);

#[fbinit::test]
fn get_all_filenodes_maybe_stale_with_disabled(fb: FacebookInit) -> Result<(), Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let ctx = CoreContext::test_mock(fb);

    let (mut reader, writer) = build_reader_writer(create_sharded()?);
    with_caching(&mut reader);
    let reader = Arc::new(reader);

    runtime.block_on(do_add_filenodes(
        &ctx,
        &writer,
        vec![
            root_first_filenode(),
            root_second_filenode(),
            root_merge_filenode(),
        ],
        REPO_ZERO,
    ))?;

    runtime.block_on(do_add_filenodes(
        &ctx,
        &writer,
        vec![file_a_first_filenode(), file_b_first_filenode()],
        REPO_ZERO,
    ))?;

    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});
    let res = with_tunables(tunables, || {
        runtime.block_on(reader.clone().get_all_filenodes_for_path(
            &ctx,
            REPO_ZERO,
            &RepoPath::RootPath,
            None,
        ))
    })?;

    if let FilenodeRangeResult::Present(_) = res {
        panic!("expected FilenodeResult::Disabled");
    }

    let root_filenodes = vec![
        root_first_filenode().info,
        root_second_filenode().info,
        root_merge_filenode().info,
    ];

    runtime.block_on(assert_all_filenodes(
        &ctx,
        reader.clone(),
        &RepoPath::RootPath,
        REPO_ZERO,
        &root_filenodes,
        None,
    ))?;

    // All filenodes are cached now, even with filenodes_disabled = true
    // all filenodes should be returned
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});
    with_tunables(tunables, || {
        runtime.block_on(assert_all_filenodes(
            &ctx,
            reader,
            &RepoPath::RootPath,
            REPO_ZERO,
            &root_filenodes,
            None,
        ))
    })?;
    Ok(())
}

#[fbinit::test]
fn test_get_filenode_with_disabled(fb: FacebookInit) -> Result<(), Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    let ctx = CoreContext::test_mock(fb);

    let (mut reader, writer) = build_reader_writer(create_sharded()?);
    with_caching(&mut reader);
    let reader = Arc::new(reader);

    runtime.block_on(do_add_filenodes(
        &ctx,
        &writer,
        vec![root_first_filenode()],
        REPO_ZERO,
    ))?;

    let payload_info = root_first_filenode().info;

    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});
    let res = with_tunables(tunables, || {
        runtime.block_on(reader.clone().get_filenode(
            &ctx,
            REPO_ZERO,
            &RepoPath::RootPath,
            payload_info.filenode,
        ))
    })?;

    if let FilenodeResult::Present(_) = res {
        panic!("expected FilenodeResult::Disabled");
    }

    runtime.block_on(assert_filenode(
        &ctx,
        reader.clone(),
        &RepoPath::root(),
        ONES_FNID,
        REPO_ZERO,
        root_first_filenode().info,
    ))?;

    // The filenode are cached now, even with filenodes_disabled = true
    // all filenodes should be returned
    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"filenodes_disabled".to_string() => true});
    with_tunables(tunables, || {
        runtime.block_on(assert_filenode(
            &ctx,
            reader,
            &RepoPath::root(),
            ONES_FNID,
            REPO_ZERO,
            root_first_filenode().info,
        ))
    })?;
    Ok(())
}

#[fbinit::test]
async fn test_all_filenodes_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let (mut reader, writer) = build_reader_writer(create_unsharded()?);
    with_caching(&mut reader);
    let reader = Arc::new(reader);
    do_add_filenode(&ctx, &writer, file_a_first_filenode(), REPO_ZERO).await?;

    let dir_a_second_filenode = PreparedFilenode {
        path: RepoPath::dir("a").unwrap(),
        info: FilenodeInfo {
            filenode: TWOS_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: TWOS_CSID,
        },
    };
    do_add_filenode(&ctx, &writer, dir_a_second_filenode.clone(), REPO_ZERO).await?;

    assert_all_filenodes(
        &ctx,
        reader.clone(),
        &RepoPath::file("a")?,
        REPO_ZERO,
        &vec![file_a_first_filenode().info],
        None,
    )
    .await?;

    assert_all_filenodes(
        &ctx,
        reader,
        &RepoPath::dir("a")?,
        REPO_ZERO,
        &vec![dir_a_second_filenode.info],
        None,
    )
    .await?;

    Ok(())
}

#[fbinit::test]
async fn test_point_filenode_caching(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let (mut reader, writer) = build_reader_writer(create_unsharded()?);
    with_caching(&mut reader);
    let reader = Arc::new(reader);
    do_add_filenode(&ctx, &writer, file_a_first_filenode(), REPO_ZERO).await?;

    let dir_a_second_filenode = PreparedFilenode {
        path: RepoPath::dir("a").unwrap(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: TWOS_CSID,
        },
    };
    do_add_filenode(&ctx, &writer, dir_a_second_filenode.clone(), REPO_ZERO).await?;

    assert_filenode(
        &ctx,
        reader.clone(),
        &RepoPath::file("a")?,
        ONES_FNID,
        REPO_ZERO,
        file_a_first_filenode().info,
    )
    .await?;

    assert_filenode(
        &ctx,
        reader,
        &RepoPath::dir("a")?,
        ONES_FNID,
        REPO_ZERO,
        dir_a_second_filenode.info,
    )
    .await?;

    Ok(())
}
