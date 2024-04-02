/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::Write;

use anyhow::Error;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::WriteBytesExt;
use bytes::Bytes;
use context::CoreContext;
use futures::future;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::BoxStream;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use mercurial_mutation::HgMutationEntry;
use mercurial_types::Delta;
use mercurial_types::HgBlobNode;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mercurial_types::NonRootMPath;
use mercurial_types::RepoPath;
use mercurial_types::RevFlags;
use mercurial_types::NULL_HASH;
use mononoke_types::path::MPath;
use mononoke_types::DateTime;
use phases::Phase;

use super::changegroup::packer::changegroup_packer;
use super::changegroup::unpacker::CgVersion;
use super::changegroup::CgDeltaChunk;
use super::changegroup::Part;
use super::changegroup::Section;
use super::chunk::Chunk;
use super::infinitepush::infinitepush_mutation_packer;
use super::obsmarkers::packer::obsmarkers_packer_stream;
use super::obsmarkers::MetadataEntry;
use super::wirepack;
use super::wirepack::packer::pack_wirepack;
use crate::errors::ErrorKind;
use crate::part_encode::PartEncodeBuilder;
use crate::part_header::PartHeaderType;
use crate::part_header::PartId;

pub type FilenodeEntry = (HgFileNodeId, HgChangesetId, HgBlobNode, Option<RevFlags>);

pub fn listkey_part<N, S, K, V>(namespace: N, items: S) -> Result<PartEncodeBuilder>
where
    N: Into<Bytes>,
    S: Stream<Item = Result<(K, V), Error>> + Send + 'static,
    K: AsRef<[u8]> + Send + 'static,
    V: AsRef<[u8]> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Listkeys)?;
    builder.add_mparam("namespace", namespace)?;
    // Ideally we'd use a size_hint here, but streams don't appear to have one.
    let payload = Vec::with_capacity(256);
    let fut = items
        .try_fold(payload, |mut payload, (key, value)| async move {
            payload.extend_from_slice(key.as_ref());
            payload.push(b'\t');
            payload.extend_from_slice(value.as_ref());
            payload.push(b'\n');
            anyhow::Ok(payload)
        })
        .map_err(|err| err.context(ErrorKind::ListkeyGeneration));

    builder.set_data_future(fut);

    Ok(builder)
}

pub fn phases_part<S>(ctx: CoreContext, phases_entries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = Result<(HgChangesetId, Phase), Error>> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::PhaseHeads)?;
    let mut scuba_logger = ctx.scuba().clone();
    let payload = Vec::with_capacity(1024);
    let fut = phases_entries
        .try_fold(payload, |mut payload, (value, phase)| async move {
            payload.write_u32::<BigEndian>(u32::from(phase))?;
            payload.write_all(value.as_ref())?;
            anyhow::Ok(payload)
        })
        .map_err(|err| err.context(ErrorKind::PhaseHeadsGeneration))
        .timed()
        .map(move |(stats, result)| {
            if result.is_ok() {
                scuba_logger
                    .add_future_stats(&stats)
                    .log_with_msg("Phases calculated - Success", None);
            }
            result
        });
    builder.set_data_future(fut);
    Ok(builder)
}

pub fn changegroup_part<CS>(
    changelogentries: CS,
    filenodeentries: Option<BoxStream<'static, Result<(NonRootMPath, Vec<FilenodeEntry>), Error>>>,
    version: CgVersion,
) -> Result<PartEncodeBuilder>
where
    CS: Stream<Item = Result<(HgNodeHash, HgBlobNode), Error>> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Changegroup)?;
    builder.add_mparam(
        "version",
        Bytes::copy_from_slice(version.to_str().as_bytes()),
    )?;

    let changelogentries = convert_changeset_stream(changelogentries, version)
        .chain(stream::once(future::ok(Part::SectionEnd(
            Section::Changeset,
        ))))
        // One more SectionEnd entry is necessary because hg client excepts filelog section
        // even if it's empty. Add a fake SectionEnd part (the choice of
        // Manifest is just for convenience).
        .chain(stream::once(future::ok(Part::SectionEnd(
            Section::Manifest,
        ))));

    let changelogentries = if version == CgVersion::Cg3Version {
        // Changegroup V3 requires one empty chunk after manifest section
        // hence adding Part::SectionEnd below
        changelogentries
            .chain(stream::once(future::ok(Part::SectionEnd(
                Section::Manifest,
            ))))
            .left_stream()
    } else {
        changelogentries.right_stream()
    };

    let changegroup = if let Some(filenodeentries) = filenodeentries {
        changelogentries
            .chain(convert_file_stream(filenodeentries, version))
            .left_stream()
    } else {
        changelogentries.right_stream()
    };

    let changegroup = changegroup.chain(stream::once(future::ok(Part::End)));

    builder.set_data_generated(changegroup_packer(changegroup));

    Ok(builder)
}

fn convert_changeset_stream<S>(
    changelogentries: S,
    version: CgVersion,
) -> impl Stream<Item = Result<Part, Error>>
where
    S: Stream<Item = Result<(HgNodeHash, HgBlobNode), Error>> + Send + 'static,
{
    changelogentries.map_ok(move |(node, blobnode)| {
        let parents = blobnode.parents().get_nodes();
        let p1 = parents.0.unwrap_or(NULL_HASH);
        let p2 = parents.1.unwrap_or(NULL_HASH);
        let base = NULL_HASH;
        // Linknode is the same as node
        let linknode = node;
        let text = blobnode.as_blob().as_inner().clone();
        let delta = Delta::new_fulltext(text.to_vec());

        let flags = if version == CgVersion::Cg3Version {
            Some(RevFlags::REVIDX_DEFAULT_FLAGS)
        } else {
            None
        };

        let deltachunk = CgDeltaChunk {
            node,
            p1,
            p2,
            base,
            linknode,
            delta,
            flags,
        };
        Part::CgChunk(Section::Changeset, deltachunk)
    })
}

