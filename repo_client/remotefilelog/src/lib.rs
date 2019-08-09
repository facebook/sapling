// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use std::{
    collections::HashSet,
    io::{Cursor, Write},
};

use blobrepo::{
    file_history::{get_file_history, get_maybe_draft_filenode},
    BlobRepo,
};
use bytes::{Bytes, BytesMut};
use censoredblob::ErrorKind::Censored;
use cloned::cloned;
use context::CoreContext;
use failure::{Error, Fail, Fallible};
use filenodes::FilenodeInfo;
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{select_all, BoxFuture, FutureExt};
use lz4_pyframe;
use mercurial::file::File;
use mercurial_types::{
    FileBytes, HgBlobNode, HgFileHistoryEntry, HgFileNodeId, MPath, RepoPath, RevFlags,
};
use metaconfig_types::LfsParams;

const METAKEYFLAG: &str = "f";
const METAKEYSIZE: &str = "s";
/// Tombstone string to replace the content of blacklisted files with
const CENSORED_CONTENT: &str =
    "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

#[derive(Debug, Fail)]
pub enum ErrorKind {
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
    .or_else(move |err| {
        let root_cause = err.find_root_cause();
        let maybe_censored_err = root_cause.downcast_ref::<censoredblob::ErrorKind>();

        // if the error is Censored return a magic string as the new content
        match maybe_censored_err {
            Some(Censored(_, _)) => {
                let meta_key_flag = RevFlags::REVIDX_DEFAULT_FLAGS;
                let raw_content = FileBytes(CENSORED_CONTENT.as_bytes().into());
                future::ok((raw_content, meta_key_flag)).right_future()
            }
            None => future::err(err).left_future(),
        }
    })
    .and_then(move |(raw_content, meta_key_flag)| {
        encode_remotefilelog_file_content(raw_content, meta_key_flag)
    });

    let file_history_bytes = get_file_history(ctx, repo, node, path, None)
        .collect()
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

fn extract_copy_from<'a>(filenode: &'a FilenodeInfo) -> Option<(MPath, HgFileNodeId)> {
    filenode
        .copyfrom
        .clone()
        .map(|(path, node)| (path.into_mpath().unwrap(), node))
}

fn validate_content(
    content: &FileBytes,
    filenode: FilenodeInfo,
    repopath: RepoPath,
    actual: HgFileNodeId,
) -> Result<(), Error> {
    let mut out: Vec<u8> = vec![];
    File::generate_metadata(extract_copy_from(&filenode).as_ref(), content, &mut out)?;
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
fn get_raw_content(
    ctx: CoreContext,
    repo: BlobRepo,
    node: HgFileNodeId,
    repopath: RepoPath,
    lfs_params: LfsParams,
    validate_hash: bool,
) -> impl Future<Item = (FileBytes, RevFlags), Error = Error> {
    let filenode_fut =
        get_maybe_draft_filenode(ctx.clone(), repo.clone(), repopath.clone(), node.clone());

    repo.get_file_envelope(ctx.clone(), node)
        .join(filenode_fut)
        .and_then({
            cloned!(ctx, repo);
            move |(envelope, filenode_info)| {
                let file_size = envelope.content_size();

                let direct_fetching_file = match lfs_params.threshold {
                    Some(threshold) => (file_size <= threshold),
                    None => true,
                };

                if direct_fetching_file {
                    (
                        repo.get_file_content(ctx, node)
                            .concat2()
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
                    let copy_from =
                        extract_copy_from(&filenode_info).map(|copy_from| copy_from.clone());

                    let file_fut = repo
                        .get_file_sha256(ctx, envelope.content_id())
                        .and_then(move |oid| {
                            File::generate_lfs_file(oid, envelope.content_size(), copy_from)
                        })
                        .map(|bytes| FileBytes(bytes));

                    (
                        file_fut.right_future(),
                        Ok(RevFlags::REVIDX_EXTSTORED).into_future(),
                    )
                }
            }
        })
}

fn encode_remotefilelog_file_content(
    raw_content: FileBytes,
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
