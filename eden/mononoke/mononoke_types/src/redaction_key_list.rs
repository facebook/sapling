/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::thrift;
use crate::{
    blob::{Blob, BlobstoreValue, RedactionKeyListBlob},
    errors::ErrorKind,
    typed_hash::{RedactionKeyListId, RedactionKeyListIdContext},
};
use anyhow::{Context, Result};
use fbthrift::compact_protocol;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RedactionKeyList {
    pub keys: Vec<String>,
}

impl RedactionKeyList {
    fn into_thrift(self) -> thrift::RedactionKeyList {
        thrift::RedactionKeyList { keys: self.keys }
    }

    fn from_thrift(t: thrift::RedactionKeyList) -> Result<Self> {
        Ok(Self { keys: t.keys })
    }
}

impl BlobstoreValue for RedactionKeyList {
    type Key = RedactionKeyListId;

    fn into_blob(self) -> RedactionKeyListBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = RedactionKeyListIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data.into())
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .with_context(|| ErrorKind::BlobDeserializeError("RedactionKeyList".into()))?;
        Self::from_thrift(thrift_tc)
    }
}
