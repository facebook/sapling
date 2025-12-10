/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Context;
use gix_packetline::PacketLineRef;
use gix_packetline::StreamingPeekableIter;
use gix_transport::bstr::ByteSlice;
use protocol::types::LsRefsRequest;
use protocol::types::RefsSource;
use protocol::types::RequestedRefs;
use protocol::types::RequestedSymrefs;
use protocol::types::SymrefFormat;
use protocol::types::TagInclusion;

const SYMREFS: &[u8] = b"symrefs";
const UNBORN: &[u8] = b"unborn";
const PEEL: &[u8] = b"peel";
const REF_PREFIX: &[u8] = b"ref-prefix ";

/// Arguments for `ls-refs` command
#[derive(Clone, Debug, Default)]
pub struct LsRefsArgs {
    /// In addition to the object pointed by it, show the underlying ref
    /// pointed by it when showing a symbolic ref.
    symrefs: bool,
    /// Show peeled tags
    peel: bool,
    /// The server will send information about HEAD even if it is a symref
    /// pointing to an unborn branch
    unborn: bool,
    /// When specified, only references having a prefix matching one of the
    /// provided prefixes are displayed
    ref_prefixes: Vec<String>,
}

impl LsRefsArgs {
    pub fn parse_from_packetline(args: &[u8]) -> anyhow::Result<Self> {
        let mut tokens = StreamingPeekableIter::new(args, &[PacketLineRef::Flush], true);
        let mut ls_ref_args = Self::default();
        while let Some(token) = tokens.read_line() {
            let token = token.context(
                "Failed to read line from packetline during ls-refs command args parsing",
            )??;
            if let PacketLineRef::Data(data) = token {
                let data = data.trim();
                if let Some(ref_type) = data.strip_prefix(REF_PREFIX) {
                    let prefix = String::from_utf8(ref_type.to_vec()).with_context(|| {
                        format!(
                            "Invalid ref-prefix argument {:?} for ls-refs command",
                            ref_type
                        )
                    })?;
                    ls_ref_args.ref_prefixes.push(prefix);
                } else {
                    match data {
                        SYMREFS => ls_ref_args.symrefs = true,
                        UNBORN => ls_ref_args.unborn = true,
                        PEEL => ls_ref_args.peel = true,
                        _ => anyhow::bail!("Unknown argument {:?} for ls-refs command", data),
                    };
                }
            } else {
                anyhow::bail!(
                    "Unexpected token {:?} in packetline during ls-refs command args parsing",
                    token
                );
            }
        }
        Ok(ls_ref_args)
    }

    pub fn into_request(self, bypass_cache: bool) -> LsRefsRequest {
        let requested_symrefs = if self.symrefs {
            RequestedSymrefs::IncludeAll(SymrefFormat::NameWithTarget)
        } else {
            RequestedSymrefs::IncludeAll(SymrefFormat::NameOnly)
        };
        let tag_inclusion = if self.peel {
            TagInclusion::WithTarget
        } else {
            TagInclusion::AsIs
        };
        let requested_refs = if !self.ref_prefixes.is_empty() {
            RequestedRefs::IncludedWithPrefix(HashSet::from_iter(self.ref_prefixes))
        } else {
            RequestedRefs::all()
        };
        LsRefsRequest {
            requested_symrefs,
            tag_inclusion,
            requested_refs,
            // Use WBC since this request is for read path, unless explicily asked not to
            refs_source: match bypass_cache {
                false => RefsSource::WarmBookmarksCache,
                true => RefsSource::DatabaseFollower,
            },
        }
    }
}
