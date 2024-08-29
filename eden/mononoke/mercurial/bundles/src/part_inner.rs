/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Type definitions for inner streams.

use std::collections::HashMap;
use std::collections::HashSet;
use std::str;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Error;
use anyhow::Result;
use bytes::BytesMut;
use futures::future;
use futures::future::BoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_ext::stream::FbStreamExt;
use lazy_static::lazy_static;
use maplit::hashset;
use slog::o;
use slog::warn;
use slog::Logger;
use tokio::io::AsyncBufRead;
use tokio_util::codec::Decoder;

use crate::capabilities;
use crate::changegroup;
use crate::errors::ErrorKind;
use crate::infinitepush;
use crate::part_header::PartHeader;
use crate::part_header::PartHeaderType;
use crate::part_outer::OuterStream;
use crate::pushrebase;
use crate::utils::decode_stream;
use crate::wirepack;
use crate::Bundle2Item;

// --- Part parameters

lazy_static! {
    static ref KNOWN_PARAMS: HashMap<PartHeaderType, HashSet<&'static str>> = {
        let mut m: HashMap<PartHeaderType, HashSet<&'static str>> = HashMap::new();
        m.insert(
            PartHeaderType::Changegroup,
            hashset! {"version", "nbchanges", "treemanifest"},
        );
        m.insert(
            PartHeaderType::B2xInfinitepush,
            hashset! {
            "pushbackbookmarks", "cgversion", "bookmark", "bookprevnode", "create", "force"},
        );
        m.insert(PartHeaderType::B2xInfinitepushBookmarks, hashset! {});
        m.insert(PartHeaderType::B2xInfinitepushMutation, hashset! {});
        m.insert(PartHeaderType::B2xCommonHeads, hashset! {});
        m.insert(
            PartHeaderType::B2xRebase,
            hashset! {"onto", "newhead", "cgversion", "obsmarkerversions"},
        );
        m.insert(
            PartHeaderType::B2xRebasePack,
            hashset! {"version", "cache", "category"},
        );
        m.insert(
            PartHeaderType::B2xTreegroup2,
            hashset! {"version", "cache", "category"},
        );
        m.insert(PartHeaderType::Replycaps, hashset! {});
        m.insert(
            PartHeaderType::Pushkey,
            hashset! { "namespace", "key", "old", "new" },
        );
        m.insert(PartHeaderType::Pushvars, hashset! {});
        m
    };
}

pub fn validate_header(header: PartHeader) -> Result<Option<PartHeader>> {
    match KNOWN_PARAMS.get(header.part_type()) {
        Some(known_params) => {
            // Make sure all the mandatory params are recognized.
            let unknown_params: Vec<_> = header
                .mparams()
                .keys()
                .filter(|param| !known_params.contains(param.as_str()))
                .cloned()
                .collect();
            if !unknown_params.is_empty() {
                bail!(ErrorKind::BundleUnknownPartParams(
                    *header.part_type(),
                    unknown_params,
                ));
            }
            Ok(Some(header))
        }
        None => {
            if header.mandatory() {
                bail!(ErrorKind::BundleUnknownPart(header));
            }
            Ok(None)
        }
    }
}

pub fn get_cg_version(header: PartHeader, field: &str) -> Result<changegroup::unpacker::CgVersion> {
    let version = header
        .mparams()
        .get(field)
        .or_else(|| header.aparams().get(field));
    let err = ErrorKind::CgDecode(format!(
        "No changegroup version in Part Header in field {}",
        field
    ))
    .into();

    version
        .ok_or(err)
        .and_then(|version_bytes| {
            str::from_utf8(version_bytes)
                .map_err(|e| ErrorKind::CgDecode(format!("{:?}", e)).into())
        })
        .and_then(|version_str| {
            version_str
                .parse::<changegroup::unpacker::CgVersion>()
                .map_err(|e| ErrorKind::CgDecode(format!("{:?}", e)).into())
        })
}

pub fn get_cg_unpacker(
    logger: Logger,
    header: PartHeader,
    field: &str,
) -> changegroup::unpacker::CgUnpacker {
    // TODO(anastasiyaz): T34812941 return Result here, no default packer (version should be specified)
    let _logger = logger.clone();
    match get_cg_version(header, field) {
        Ok(version) => changegroup::unpacker::CgUnpacker::new(logger, version),
        Err(e) => {
            // ChangeGroup2 by default
            warn!(_logger.clone(), "{:?}", e);
            let default_version = changegroup::unpacker::CgVersion::Cg2Version;
            changegroup::unpacker::CgUnpacker::new(_logger, default_version)
        }
    }
}

