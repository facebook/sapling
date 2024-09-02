/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bookmarks::BookmarkName;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql::mysql;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
pub struct RowId(pub u64);

impl From<RowId> for Value {
    fn from(id: RowId) -> Self {
        Value::UInt(id.0)
    }
}

impl std::fmt::Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ConvIr<RowId> for RowId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(RowId(from_value_opt(v)?))
    }
    fn commit(self) -> Self {
        self
    }
    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for RowId {
    type Intermediate = RowId;
}

macro_rules! mysql_string_newtype {
    ($ty: ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
        pub struct $ty(pub String);

        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<$ty> for Value {
            fn from(val: $ty) -> Self {
                Value::Bytes(val.0.into_bytes())
            }
        }

        impl ConvIr<$ty> for $ty {
            fn new(v: Value) -> Result<Self, FromValueError> {
                match v {
                    Value::Bytes(bytes) => match String::from_utf8(bytes) {
                        Ok(s) => Ok($ty(s)),
                        Err(from_utf8_error) => {
                            Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                        }
                    },
                    v => Err(FromValueError(v)),
                }
            }

            fn commit(self) -> $ty {
                self
            }

            fn rollback(self) -> Value {
                self.into()
            }
        }

        impl FromValue for $ty {
            type Intermediate = $ty;
        }
    };
}

mysql_string_newtype!(BlobstoreKey);
mysql_string_newtype!(RequestType);
mysql_string_newtype!(ClaimedBy);

#[derive(Clone, Copy, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
pub enum RequestStatus {
    New,
    InProgress,
    Ready,
    Polled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongRunningRequestEntry {
    pub id: RowId,
    pub repo_id: RepositoryId,
    pub bookmark: BookmarkName,
    pub request_type: RequestType,
    pub args_blobstore_key: BlobstoreKey,
    pub result_blobstore_key: Option<BlobstoreKey>,
    pub created_at: Timestamp,
    pub started_processing_at: Option<Timestamp>,
    pub inprogress_last_updated_at: Option<Timestamp>,
    pub ready_at: Option<Timestamp>,
    pub polled_at: Option<Timestamp>,
    pub status: RequestStatus,
    pub claimed_by: Option<ClaimedBy>,
}

impl std::fmt::Display for RequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use RequestStatus::*;

        let s = match self {
            New => "new",
            InProgress => "inprogress",
            Ready => "ready",
            Polled => "polled",
        };
        write!(f, "{}", s)
    }
}

impl ConvIr<RequestStatus> for RequestStatus {
    fn new(v: Value) -> Result<Self, FromValueError> {
        use RequestStatus::*;

        match v {
            Value::Bytes(ref b) if b == b"new" => Ok(New),
            Value::Bytes(ref b) if b == b"inprogress" => Ok(InProgress),
            Value::Bytes(ref b) if b == b"ready" => Ok(Ready),
            Value::Bytes(ref b) if b == b"polled" => Ok(Polled),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> RequestStatus {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for RequestStatus {
    type Intermediate = RequestStatus;
}

impl From<RequestStatus> for Value {
    fn from(status: RequestStatus) -> Self {
        use RequestStatus::*;

        match status {
            New => Value::Bytes(b"new".to_vec()),
            InProgress => Value::Bytes(b"inprogress".to_vec()),
            Ready => Value::Bytes(b"ready".to_vec()),
            Polled => Value::Bytes(b"polled".to_vec()),
        }
    }
}

/// A full identified for a request
/// Note: while RowId is guaranteed to be unique in the table,
///       it is generally illegal to make queries without knowing
///       which request type you are talking about
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestId(pub RowId, pub RequestType);
