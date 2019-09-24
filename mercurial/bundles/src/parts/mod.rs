// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::changegroup::packer::CgPacker;
use super::changegroup::{CgDeltaChunk, Part, Section};
use super::chunk::Chunk;
use super::obsmarkers::packer::obsmarkers_packer_stream;
use super::obsmarkers::MetadataEntry;
use super::wirepack;
use super::wirepack::packer::WirePackPacker;
use crate::errors::*;
use crate::part_encode::PartEncodeBuilder;
use crate::part_header::{PartHeaderType, PartId};
use byteorder::{BigEndian, WriteBytesExt};
use bytes::Bytes;
use context::CoreContext;
use failure_ext::prelude::*;
use futures::stream::{iter_ok, once};
use futures::{Future, Stream};
use futures_ext::BoxFuture;
use futures_stats::Timed;
use mercurial_types::{
    Delta, HgBlobNode, HgChangesetId, HgFileNodeId, HgNodeHash, HgPhase, MPath, RepoPath, NULL_HASH,
};
use mononoke_types::DateTime;
use scuba_ext::ScubaSampleBuilderExt;
use std::fmt;
use std::io::Write;

pub fn listkey_part<N, S, K, V>(namespace: N, items: S) -> Result<PartEncodeBuilder>
where
    N: Into<Bytes>,
    S: Stream<Item = (K, V), Error = Error> + Send + 'static,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Listkeys)?;
    builder.add_mparam("namespace", namespace)?;
    // Ideally we'd use a size_hint here, but streams don't appear to have one.
    let payload = Vec::with_capacity(256);
    let fut = items
        .fold(payload, |mut payload, (key, value)| {
            payload.extend_from_slice(key.as_ref());
            payload.push(b'\t');
            payload.extend_from_slice(value.as_ref());
            payload.push(b'\n');
            Ok::<_, Error>(payload)
        })
        .map_err(|err| Error::from(err.chain_err(ErrorKind::ListkeyGeneration)));

    builder.set_data_future(fut);

    Ok(builder)
}

pub fn phases_part<S>(ctx: CoreContext, phases_entries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = (HgChangesetId, HgPhase), Error = Error> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::PhaseHeads)?;
    let mut scuba_logger = ctx.scuba().clone();
    let payload = Vec::with_capacity(1024);
    let fut = phases_entries
        .fold(payload, |mut payload, (value, phase)| {
            payload.write_u32::<BigEndian>(phase as u32)?;
            payload.write(value.as_ref())?;
            Ok::<_, Error>(payload)
        })
        .map_err(|err| Error::from(err.chain_err(ErrorKind::PhaseHeadsGeneration)))
        .timed(move |stats, result| {
            if result.is_ok() {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Phases calculated - Success", None);
            }
            Ok(())
        });
    builder.set_data_future(fut);
    Ok(builder)
}

pub fn changegroup_part<S>(changelogentries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = (HgNodeHash, HgBlobNode), Error = Error> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Changegroup)?;
    builder.add_mparam("version", "02")?;

    let changelogentries = convert_changeset_stream(changelogentries)
        .chain(once(Ok(Part::SectionEnd(Section::Changeset))))
        // One more SectionEnd entry is necessary because hg client excepts filelog section
        // even if it's empty. Add a fake SectionEnd part (the choice of
        // Manifest is just for convenience).
        .chain(once(Ok(Part::SectionEnd(Section::Manifest))))
        .chain(once(Ok(Part::End)));

    let cgdata = CgPacker::new(changelogentries);
    builder.set_data_generated(cgdata);

    Ok(builder)
}

pub fn pushrebase_changegroup_part<CS, FS>(
    changelogentries: CS,
    filenodeentries: FS,
    onto_bookmark: impl AsRef<str>,
) -> Result<PartEncodeBuilder>
where
    CS: Stream<Item = (HgNodeHash, HgBlobNode), Error = Error> + Send + 'static,
    FS: Stream<Item = (MPath, Vec<(HgFileNodeId, HgChangesetId, HgBlobNode)>), Error = Error>
        + Send
        + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::B2xRebase)?;
    builder.add_mparam("onto", onto_bookmark.as_ref())?;
    builder.add_aparam("cgversion", "02")?;

    let changelogentries = convert_changeset_stream(changelogentries)
        .chain(once(Ok(Part::SectionEnd(Section::Changeset))))
        // One more SectionEnd entry is necessary because hg client excepts filelog section
        // even if it's empty. Add a fake SectionEnd part (the choice of
        // Manifest is just for convenience).
        .chain(once(Ok(Part::SectionEnd(Section::Manifest))))
        .chain(convert_file_stream(filenodeentries))
        .chain(once(Ok(Part::End)));

    let cgdata = CgPacker::new(changelogentries);
    builder.set_data_generated(cgdata);

    Ok(builder)
}

