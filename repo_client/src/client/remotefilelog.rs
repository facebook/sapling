// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Write};

use bytes::{Bytes, BytesMut};
use futures::{future::ok, stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use pylz4;

use blobrepo::BlobRepo;
use filenodes::FilenodeInfo;

use context::CoreContext;
use mercurial::file::File;
use mercurial_types::{
    HgBlobNode, HgChangesetId, HgFileNodeId, HgNodeHash, HgParents, MPath, RepoPath, RevFlags,
    NULL_CSID, NULL_HASH,
};

use metaconfig_types::LfsParams;
use tracing::Traced;

use errors::*;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";

/// Remotefilelog blob consists of file content in `node` revision and all the history
/// of the file up to `node`
pub fn create_remotefilelog_blob(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgNodeHash,
    path: MPath,
    lfs_params: LfsParams,
    validate_hash: bool,
) -> BoxFuture<Bytes, Error> {
    let trace_args = trace_args!("node" => node.to_string(), "path" => path.to_string());

    // raw_content includes copy information
    let raw_content_bytes = repo
        .get_file_size(ctx.clone(), &HgFileNodeId::new(node))
        .map({
            move |file_size| match lfs_params.threshold {
                Some(threshold) => (file_size <= threshold, file_size),
                None => (true, file_size),
            }
        })
        .and_then({
            cloned!(ctx, repo);
            move |(direct_fetching_file, file_size)| {
                if direct_fetching_file {
                    (
                        repo.get_file_content(ctx, &node).left_future(),
                        Ok(RevFlags::REVIDX_DEFAULT_FLAGS).into_future(),
                    )
                } else {
                    // pass content id to prevent envelope fetching
                    cloned!(repo);
                    (
                        repo.get_file_content_id(ctx.clone(), &HgFileNodeId::new(node))
                            .and_then(move |content_id| {
                                repo.generate_lfs_file(ctx, content_id, file_size)
                            })
                            .right_future(),
                        Ok(RevFlags::REVIDX_EXTSTORED).into_future(),
                    )
                }
            }
        })
        .and_then(move |(raw_content, meta_key_flag)| {
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
        })
        .traced(
            ctx.trace(),
            "fetching remotefilelog content",
            trace_args.clone(),
        );

    // Do bulk prefetch of the filenodes first. That saves lots of db roundtrips.
    // Prefetched filenodes are used as a cache. If filenode is not in the cache, then it will
    // be fetched again.
    let prefetched_filenodes = repo
        .get_all_filenodes(ctx.clone(), RepoPath::FilePath(path.clone()))
        .map(move |filenodes| {
            filenodes
                .into_iter()
                .map(|filenode| (filenode.filenode.into_nodehash(), filenode))
                .collect()
        })
        .traced(ctx.trace(), "prefetching file history", trace_args.clone());

    let file_history_bytes = prefetched_filenodes
        .and_then({
            cloned!(ctx, node, path, repo, trace_args);
            move |prefetched_filenodes| {
                get_file_history(ctx.clone(), repo, node, path, prefetched_filenodes)
                    .collect()
                    .traced(ctx.trace(), "fetching non-prefetched history", trace_args)
            }
        })
        .and_then(|history| {
            let approximate_history_entry_size = 81;
            let mut writer = Cursor::new(Vec::with_capacity(
                history.len() * approximate_history_entry_size,
            ));

            for (node, parents, linknode, copy) in history {
                let (p1, p2) = match parents {
                    HgParents::None => (NULL_HASH, NULL_HASH),
                    HgParents::One(p) => (p, NULL_HASH),
                    HgParents::Two(p1, p2) => (p1, p2),
                };

                let (p1, p2, copied_from) = if let Some((copied_from, copied_rev)) = copy {
                    // Mercurial has a complicated copy/renames logic.
                    // If (path1, filenode1) is copied/renamed from (path2, filenode2),
                    // filenode1's p1 is set to filenode2, and copy_from path is set to path2
                    // filenode1's p2 is null for non-merge commits. It might be non-null for merges.
                    (copied_rev, p1, Some(copied_from))
                } else {
                    (p1, p2, None)
                };

                writer.write_all(node.as_bytes())?;
                writer.write_all(p1.as_bytes())?;
                writer.write_all(p2.as_bytes())?;
                writer.write_all(linknode.into_nodehash().as_bytes())?;
                if let Some(copied_from) = copied_from {
                    writer.write_all(&copied_from.to_vec())?;
                }

                write!(writer, "\0")?;
            }
            Ok(writer.into_inner())
        })
        .traced(ctx.trace(), "fetching file history", trace_args);

    let validate_content = if validate_hash {
        validate_content(ctx, repo.clone(), path, node).left_future()
    } else {
        ok(()).right_future()
    };

    validate_content
        .and_then(|()| {
            raw_content_bytes
                .join(file_history_bytes)
                .map(|(mut raw_content, file_history)| {
                    raw_content.extend(file_history);
                    raw_content
                })
                .and_then(|content| pylz4::compress(&content))
                .map(|bytes| Bytes::from(bytes))
        })
        .boxify()
}

fn validate_content(
    ctx: CoreContext,
    repo: BlobRepo,
    path: MPath,
    actual: HgNodeHash,
) -> impl Future<Item = (), Error = Error> {
    let file_content = repo.get_file_content(ctx.clone(), &actual);
    let repopath = RepoPath::FilePath(path.clone());
    let filenode = get_maybe_draft_filenode(ctx, repo, repopath.clone(), actual);

    file_content
        .join(filenode)
        .and_then(move |(content, filenode)| {
            let mut out: Vec<u8> = vec![];
            File::generate_metadata(
                filenode
                    .copyfrom
                    .map(|(path, node)| (path.into_mpath().unwrap(), node.into_nodehash()))
                    .as_ref(),
                &content,
                &mut out,
            )?;
            let mut bytes = BytesMut::from(out);
            bytes.extend_from_slice(&content.into_bytes());

            let p1 = filenode.p1.map(|p| p.into_nodehash());
            let p2 = filenode.p2.map(|p| p.into_nodehash());
            let expected = HgBlobNode::new(bytes.freeze(), p1.as_ref(), p2.as_ref()).nodeid();
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
        })
}

fn get_file_history(
    ctx: CoreContext,
    repo: BlobRepo,
    startnode: HgNodeHash,
    path: MPath,
    prefetched_history: HashMap<HgNodeHash, FilenodeInfo>,
) -> BoxStream<
    (
        HgNodeHash,
        HgParents,
        HgChangesetId,
        Option<(MPath, HgNodeHash)>,
    ),
    Error,
> {
    if startnode == NULL_HASH {
        return stream::empty().boxify();
    }
    let mut startstate = VecDeque::new();
    startstate.push_back(startnode);
    let seen_nodes: HashSet<_> = [startnode].iter().cloned().collect();
    let path = RepoPath::FilePath(path);

    stream::unfold(
        (startstate, seen_nodes),
        move |(mut nodes, mut seen_nodes): (VecDeque<HgNodeHash>, HashSet<HgNodeHash>)| {
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
                let parents = HgParents::new(p1.as_ref(), p2.as_ref());

                let linknode = filenode.linknode;

                let copyfrom = match filenode.copyfrom {
                    Some((RepoPath::FilePath(frompath), node)) => {
                        Some((frompath, node.into_nodehash()))
                    }
                    Some((frompath, _)) => {
                        return Err(ErrorKind::InconsistentCopyInfo(filenode.path, frompath).into());
                    }
                    None => None,
                };

                nodes.extend(parents.into_iter().filter(|p| seen_nodes.insert(*p)));
                Ok(((node, parents, linknode, copyfrom), (nodes, seen_nodes)))
            });

            Some(history)
        },
    )
    .boxify()
}

fn get_maybe_draft_filenode(
    ctx: CoreContext,
    repo: BlobRepo,
    path: RepoPath,
    node: HgNodeHash,
) -> impl Future<Item = FilenodeInfo, Error = Error> {
    repo.get_filenode_opt(ctx.clone(), &path, &node).and_then({
        cloned!(repo, ctx, path, node);
        move |filenode_opt| match filenode_opt {
            Some(filenode) => ok(filenode).left_future(),
            None => {
                // The filenode couldn't be found.  This may be because it is a
                // draft node, which doesn't get stored in the database.  Attempt
                // to reconstruct the filenode from the envelope.  Use `NULL_CSID`
                // to indicate a draft linknode.
                repo.get_filenode_from_envelope(ctx, &path, &node, &NULL_CSID)
                    .right_future()
            }
        }
    })
}
