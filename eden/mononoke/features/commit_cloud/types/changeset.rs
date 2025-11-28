/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::hash::GitSha1;
use mononoke_types::sha1_hash::Sha1;
use mysql_common::value::convert::ConvIr;
use mysql_common::value::convert::FromValue;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    mysql::OptTryFromRowField,
    Serialize,
    Deserialize,
    Eq,
    Hash
)]
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

impl Display for CloudChangesetId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl From<CloudChangesetId> for HgChangesetId {
    fn from(cs: CloudChangesetId) -> Self {
        HgChangesetId::new(HgNodeHash::new(cs.0))
    }
}

impl From<HgChangesetId> for CloudChangesetId {
    fn from(cs: HgChangesetId) -> Self {
        CloudChangesetId(*cs.into_nodehash().sha1())
    }
}

impl From<GitSha1> for CloudChangesetId {
    fn from(cs: GitSha1) -> Self {
        CloudChangesetId(Sha1::from_byte_array(cs.into_inner()))
    }
}

impl From<CloudChangesetId> for GitSha1 {
    fn from(cs: CloudChangesetId) -> Self {
        GitSha1::from_byte_array(cs.0.into_byte_array())
    }
}