fn convert_changeset_stream<S>(changelogentries: S) -> impl Stream<Item = Part, Error = Error>
where
    S: Stream<Item = (HgNodeHash, HgBlobNode), Error = Error> + Send + 'static,
{
    changelogentries.map(|(node, blobnode)| {
        let parents = blobnode.parents().get_nodes();
        let p1 = parents.0.unwrap_or(NULL_HASH);
        let p2 = parents.1.unwrap_or(NULL_HASH);
        let base = NULL_HASH;
        // Linknode is the same as node
        let linknode = node;
        let text = blobnode.as_blob().as_inner().clone();
        let delta = Delta::new_fulltext(text.to_vec());

        let deltachunk = CgDeltaChunk {
            node,
            p1,
            p2,
            base,
            linknode,
            delta,
            flags: None,
        };
        Part::CgChunk(Section::Changeset, deltachunk)
    })
}

fn convert_file_stream<FS>(filenodeentries: FS) -> impl Stream<Item = Part, Error = Error>
where
    FS: Stream<Item = (MPath, Vec<(HgFileNodeId, HgChangesetId, HgBlobNode)>), Error = Error>
        + Send
        + 'static,
{
    filenodeentries
        .map(|(path, nodes)| {
            let mut items = vec![];
            for (node, hg_cs_id, blobnode) in nodes {
                let parents = blobnode.parents().get_nodes();
                let p1 = parents.0.unwrap_or(NULL_HASH);
                let p2 = parents.1.unwrap_or(NULL_HASH);
                let base = NULL_HASH;
                // Linknode is the same as node
                let linknode = hg_cs_id.into_nodehash();
                let text = blobnode.as_blob().as_inner().clone();
                let delta = Delta::new_fulltext(text.to_vec());

                let deltachunk = CgDeltaChunk {
                    node: node.into_nodehash(),
                    p1,
                    p2,
                    base,
                    linknode,
                    delta,
                    // TODO(stash) - need flags for lfs
                    flags: None,
                };
                items.push(Part::CgChunk(Section::Filelog(path.clone()), deltachunk));
            }

            items.push(Part::SectionEnd(Section::Filelog(path)));
            iter_ok(items)
        })
        .flatten()
}

pub fn replycaps_part(caps: Bytes) -> Result<PartEncodeBuilder> {
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Replycaps)?;
    builder.set_data_fixed(Chunk::new(caps)?);

    Ok(builder)
}

pub fn common_heads_part(heads: Vec<HgChangesetId>) -> Result<PartEncodeBuilder> {
    let mut w = Vec::new();
    for h in heads {
        w.extend(h.as_bytes());
    }

    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::B2xCommonHeads)?;
    builder.set_data_fixed(Chunk::new(w)?);

    Ok(builder)
}

pub struct TreepackPartInput {
    pub node: HgNodeHash,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub content: Bytes,
    pub fullpath: Option<MPath>,
    pub linknode: HgNodeHash,
}

