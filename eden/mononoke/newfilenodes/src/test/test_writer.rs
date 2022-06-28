/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use context::PerfCounterType;
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use filenodes::PreparedFilenode;
use mercurial_types_mocks::nodehash::ONES_CSID;
use mercurial_types_mocks::nodehash::ONES_FNID;
use mononoke_types::RepoPath;
use mononoke_types_mocks::repo::REPO_ZERO;

use super::util::build_reader_writer;
use super::util::build_shard;

#[fbinit::test]
async fn test_batching(fb: FacebookInit) -> Result<(), Error> {
    let (_, writer) = build_reader_writer(vec![build_shard()?, build_shard()?]);

    let ctx = CoreContext::test_mock(fb);

    let filenodes = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
        .into_iter()
        .map(|p| PreparedFilenode {
            path: RepoPath::file(p).unwrap(),
            info: FilenodeInfo {
                filenode: ONES_FNID,
                p1: None,
                p2: None,
                copyfrom: None,
                linknode: ONES_CSID,
            },
        })
        .collect();

    writer
        .insert_filenodes(&ctx, REPO_ZERO, filenodes, false)
        .await?
        .do_not_handle_disabled_filenodes()?;

    // 1 for paths, 1 for filenodes, per shard.
    assert_eq!(
        ctx.perf_counters().get_counter(PerfCounterType::SqlWrites),
        4
    );

    Ok(())
}

#[fbinit::test]
async fn test_no_empty_queries(fb: FacebookInit) -> Result<(), Error> {
    let (_, writer) = build_reader_writer(vec![build_shard()?, build_shard()?]);

    let ctx = CoreContext::test_mock(fb);

    let filenodes = vec![PreparedFilenode {
        path: RepoPath::file("a").unwrap(),
        info: FilenodeInfo {
            filenode: ONES_FNID,
            p1: None,
            p2: None,
            copyfrom: None,
            linknode: ONES_CSID,
        },
    }];

    writer
        .insert_filenodes(&ctx, REPO_ZERO, filenodes, false)
        .await?
        .do_not_handle_disabled_filenodes()?;

    // 1 for paths, 1 for filenodes.
    assert_eq!(
        ctx.perf_counters().get_counter(PerfCounterType::SqlWrites),
        2
    );

    Ok(())
}
