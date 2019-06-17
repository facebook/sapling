// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Cursor, Write},
};

use blobrepo::BlobRepo;
use bytes::{Bytes, BytesMut};
use cloned::cloned;
use context::CoreContext;
use failure::{Error, Fail, Fallible};
use filenodes::FilenodeInfo;
use futures::{future::ok, stream, Future, IntoFuture, Stream};
use futures_ext::{select_all, BoxFuture, BoxStream, FutureExt, StreamExt};
use lz4_pyframe;
use maplit::hashset;
use mercurial::file::File;
use mercurial_types::{
    HgBlobNode, HgFileHistoryEntry, HgFileNodeId, HgParents, MPath, RepoPath, RevFlags, NULL_CSID,
    NULL_HASH,
};
use metaconfig_types::LfsParams;
use mononoke_types::FileContents;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "internal error: file {} copied from directory {}", _0, _1)]
    InconsistentCopyInfo(RepoPath, RepoPath),
    #[fail(
        display = "Data corruption for {}: expected {}, actual {}!",
        _0, _1, _2
    )]
    DataCorruption {
        path: RepoPath,
        expected: HgFileNodeId,
        actual: HgFileNodeId,
    },
}

/// Remotefilelog blob consists of file content in `node` revision and all the history
/// of the file up to `node`
pub fn create_remotefilelog_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    path: MPath,
    lfs_params: LfsParams,
    validate_hash: bool,
) -> BoxFuture<Bytes, Error> {
    let raw_content_bytes = get_raw_content(
        ctx.clone(),
        repo.clone(),
        node,
        RepoPath::FilePath(path.clone()),
        lfs_params,
        validate_hash,
    )
    .and_then(move |(raw_content, meta_key_flag)| {
        encode_remotefilelog_file_content(raw_content, meta_key_flag)
    });

    // Do bulk prefetch of the filenodes first. That saves lots of db roundtrips.
    // Prefetched filenodes are used as a cache. If filenode is not in the cache, then it will
    // be fetched again.
    let prefetched_filenodes = prefetch_history(ctx.clone(), repo.clone(), path.clone());

    let file_history_bytes = prefetched_filenodes
        .and_then({
            cloned!(ctx, node, path, repo);
            move |prefetched_filenodes| {
                get_file_history_using_prefetched(
                    ctx.clone(),
                    repo,
                    node,
                    path,
                    None,
                    prefetched_filenodes,
                )
                .collect()
            }
        })
        .and_then(serialize_history);

    raw_content_bytes
        .join(file_history_bytes)
        .map(|(mut raw_content, file_history)| {
            raw_content.extend(file_history);
            raw_content
        })
        .and_then(|content| lz4_pyframe::compress(&content))
        .map(|bytes| Bytes::from(bytes))
        .boxify()
}

fn validate_content(
    content: &FileContents,
    filenode: FilenodeInfo,
    repopath: RepoPath,
    actual: HgFileNodeId,
) -> Result<(), Error> {
    let mut out: Vec<u8> = vec![];
    File::generate_metadata(
        filenode
            .copyfrom
            .map(|(path, node)| (path.into_mpath().unwrap(), node))
            .as_ref(),
        content,
        &mut out,
    )?;
    let mut bytes = BytesMut::from(out);
    bytes.extend_from_slice(content.as_bytes());

    let p1 = filenode.p1.map(|p| p.into_nodehash());
    let p2 = filenode.p2.map(|p| p.into_nodehash());
    let expected = HgFileNodeId::new(HgBlobNode::new(bytes.freeze(), p1, p2).nodeid());
    if actual == expected {
        Ok(())
    } else {
        Err(ErrorKind::DataCorruption {
            path: repopath,
            expected,
            actual,
        }
        .into())
    }
}

/// Get the raw content of a file or content hash in the case of LFS files.
/// Can also optionally validate a hash hg filenode
pub fn get_raw_content(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    repopath: RepoPath,
    lfs_params: LfsParams,
    validate_hash: bool,
) -> impl Future<Item = (FileContents, RevFlags), Error = Error> {
    let filenode_fut =
        get_maybe_draft_filenode(ctx.clone(), repo.clone(), repopath.clone(), node.clone());

    repo.get_file_size(ctx.clone(), node)
        .map({
            move |file_size| match lfs_params.threshold {
                Some(threshold) => (file_size <= threshold, file_size),
                None => (true, file_size),
            }
        })
        .join(filenode_fut)
        .and_then({
            cloned!(ctx, repo);
            move |((direct_fetching_file, file_size), filenode_info)| {
                if direct_fetching_file {
                    (
                        repo.get_file_content(ctx, node)
                            .and_then(move |content| {
                                if validate_hash {
                                    validate_content(&content, filenode_info, repopath, node)
                                        .map(|()| content)
                                } else {
                                    Ok(content)
                                }
                            })
                            .left_future(),
                        Ok(RevFlags::REVIDX_DEFAULT_FLAGS).into_future(),
                    )
                } else {
                    // pass content id to prevent envelope fetching
                    cloned!(repo);
                    (
                        repo.get_file_content_id(ctx.clone(), node)
                            .and_then(move |content_id| {
                                repo.generate_lfs_file(ctx, content_id, file_size)
                            })
                            .right_future(),
                        Ok(RevFlags::REVIDX_EXTSTORED).into_future(),
                    )
                }
            }
        })
}