/// Convert an OuterStream into an InnerStream using the part header.
pub(crate) fn inner_stream<R: AsyncBufRead + Send + 'static>(
    logger: Logger,
    header: PartHeader,
    stream: OuterStream<R>,
) -> (
    Bundle2Item<'static>,
    BoxFuture<'static, Result<OuterStream<R>, Error>>,
) {
    let wrapped_stream = stream
        .try_take_while(|frame| {
            futures::future::ok(frame.as_ref().map_or(false, |frame| frame.is_payload()))
        })
        .map_ok(|frame| frame.unwrap().get_payload());
    let (wrapped_stream, remainder) = wrapped_stream.return_remainder();

    let bundle2item = match *header.part_type() {
        PartHeaderType::Changegroup => {
            let cg2_stream = decode_stream(
                wrapped_stream,
                get_cg_unpacker(
                    logger.new(o!("stream" => "changegroup")),
                    header.clone(),
                    "version",
                ),
            );
            Bundle2Item::Changegroup(header, cg2_stream.boxed())
        }
        PartHeaderType::B2xCommonHeads => {
            let heads_stream =
                decode_stream(wrapped_stream, pushrebase::CommonHeadsUnpacker::new());
            Bundle2Item::B2xCommonHeads(header, heads_stream.boxed())
        }
        PartHeaderType::B2xInfinitepush => {
            let cg2_stream = decode_stream(
                wrapped_stream,
                get_cg_unpacker(
                    logger.new(o!("stream" => "b2xinfinitepush")),
                    header.clone(),
                    "cgversion",
                ),
            );
            Bundle2Item::B2xInfinitepush(header, cg2_stream.boxed())
        }
        PartHeaderType::B2xInfinitepushBookmarks => {
            let bookmarks_stream = decode_stream(
                wrapped_stream,
                infinitepush::InfinitepushBookmarksUnpacker::new(),
            );
            Bundle2Item::B2xInfinitepushBookmarks(header, bookmarks_stream.boxed())
        }
        PartHeaderType::B2xInfinitepushMutation => {
            let mutation_stream = decode_stream(
                wrapped_stream,
                infinitepush::InfinitepushMutationUnpacker::new(),
            );
            Bundle2Item::B2xInfinitepushMutation(header, mutation_stream.boxed())
        }
        PartHeaderType::B2xTreegroup2 => {
            let wirepack_stream = decode_stream(
                wrapped_stream,
                wirepack::unpacker::new(
                    logger.new(o!("stream" => "wirepack")),
                    // Mercurial only knows how to send trees at the moment.
                    // TODO: add support for file wirepacks once that's a thing
                    wirepack::Kind::Tree,
                ),
            );
            Bundle2Item::B2xTreegroup2(header, wirepack_stream.boxed())
        }
        PartHeaderType::Replycaps => {
            let caps = decode_stream(wrapped_stream, capabilities::CapabilitiesUnpacker)
                .try_collect::<Vec<_>>()
                .and_then(|caps| async move {
                    ensure!(caps.len() == 1, "Unexpected Replycaps payload: {:?}", caps);
                    Ok(caps.into_iter().next().unwrap())
                });
            Bundle2Item::Replycaps(header, caps.boxed())
        }
        PartHeaderType::B2xRebasePack => {
            let wirepack_stream = decode_stream(
                wrapped_stream,
                wirepack::unpacker::new(
                    logger.new(o!("stream" => "wirepack")),
                    // Mercurial only knows how to send trees at the moment.
                    // TODO: add support for file wirepacks once that's a thing
                    wirepack::Kind::Tree,
                ),
            );
            Bundle2Item::B2xRebasePack(header, wirepack_stream.boxed())
        }
        PartHeaderType::B2xRebase => {
            let cg2_stream = decode_stream(
                wrapped_stream,
                get_cg_unpacker(
                    logger.new(o!("stream" => "bx2rebase")),
                    header.clone(),
                    "cgversion",
                ),
            );
            Bundle2Item::B2xRebase(header, cg2_stream.boxed())
        }
        PartHeaderType::Pushkey => {
            // Pushkey part has an empty part payload, but we still need to "parse" it
            // Otherwise polling remainder stream may fail.
            let empty =
                decode_stream(wrapped_stream, EmptyUnpacker).try_for_each(|_| future::ok(()));
            Bundle2Item::Pushkey(header, empty.boxed())
        }
        PartHeaderType::Pushvars => {
            // Pushvars part has an empty part payload, but we still need to "parse" it
            // Otherwise polling remainder stream may fail.
            let empty =
                decode_stream(wrapped_stream, EmptyUnpacker).try_for_each(|_| future::ok(()));
            Bundle2Item::Pushvars(header, empty.boxed())
        }
        _ => panic!("TODO: make this an error"),
    };

    (
        bundle2item,
        remainder
            .map_ok(|s| s.into_inner().into_inner())
            .map_err(Error::from)
            .boxed(),
    )
}

// Decoder for an empty part (for example, pushkey)
pub struct EmptyUnpacker;

impl Decoder for EmptyUnpacker {
    type Item = ();
    type Error = Error;

    fn decode(&mut self, _buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        Ok(None)
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use crate::changegroup::unpacker::CgVersion;
    use crate::part_header::PartHeaderBuilder;
    use crate::part_header::PartHeaderType;
    use crate::part_inner::*;

    #[mononoke::test]
    fn test_cg_unpacker_v3() {
        let mut header_builder =
            PartHeaderBuilder::new(PartHeaderType::Changegroup, false).unwrap();
        header_builder.add_aparam("version", "03").unwrap();
        let header = header_builder.build(1);

        assert_eq!(
            get_cg_version(header, "version").unwrap(),
            CgVersion::Cg3Version
        );
    }

    #[mononoke::test]
    fn test_cg_unpacker_v2() {
        let mut header_builder =
            PartHeaderBuilder::new(PartHeaderType::Changegroup, false).unwrap();
        header_builder.add_aparam("version", "02").unwrap();
        let header = header_builder.build(1);

        assert_eq!(
            get_cg_version(header, "version").unwrap(),
            CgVersion::Cg2Version
        );
    }

    #[mononoke::test]
    fn test_cg_unpacker_default_v2() {
        let header_builder = PartHeaderBuilder::new(PartHeaderType::Changegroup, false).unwrap();
        let h = header_builder.build(1);

        assert!(get_cg_version(h, "version").is_err());
    }
}
