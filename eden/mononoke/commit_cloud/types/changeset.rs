/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use mononoke_types::sha1_hash::Sha1;
use mysql_common::value::convert::ConvIr;
use mysql_common::value::convert::FromValue;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

#[derive(Clone, Debug, PartialEq, mysql::OptTryFromRowField)]
pub struct CloudChangesetId(pub Sha1);

impl From<CloudChangesetId> for Value {
    fn from(cs: CloudChangesetId) -> Self {
        Value::Bytes(cs.0.to_hex().as_bytes().to_vec())
    }
}

impl ConvIr<CloudChangesetId> for CloudChangesetId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => match std::str::from_utf8(&bytes) {
                Ok(s) => Sha1::from_str(s)
                    .map(CloudChangesetId)
                    .map_err(|_| FromValueError(Value::Bytes(bytes))),
                Err(_) => Err(FromValueError(Value::Bytes(bytes))),
            },
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> CloudChangesetId {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for CloudChangesetId {
    type Intermediate = CloudChangesetId;
}