fn convert_file_stream<FS>(
    filenodeentries: FS,
    cg_version: CgVersion,
) -> impl Stream<Item = Result<Part, Error>>
where
    FS: Stream<Item = Result<(NonRootMPath, Vec<FilenodeEntry>), Error>> + Send + 'static,
{
    filenodeentries
        .map_ok(move |(path, nodes)| {
            let mut items = vec![];
            for (node, hg_cs_id, blobnode, flags) in nodes {
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
                    flags,
                };
                if flags.is_some() && cg_version == CgVersion::Cg2Version {
                    return stream::once(future::err(Error::msg(
                        "internal error: unexpected flags in cg2 generation",
                    )))
                    .left_stream();
                }
                items.push(Part::CgChunk(Section::Filelog(path.clone()), deltachunk));
            }

            items.push(Part::SectionEnd(Section::Filelog(path)));
            stream::iter(items).map(anyhow::Ok).right_stream()
        })
        .try_flatten()
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
    pub fullpath: MPath,
    pub linknode: HgNodeHash,
}

// Controls whether client should store trees in hg cache (which means
// they might be deleted and refetched from the server) or they should be stored
// in .hg directory (which means client should never delete them).
// Normally this should only be used for hydrated commit cloud commits, and
// with hg server deprecation it won't be necessary anymore.
#[derive(Clone, Copy)]
pub enum StoreInHgCache {
    Yes,
    No,
}

pub fn treepack_part<S>(entries: S, hg_cache_policy: StoreInHgCache) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = Result<BoxFuture<'static, Result<TreepackPartInput, Error>>, Error>>
        + Send
        + 'static,
{
    treepack_part_impl(entries, PartHeaderType::B2xTreegroup2, hg_cache_policy)
}

pub fn pushrebase_treepack_part<S>(entries: S) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = Result<BoxFuture<'static, Result<TreepackPartInput, Error>>, Error>>
        + Send
        + 'static,
{
    treepack_part_impl(entries, PartHeaderType::B2xRebasePack, StoreInHgCache::Yes)
}

fn treepack_part_impl<S>(
    entries: S,
    header_type: PartHeaderType,
    hg_cache_policy: StoreInHgCache,
) -> Result<PartEncodeBuilder>
where
    S: Stream<Item = Result<BoxFuture<'static, Result<TreepackPartInput, Error>>, Error>>
        + Send
        + 'static,
{
    let mut builder = PartEncodeBuilder::mandatory(header_type)?;
    builder.add_mparam("version", "1")?;
    match hg_cache_policy {
        StoreInHgCache::Yes => {
            builder.add_mparam("cache", "True")?;
        }
        StoreInHgCache::No => {
            builder.add_mparam("cache", "False")?;
        }
    };

    builder.add_mparam("category", "manifests")?;

    let buffer_size =
        justknobs::get_as::<usize>("scm/mononoke:repo_client_gettreepack_buffer_size", None)
            .unwrap_or(1000);

    let wirepack_parts = entries
        .try_buffered(buffer_size)
        .map_ok(|input| {
            let path = match input.fullpath.into_optional_non_root_path() {
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

            stream::iter(vec![history_meta, history, data_meta, data]).map(anyhow::Ok)
        })
        .try_flatten()
        .chain(stream::once(future::ok(wirepack::Part::End)));

    builder.set_data_generated(pack_wirepack(wirepack_parts, wirepack::Kind::Tree));

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
        match *self {
            ChangegroupApplyResult::Success { heads_num_diff } => {
                if heads_num_diff >= 0 {
                    write!(f, "{}", 1 + heads_num_diff)
                } else {
                    write!(f, "{}", -1 + heads_num_diff)
                }
            }
            ChangegroupApplyResult::Error => write!(f, "0"),
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
    S: Stream<Item = Result<(HgChangesetId, Vec<HgChangesetId>), Error>> + Send + 'static,
{
    let stream = obsmarkers_packer_stream(pairs, time, metadata);

    let mut builder = PartEncodeBuilder::mandatory(PartHeaderType::Obsmarkers)?;
    builder.set_data_generated(stream);
    Ok(builder)
}

pub fn infinitepush_mutation_part<F>(entries: F) -> Result<PartEncodeBuilder>
where
    F: Future<Output = Result<Vec<HgMutationEntry>, Error>> + Send + 'static,
{
    let mut builder = PartEncodeBuilder::advisory(PartHeaderType::B2xInfinitepushMutation)?;
    let data = entries.and_then(|entries| future::ready(infinitepush_mutation_packer(entries)));
    builder.set_data_future(data);
    Ok(builder)
}

pub fn pushvars_part(push_vars: HashMap<String, Bytes>) -> Result<PartEncodeBuilder> {
    let mut builder = PartEncodeBuilder::advisory(PartHeaderType::Pushvars)?;
    for (var, bytes) in push_vars {
        builder.add_aparam(var, bytes)?;
    }
    Ok(builder)
}
