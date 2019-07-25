// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Unpacking capabilities

use std::borrow::Cow;
use std::collections::HashMap;

use bytes::BytesMut;
use percent_encoding::percent_decode;
use tokio_io::codec::Decoder;

use crate::errors::*;

#[derive(Debug, PartialEq, Eq)]
pub struct Capabilities {
    caps: HashMap<String, Vec<String>>,
}

/// This is a tokio_io Decoder for capabilities used f.e. in "replycaps" part of bundle2
///
/// The format is as follows:
/// <capabilities> := '' EOF | <line> ['\n' <line>]* EOF
/// <line> := <key> | <key> '=' <values>
/// <values> := '' | <value> [',' <value>]*
/// <key> := `url encoded key`
/// <value> := `url encoded value`
///
/// Notice that this decoder needs to reach EOF and it decodes everything as a single item
pub struct CapabilitiesUnpacker;

impl Decoder for CapabilitiesUnpacker {
    type Item = Capabilities;
    type Error = Error;

    fn decode(&mut self, _buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        Ok(None) // This unpacker unpacks single element, wait for EOF
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        let mut caps = HashMap::new();
        for kv in buf.split(|b| b == &b'\n') {
            let mut kv = kv.splitn(2, |b| b == &b'=');
            let key = percent_decode(kv.next().expect("must have at least 1 element"))
                .decode_utf8()?
                .into_owned();
            let values = {
                match kv.next() {
                    None => Vec::new(),
                    Some(values) => {
                        let res: ::std::result::Result<Vec<_>, _> = values
                            .split(|b| b == &b',')
                            .filter(|v| !v.is_empty())
                            .map(|v| percent_decode(v).decode_utf8().map(Cow::into_owned))
                            .collect();
                        res?
                    }
                }
            };
            caps.insert(key, values);
        }

        buf.clear(); // all buf was consumed

        Ok(Some(Capabilities { caps }))
    }
}