fn encode_remotefilelog_file_content(
    raw_content: FileContents,
    meta_key_flag: RevFlags,
) -> Result<Vec<u8>, Error> {
    let raw_content = raw_content.into_bytes();
    // requires digit counting to know for sure, use reasonable approximation
    let approximate_header_size = 12;
    let mut writer = Cursor::new(Vec::with_capacity(
        approximate_header_size + raw_content.len(),
    ));

    // Write header
    let res = write!(
        writer,
        "v1\n{}{}\n{}{}\0",
        METAKEYSIZE,
        raw_content.len(),
        METAKEYFLAG,
        meta_key_flag,
    );

    res.and_then(|_| writer.write_all(&raw_content))
        .map_err(Error::from)
        .map(|_| writer.into_inner())
}

/// Get ancestors of all filenodes
/// Current implementation might be inefficient because it might re-fetch the same filenode a few
/// times
pub fn get_unordered_file_history_for_multiple_nodes(
    ctx: CoreContext,
    repo: BlobRepo,
    filenodes: HashSet<HgFileNodeId>,
    path: &MPath,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    select_all(
        filenodes.into_iter().map(|filenode| {
            get_file_history(ctx.clone(), repo.clone(), filenode, path.clone(), None)
        }),
    )
    .filter({
        let mut used_filenodes = HashSet::new();
        move |entry| used_filenodes.insert(entry.filenode().clone())
    })
}

/// Get the history of the file corresponding to the given filenode and path.
pub fn get_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    filenode: HgFileNodeId,
    path: MPath,
    max_depth: Option<u32>,
) -> impl Stream<Item = HgFileHistoryEntry, Error = Error> {
    prefetch_history(ctx.clone(), repo.clone(), path.clone())
        .map(move |prefetched| {
            get_file_history_using_prefetched(ctx, repo, filenode, path, max_depth, prefetched)
        })
        .flatten_stream()
}

/// Prefetch and cache filenode information. Performing these fetches in bulk upfront
/// prevents an excessive number of DB roundtrips when constructing file history.
fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    path: MPath,
) -> impl Future<Item = HashMap<HgFileNodeId, FilenodeInfo>, Error = Error> {
    repo.get_all_filenodes(ctx, RepoPath::FilePath(path))
        .map(|filenodes| {
            filenodes
                .into_iter()
                .map(|filenode| (filenode.filenode, filenode))
                .collect()
        })
}

/// Get the history of the file at the specified path, using the given
/// prefetched history map as a cache to speed up the operation.
fn get_file_history_using_prefetched(
    ctx: CoreContext,
    repo: BlobRepo,
    startnode: HgFileNodeId,
    path: MPath,
    max_depth: Option<u32>,
    prefetched_history: HashMap<HgFileNodeId, FilenodeInfo>,
) -> BoxStream<HgFileHistoryEntry, Error> {
    if startnode == HgFileNodeId::new(NULL_HASH) {
        return stream::empty().boxify();
    }

    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes = hashset! {startnode};
    let path = RepoPath::FilePath(path);

    stream::unfold(
        (startstate, seen_nodes, 0),
        move |(mut nodes, mut seen_nodes, depth): (
            VecDeque<HgFileNodeId>,
            HashSet<HgFileNodeId>,
            u32,
        )| {
            match max_depth {
                Some(max_depth) if depth >= max_depth => return None,
                _ => {}
            }

            let node = nodes.pop_front()?;

            let filenode_fut = if let Some(filenode) = prefetched_history.get(&node) {
                ok(filenode.clone()).left_future()
            } else {
                get_maybe_draft_filenode(ctx.clone(), repo.clone(), path.clone(), node)
                    .right_future()
            };

            let history = filenode_fut.and_then(move |filenode| {
                let p1 = filenode.p1.map(|p| p.into_nodehash());
                let p2 = filenode.p2.map(|p| p.into_nodehash());
                let parents = HgParents::new(p1, p2);

                let linknode = filenode.linknode;

                let copyfrom = match filenode.copyfrom {
                    Some((RepoPath::FilePath(frompath), node)) => Some((frompath, node)),
                    Some((frompath, _)) => {
                        return Err(ErrorKind::InconsistentCopyInfo(filenode.path, frompath).into());
                    }
                    None => None,
                };

                let entry = HgFileHistoryEntry::new(node, parents, linknode, copyfrom);

                nodes.extend(
                    parents
                        .into_iter()
                        .map(HgFileNodeId::new)
                        .filter(|p| seen_nodes.insert(*p)),
                );
                Ok((entry, (nodes, seen_nodes, depth + 1)))
            });

            Some(history)
        },
    )
    .boxify()
}

/// Convert file history into bytes as expected in Mercurial's loose file format.
fn serialize_history(history: Vec<HgFileHistoryEntry>) -> Fallible<Vec<u8>> {
    let approximate_history_entry_size = 81;
    let mut writer = Cursor::new(Vec::<u8>::with_capacity(
        history.len() * approximate_history_entry_size,
    ));

    for entry in history {
        entry.write_to_loose_file(&mut writer)?;
    }

    Ok(writer.into_inner())
}

fn get_maybe_draft_filenode(
    ctx: CoreContext,
    repo: BlobRepo,
    path: RepoPath,
    node: HgFileNodeId,
) -> impl Future<Item = FilenodeInfo, Error = Error> {
    repo.get_filenode_opt(ctx.clone(), &path, node).and_then({
        cloned!(repo, ctx, path, node);
        move |filenode_opt| match filenode_opt {
            Some(filenode) => ok(filenode).left_future(),
            None => {
                // The filenode couldn't be found.  This may be because it is a
                // draft node, which doesn't get stored in the database.  Attempt
                // to reconstruct the filenode from the envelope.  Use `NULL_CSID`
                // to indicate a draft linknode.
                repo.get_filenode_from_envelope(ctx, &path, node, NULL_CSID)
                    .right_future()
            }
        }
    })
}
