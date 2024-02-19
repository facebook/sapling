/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use gix_hash::ObjectId;
use gix_packetline::PacketLineRef;
use gix_packetline::StreamingPeekableIter;
use gix_transport::bstr::ByteSlice;
use protocol::types::DeltaInclusion;
use protocol::types::PackItemStreamRequest;
use protocol::types::PackfileItemInclusion;
use protocol::types::TagInclusion;

const WANT_PREFIX: &[u8] = b"want ";
const HAVE_PREFIX: &[u8] = b"have ";
const DONE: &[u8] = b"done";
const THIN_PACK: &[u8] = b"thin-pack";
const NO_PROGRESS: &[u8] = b"no-progress";
const INCLUDE_TAG: &[u8] = b"include-tag";
const OFSET_DELTA: &[u8] = b"ofs-delta";

/// Arguments for `fetch` command
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct FetchArgs {
    /// Indicates to the server the objects which the client wants to
    /// retrieve
    wants: Vec<ObjectId>,
    /// Indicates to the server the objects which the client already has
    /// locally
    haves: Vec<ObjectId>,
    /// Indicates to the server that negotiation should terminate (or
    /// not even begin if performing a clone) and that the server should
    /// use the information supplied in the request to construct the packfile
    done: bool,
    /// Request that a thin pack be sent, which is a pack with deltas
    /// which reference base objects not contained within the pack (but
    /// are known to exist at the receiving end)
    thin_pack: bool,
    /// Request that progress information that would normally be sent on
    /// side-band channel 2, during the packfile transfer, should not be sent
    no_progress: bool,
    /// Request that annotated tags should be sent if the objects they
    /// point to are being sent.
    include_tag: bool,
    /// Indicate that the client understands PACKv2 with delta referring
    /// to its base by position in pack rather than by an oid
    ofs_delta: bool,
    // NOTE: More possible arguments exist which will be added later
}

impl FetchArgs {
    pub fn parse_from_packetline(args: &[u8]) -> anyhow::Result<Self> {
        let mut tokens = StreamingPeekableIter::new(args, &[PacketLineRef::Flush], true);
        let mut fetch_args = Self::default();
        while let Some(token) = tokens.read_line() {
            let token = token.context(
                "Failed to read line from packetline during fetch command args parsing",
            )??;
            if let PacketLineRef::Data(data) = token {
                let data = data.trim();
                if let Some(oid) = data.strip_prefix(WANT_PREFIX) {
                    let object_id = ObjectId::from_hex(oid).with_context(|| {
                        format!("Invalid object id {:?} received during fetch request", oid)
                    })?;
                    fetch_args.wants.push(object_id);
                } else if let Some(oid) = data.strip_prefix(HAVE_PREFIX) {
                    let object_id = ObjectId::from_hex(oid).with_context(|| {
                        format!("Invalid object id {:?} received during fetch request", oid)
                    })?;
                    fetch_args.haves.push(object_id);
                } else {
                    match data {
                        DONE => fetch_args.done = true,
                        THIN_PACK => fetch_args.thin_pack = true,
                        NO_PROGRESS => fetch_args.no_progress = true,
                        INCLUDE_TAG => fetch_args.include_tag = true,
                        OFSET_DELTA => fetch_args.ofs_delta = true,
                        _ => continue,
                        // Ideally we want to bail here, but since not all fetch arguments are covered, we just need to ignore the
                        // extra for now
                    };
                }
            } else {
                anyhow::bail!(
                    "Unexpected token {:?} in packetline during fetch command args parsing",
                    token
                );
            };
        }
        Ok(fetch_args)
    }

    //NOTE: This request is only for full clone and will not work for incremental pulls
    pub fn into_request(self) -> PackItemStreamRequest {
        PackItemStreamRequest::full_repo(
            DeltaInclusion::standard(),
            TagInclusion::AsIs,
            PackfileItemInclusion::FetchAndStore,
        )
    }
}
