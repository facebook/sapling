// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Type definitions for inner streams.
#![deny(warnings)]

use std::collections::{HashMap, HashSet};
use std::str;

use futures::{future, Stream};
use slog;

use ascii::AsciiStr;
use bytes::Bytes;
use futures::stream::Map;
use tokio_io::AsyncRead;

use changegroup;
use errors::*;
use futures_ext::{BoxStreamWrapper, StreamExt, StreamLayeredExt, TakeWhile};
use part_header::PartHeader;
use part_outer::{OuterFrame, OuterStream};

// --- Part parameters

macro_rules! add_part {
    ( $m:expr, $part_type:expr, [$( $params:expr ),*] ) => {{
        let mut h = HashSet::new();
        $(h.insert($params);)*
        $m.insert(AsciiStr::from_ascii($part_type).unwrap(), h);
    }}
}

lazy_static! {
    static ref KNOWN_PARAMS: HashMap<&'static AsciiStr, HashSet<&'static str>> = {
        let mut m: HashMap<&'static AsciiStr, HashSet<&'static str>> = HashMap::new();
        add_part!(m, "changegroup", ["version", "nbchanges", "treemanifest"]);
        m
    };
}

type BoolFuture = future::FutureResult<bool, Error>;

type WrappedStream<T> = Map<
    TakeWhile<OuterStream<T>, fn(&OuterFrame) -> BoolFuture, BoolFuture>,
    fn(OuterFrame) -> Bytes,
>;

pub trait InnerStream<T>
    : Stream<Item = InnerPart, Error = Error> + BoxStreamWrapper<WrappedStream<T>>
where
    T: AsyncRead + 'static,
{
}

impl<T, U> InnerStream<T> for U
where
    U: Stream<Item = InnerPart, Error = Error> + BoxStreamWrapper<WrappedStream<T>>,
    T: AsyncRead + 'static,
{
}

pub type BoxInnerStream<T> = Box<InnerStream<T, Item = InnerPart, Error = Error>>;

#[derive(Debug, Eq, PartialEq)]
pub enum InnerPart {
    Cg2(changegroup::Part),
}

impl InnerPart {
    pub fn is_cg2(&self) -> bool {
        match self {
            &InnerPart::Cg2(_) => true,
        }
    }

    pub fn cg2_part(self) -> changegroup::Part {
        match self {
            InnerPart::Cg2(part) => part,
        }
    }
}

pub fn validate_header(header: PartHeader) -> Result<Option<PartHeader>> {
    match KNOWN_PARAMS.get(header.part_type_lower()) {
        Some(ref known_params) => {
            // Make sure all the mandatory params are recognized.
            let unknown_params: Vec<_> = header
                .mparams()
                .keys()
                .filter(|param| !known_params.contains(param.as_str()))
                .map(|param| param.clone())
                .collect();
            if !unknown_params.is_empty() {
                bail!(ErrorKind::BundleUnknownPartParams(
                    header.part_type().to_ascii_string(),
                    unknown_params
                ));
            }
            Ok(Some(header))
        }
        None => {
            if header.is_mandatory() {
                bail!(ErrorKind::BundleUnknownPart(header));
            }
            Ok(None)
        }
    }
}

/// Convert an OuterStream into an InnerStream using the part header.
pub fn inner_stream<R: AsyncRead>(
    header: &PartHeader,
    stream: OuterStream<R>,
    logger: &slog::Logger,
) -> BoxInnerStream<R> {
    // The casts are required for Rust to not complain about "expected fn
    // pointer, found fn item". See http://stackoverflow.com/q/34787928.
    let wrapped_stream: WrappedStream<R> = stream
        .take_while_wrapper(is_payload_fut as fn(&OuterFrame) -> BoolFuture)
        .map(OuterFrame::get_payload as fn(OuterFrame) -> Bytes);
    match header.part_type_lower().as_str() {
        "changegroup" => {
            let cg2_stream = wrapped_stream.decode(changegroup::unpacker::Cg2Unpacker::new(
                logger.new(o!("stream" => "cg2")),
            ));
            Box::new(cg2_stream)
        }
        _ => panic!("TODO: make this an error"),
    }
}

fn is_payload_fut(item: &OuterFrame) -> BoolFuture {
    future::ok(item.is_payload())
}
