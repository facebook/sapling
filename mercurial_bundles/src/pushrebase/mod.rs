// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Pushrebase codecs

use bytes::BytesMut;
use mercurial_types::{HgChangesetId, HgNodeHash};
use tokio_codec::Decoder;

use crate::errors::*;

#[derive(Debug)]
pub struct CommonHeadsUnpacker {}

impl CommonHeadsUnpacker {
    pub fn new() -> Self {
        Self {}
    }
}

impl Decoder for CommonHeadsUnpacker {
    type Item = HgChangesetId;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        if buf.len() >= 20 {
            let newcsid = buf.split_to(20).freeze();
            let nodehash = HgNodeHash::from_bytes(&newcsid)?;
            Ok(Some(HgChangesetId::new(nodehash)))
        } else {
            Ok(None)
        }
    }
}
