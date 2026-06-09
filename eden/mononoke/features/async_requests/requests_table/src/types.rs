/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::FromValue;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    mysql::OptTryFromRowField
)]
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

impl TryFrom<Value> for RowId {
    type Error = FromValueError;
    fn try_from(v: Value) -> Result<Self, FromValueError> {
        Ok(RowId(from_value_opt(v)?))
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

        impl TryFrom<Value> for $ty {
            type Error = FromValueError;
            fn try_from(v: Value) -> Result<Self, FromValueError> {
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
        }

        impl FromValue for $ty {
            type Intermediate = $ty;
        }
    };
}

mysql_string_newtype!(BlobstoreKey);
mysql_string_newtype!(RequestType);
mysql_string_newtype!(ClaimedBy);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, mysql::OptTryFromRowField)]
pub enum RequestStatus {
    New,
    InProgress,
    Ready,
    Polled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LongRunningRequestEntry {
    pub id: RowId,
    pub repo_id: Option<RepositoryId>,
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
    pub num_retries: Option<u8>,
    pub failed_at: Option<Timestamp>,
    pub root_request_id: Option<RowId>,
    pub created_by: Option<String>,
}

impl std::fmt::Display for RequestStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use RequestStatus::*;

        let s = match self {
            New => "new",
            InProgress => "inprogress",
            Ready => "ready",
            Polled => "polled",
            Failed => "failed",
        };
        write!(f, "{s}")
    }
}

impl TryFrom<Value> for RequestStatus {
    type Error = FromValueError;
    fn try_from(v: Value) -> Result<Self, FromValueError> {
        use RequestStatus::*;

        match v {
            Value::Bytes(ref b) if b == b"new" => Ok(New),
            Value::Bytes(ref b) if b == b"inprogress" => Ok(InProgress),
            Value::Bytes(ref b) if b == b"ready" => Ok(Ready),
            Value::Bytes(ref b) if b == b"polled" => Ok(Polled),
            Value::Bytes(ref b) if b == b"failed" => Ok(Failed),
            v => Err(FromValueError(v)),
        }
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
            Failed => Value::Bytes(b"failed".to_vec()),
        }
    }
}

/// A full identified for a request
/// Note: while RowId is guaranteed to be unique in the table,
///       it is generally illegal to make queries without knowing
///       which request type you are talking about
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestId(pub RowId, pub RequestType);

/// A row returned by `list_recent_backfills_with_repo_count`.
///
/// Includes both the root request's own status and aggregated per-status
/// counts of its children, so callers can render a user-facing aggregate
/// status without a follow-up query (the root reaches `Ready` once it
/// finishes spawning children, even though the children may still be
/// running).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentBackfillEntry {
    pub id: RowId,
    pub created_at: Timestamp,
    pub root_status: RequestStatus,
    pub repo_count: i64,
    pub created_by: Option<String>,
    pub args_blobstore_key: BlobstoreKey,
    pub child_new_count: i64,
    pub child_inprogress_count: i64,
    pub child_ready_count: i64,
    pub child_failed_count: i64,
}

#[derive(Clone, Hash, Eq, PartialEq)]
pub struct QueueStatsEntry {
    pub repo_id: Option<RepositoryId>,
    pub status: RequestStatus,
}

pub struct QueueStats {
    pub queue_length_by_status: Vec<(RequestStatus, u64)>,
    pub queue_age_by_status: Vec<(RequestStatus, Timestamp)>,

    pub queue_length_by_repo_and_status: Vec<(QueueStatsEntry, u64)>,
    pub queue_age_by_repo_and_status: Vec<(QueueStatsEntry, Timestamp)>,
}
