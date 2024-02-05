/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod bundle2;
pub mod bundle2_encode;
pub mod capabilities;
pub mod changegroup;
mod chunk;
mod delta;
pub mod infinitepush;
pub mod obsmarkers;
pub mod part_encode;
mod part_header;
mod part_inner;
mod part_outer;
pub mod parts;
mod pushrebase;
mod quickcheck_types;
pub mod stream_start;
#[cfg(test)]
mod test;
mod types;
pub mod wirepack;

mod errors;
pub use crate::errors::ErrorKind;
mod utils;

use std::fmt;

use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use bytes_old::Bytes as BytesOld;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::sink::SinkExt;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::FutureExt;
use tokio_util::io::CopyToBytes;
use tokio_util::io::SinkWriter;

pub use crate::bundle2_encode::Bundle2EncodeBuilder;
pub use crate::part_header::PartHeader;
pub use crate::part_header::PartHeaderInner;
pub use crate::part_header::PartHeaderType;
pub use crate::part_header::PartId;
pub use crate::types::StreamHeader;

pub enum Bundle2Item<'a> {
    Start(StreamHeader),
    Changegroup(PartHeader, BoxStream<'a, Result<changegroup::Part>>),
    B2xCommonHeads(
        PartHeader,
        BoxStream<'a, Result<mercurial_types::HgChangesetId>>,
    ),
    B2xInfinitepush(PartHeader, BoxStream<'a, Result<changegroup::Part>>),
    B2xTreegroup2(PartHeader, BoxStream<'a, Result<wirepack::Part>>),
    // B2xInfinitepushBookmarks returns BytesOld because this part is not going to be used.
    B2xInfinitepushBookmarks(PartHeader, BoxStream<'a, Result<BytesOld>>),
    B2xInfinitepushMutation(
        PartHeader,
        BoxStream<'a, Result<Vec<mercurial_mutation::HgMutationEntry>>>,
    ),
    B2xRebasePack(PartHeader, BoxStream<'a, Result<wirepack::Part>>),
    B2xRebase(PartHeader, BoxStream<'a, Result<changegroup::Part>>),
    Replycaps(
        PartHeader,
        BoxFuture<'a, Result<capabilities::Capabilities>>,
    ),
    Pushkey(PartHeader, BoxFuture<'a, Result<()>>),
    Pushvars(PartHeader, BoxFuture<'a, Result<()>>),
}

impl<'a> Bundle2Item<'a> {
    #[cfg(test)]
    pub(crate) fn unwrap_start(self) -> StreamHeader {
        match self {
            Bundle2Item::Start(stream_header) => stream_header,
            other => panic!("expected item to be Start, was {:?}", other),
        }
    }
}

impl<'a> fmt::Debug for Bundle2Item<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::Bundle2Item::*;
        match *self {
            Start(ref header) => write!(f, "Bundle2Item::Start({:?})", header),
            Changegroup(ref header, _) => write!(f, "Bundle2Item::Changegroup({:?}, ...)", header),
            B2xCommonHeads(ref header, _) => {
                write!(f, "Bundle2Item::B2xCommonHeads({:?}, ...)", header)
            }
            B2xInfinitepush(ref header, _) => {
                write!(f, "Bundle2Item::B2xInfinitepush({:?}, ...)", header)
            }
            B2xInfinitepushBookmarks(ref header, _) => write!(
                f,
                "Bundle2Item::B2xInfinitepushBookmarks({:?}, ...)",
                header
            ),
            B2xInfinitepushMutation(ref header, _) => {
                write!(f, "Bundle2Item::B2xInfinitepushMutation({:?}, ...)", header)
            }
            B2xTreegroup2(ref header, _) => {
                write!(f, "Bundle2Item::B2xTreegroup2({:?}, ...)", header)
            }
            B2xRebasePack(ref header, _) => {
                write!(f, "Bundle2Item::B2xRebasePack({:?}, ...)", header)
            }
            B2xRebase(ref header, _) => write!(f, "Bundle2Item::B2xRebase({:?}, ...)", header),
            Replycaps(ref header, _) => write!(f, "Bundle2Item::Replycaps({:?}, ...)", header),
            Pushkey(ref header, _) => write!(f, "Bundle2Item::Pushkey({:?}, ...)", header),
            Pushvars(ref header, _) => write!(f, "Bundle2Item::Pushvars({:?}, ...)", header),
        }
    }
}

/// Given bundle parts, returns a stream of Bytes that represent an encoded bundle with these parts
pub fn create_bundle_stream_new(
    parts: Vec<part_encode::PartEncodeBuilder>,
) -> impl Stream<Item = Result<Bytes, Error>> {
    let (sender, receiver) = mpsc::channel::<Bytes>(1);
    // Sends either and empty Bytes if bundle generation was successful or an error.
    // Empty Bytes are used just to make chaining of streams below easier.
    let (result_sender, result_receiver) = oneshot::channel::<Result<Bytes>>();
    let mut bundle = Bundle2EncodeBuilder::new(SinkWriter::new(
        CopyToBytes::new(sender)
            .sink_map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{e:?}"))),
    ));
    for part in parts {
        bundle.add_part(part);
    }

    tokio::spawn(async move {
        match bundle.build().await {
            Ok(_) => {
                // Bundle was successfully generated, so there is nothing add.
                // So just add empty bytes.
                let _ = result_sender.send(Ok(Bytes::new()));
            }
            Err(err) => {
                let _ = result_sender.send(Err(err));
            }
        }

        anyhow::Ok(())
    });

    receiver
        .map(Ok)
        .chain(result_receiver.map(|res| anyhow::Ok(res??)).into_stream())
}
