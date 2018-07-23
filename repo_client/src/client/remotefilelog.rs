// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Write};
use std::sync::Arc;

use bytes::Bytes;
use futures::{stream, Future, IntoFuture, Stream, future::Either};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use pylz4;

use blobrepo::BlobRepo;
use filenodes::FilenodeInfo;
use mercurial_types::{HgChangesetId, HgNodeHash, HgParents, MPath, RepoPath, NULL_HASH};
use tracing::{TraceContext, Traced};

use errors::*;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";

/// Remotefilelog blob consists of file content in `node` revision and all the history
/// of the file up to `node`
pub fn create_remotefilelog_blob(
    repo: Arc<BlobRepo>,
    node: HgNodeHash,
    path: MPath,
    trace: TraceContext,
) -> BoxFuture<Bytes, Error> {
    // raw_content includes copy information
    let raw_content_bytes = repo.get_file_content(&node)
        .and_then(move |raw_content| {
            let raw_content = raw_content.into_bytes();
            // requires digit counting to know for sure, use reasonable approximation
            let approximate_header_size = 12;
            let mut writer = Cursor::new(Vec::with_capacity(
                approximate_header_size + raw_content.len(),
            ));

            // Write header
            // TODO(stash): support LFS files using METAKEYFLAG
            let res = write!(
                writer,
                "v1\n{}{}\n{}{}\0",
                METAKEYSIZE,
                raw_content.len(),
                METAKEYFLAG,
                0,
            );

            res.and_then(|_| writer.write_all(&raw_content))
                .map_err(Error::from)
                .map(|_| writer.into_inner())
        })
        .traced(&trace, "fetching remotefilelog content", trace_args!());

    // Do bulk prefetch of the filenodes first. That saves lots of db roundtrips.
    // Prefetched filenodes are used as a cache. If filenode is not in the cache, then it will
    // be fetched again.
    let prefetched_filenodes = repo.get_all_filenodes(RepoPath::FilePath(path.clone()))
        .map(|filenodes| {
            filenodes
                .into_iter()
                .map(|filenode| (filenode.filenode.into_nodehash(), filenode))
                .collect()
        });

    let file_history_bytes = prefetched_filenodes
        .and_then({
            let node = node.clone();
            let trace = trace.clone();
            move |prefetched_filenodes| {
                get_file_history(repo, node, path, prefetched_filenodes, trace).collect()
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
        .traced(&trace, "fetching file history", trace_args!());

    raw_content_bytes
        .join(file_history_bytes)
        .map(|(mut raw_content, file_history)| {
            raw_content.extend(file_history);
            raw_content
        })
        .and_then(|content| pylz4::compress(&content))
        .map(|bytes| Bytes::from(bytes))
        .boxify()
}

fn get_file_history(
    repo: Arc<BlobRepo>,
    startnode: HgNodeHash,
    path: MPath,
    prefetched_history: HashMap<HgNodeHash, FilenodeInfo>,
    trace: TraceContext,
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
        move |cur_data: (VecDeque<HgNodeHash>, HashSet<HgNodeHash>)| {
            let (mut nodes, mut seen_nodes) = cur_data;
            let node = nodes.pop_front()?;

            let futs = if prefetched_history.contains_key(&node) {
                let filenode = prefetched_history.get(&node).unwrap();

                let p1 = filenode.p1.map(|p| p.into_nodehash());
                let p2 = filenode.p2.map(|p| p.into_nodehash());
                let parents = Ok(HgParents::new(p1.as_ref(), p2.as_ref())).into_future();

                let linknode = Ok(filenode.linknode).into_future();

                let copy =
                    Ok(filenode
                        .copyfrom
                        .clone()
                        .map(|(frompath, node)| (frompath, node.into_nodehash())))
                        .into_future();

                (Either::A(parents), Either::A(linknode), Either::A(copy))
            } else {
                let parents = repo.get_parents(&path, &node);
                let parents = parents.traced(&trace, "fetching parents", trace_args!());

                let copy = repo.get_file_copy(&path, &node);
                let copy = copy.traced(&trace, "fetching copy info", trace_args!());

                let linknode = Ok(path.clone()).into_future().and_then({
                    let repo = repo.clone();
                    move |path| repo.get_linknode(path, &node)
                });
                let linknode = linknode.traced(&trace, "fetching linknode info", trace_args!());

                (Either::B(parents), Either::B(linknode), Either::B(copy))
            };

            let (parents, linknode, copy) = futs;

            let copy = copy.and_then({
                let path = path.clone();
                move |filecopy| match filecopy {
                    Some((RepoPath::FilePath(copyto), rev)) => Ok(Some((copyto, rev))),
                    Some((copyto, _)) => Err(ErrorKind::InconsistenCopyInfo(path, copyto).into()),
                    None => Ok(None),
                }
            });

            let joined = parents
                .join(linknode)
                .join(copy)
                .map(|(pl, c)| (pl.0, pl.1, c));

            Some(joined.map(move |(parents, linknode, copy)| {
                nodes.extend(parents.into_iter().filter(|p| seen_nodes.insert(*p)));
                ((node, parents, linknode, copy), (nodes, seen_nodes))
            }))
        },
    ).boxify()
}
