/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use fbthrift::compact_protocol;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::RedactionKeyListBlob;
use crate::thrift;
use crate::typed_hash::RedactionKeyListId;
use crate::typed_hash::RedactionKeyListIdContext;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RedactionKeyList {
    pub keys: Vec<String>,
}

impl RedactionKeyList {
    fn into_thrift(self) -> thrift::redaction::RedactionKeyList {
        thrift::redaction::RedactionKeyList { keys: self.keys }
    }

    fn from_thrift(t: thrift::redaction::RedactionKeyList) -> Result<Self> {
        Ok(Self { keys: t.keys })
    }

    pub fn from_bytes(serialized: &[u8]) -> Result<Self> {
        Self::from_thrift(
            compact_protocol::deserialize(serialized)
                .with_context(|| anyhow!("While deserializing RedactionKeyList"))?,
        )
    }
}

impl BlobstoreValue for RedactionKeyList {
    type Key = RedactionKeyListId;

    fn into_blob(self) -> RedactionKeyListBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(thrift);
        let mut context = RedactionKeyListIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}