pub fn treepack_part<S>(entries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = BoxFuture<TreepackPartInput, Error>, Error = Error> + Send + 'static,
{
    treepack_part_impl(entries, PartHeaderType::B2xTreegroup2)
}

pub fn pushrebase_treepack_part<S>(entries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = BoxFuture<TreepackPartInput, Error>, Error = Error> + Send + 'static,
{
    treepack_part_impl(entries, PartHeaderType::B2xRebasePack)
}

fn treepack_part_impl<S>(entries: S, header_type: PartHeaderType) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = BoxFuture<TreepackPartInput, Error>, Error = Error> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(header_type)?;
    builder.add_mparam("version", "1")?;
    builder.add_mparam("cache", "True")?;
    builder.add_mparam("category", "manifests")?;

    let buffer_size = 10000; // TODO(stash): make it configurable
    let wirepack_parts = entries
        .buffered(buffer_size)
        .map(|input| {
            let path = match input.fullpath {
                Some(path) => RepoPath::DirectoryPath(path),
                None => RepoPath::RootPath,
            };

            let history_meta = wirepack::Part::HistoryMeta {
                path: path.clone(),
                entry_count: 1,
            };

            let history = wirepack::Part::History(wirepack::HistoryEntry {
                node: input.node.clone(),
                p1: input.p1.into(),
                p2: input.p2.into(),
                linknode: input.linknode,
                // No copies/renames for trees
                copy_from: None,
            });

            let data_meta = wirepack::Part::DataMeta {
                path,
                entry_count: 1,
            };

            let data = wirepack::Part::Data(wirepack::DataEntry {
                node: input.node,
                delta_base: NULL_HASH,
                delta: Delta::new_fulltext(input.content.to_vec()),
                metadata: None,
            });

            iter_ok(vec![history_meta, history, data_meta, data].into_iter())
        })
        .flatten()
        .chain(once(Ok(wirepack::Part::End)));

    let packer = WirePackPacker::new(wirepack_parts, wirepack::Kind::Tree);
    builder.set_data_generated(packer);

    Ok(builder)
}

pub enum ChangegroupApplyResult {
    Success { heads_num_diff: i64 },
    Error,
}

// Mercurial source code comments are a bit contradictory:
//
// From mercurial/changegroup.py
// Return an integer summarizing the change to this repo:
// - nothing changed or no source: 0
// - more heads than before: 1+added heads (2..n)
// - fewer heads than before: -1-removed heads (-2..-n)
// - number of heads stays the same: 1
//
// From mercurial/exchange.py
// Integer version of the changegroup push result
// - None means nothing to push
// - 0 means HTTP error
// - 1 means we pushed and remote head count is unchanged *or*
//   we have outgoing changesets but refused to push
// - other values as described by addchangegroup()
//
// We are using 0 to indicate a error, 1 + heads_num_diff if the number of heads increased,
// -1 + heads_num_diff if the number of heads decreased. Note that we may change it in the future

impl fmt::Display for ChangegroupApplyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            &ChangegroupApplyResult::Success { heads_num_diff } => {
                if heads_num_diff >= 0 {
                    write!(f, "{}", 1 + heads_num_diff)
                } else {
                    write!(f, "{}", -1 + heads_num_diff)
                }
            }
            &ChangegroupApplyResult::Error => write!(f, "0"),
        }
    }
}

pub fn replychangegroup_part(
    res: ChangegroupApplyResult,
    in_reply_to: PartId,
) -> Result<PartEncodeBuilder> {
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::ReplyChangegroup)?;
    builder.add_mparam("return", format!("{}", res))?;
    builder.add_mparam("in-reply-to", format!("{}", in_reply_to))?;

    Ok(builder)
}

pub fn bookmark_pushkey_part(key: String, old: String, new: String) -> Result<PartEncodeBuilder> {
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Pushkey)?;
    builder.add_mparam("namespace", "bookmarks")?;
    builder.add_mparam("key", key)?;
    builder.add_mparam("old", old)?;
    builder.add_mparam("new", new)?;

    Ok(builder)
}

pub fn replypushkey_part(res: bool, in_reply_to: PartId) -> Result<PartEncodeBuilder> {
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::ReplyPushkey)?;
    if res {
        builder.add_mparam("return", "1")?;
    } else {
        builder.add_mparam("return", "0")?;
    }
    builder.add_mparam("in-reply-to", format!("{}", in_reply_to))?;

    Ok(builder)
}

pub fn obsmarkers_part<S>(
    pairs: S,
    time: DateTime,
    metadata: Vec<MetadataEntry>,
) -> Result<PartEncodeBuilder>
where
    S: 'static + Stream<Item = (HgChangesetId, Vec<HgChangesetId>), Error = Error> + Send,
{
    let stream = obsmarkers_packer_stream(pairs, time, metadata);

    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Obsmarkers)?;
    builder.set_data_generated(stream);
    Ok(builder)
}
