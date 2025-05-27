/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod write_ahead_log;

use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use uuid::Uuid;
pub use write_ahead_log::BlobstoreWal;
pub use write_ahead_log::BlobstoreWalEntry;
pub use write_ahead_log::SqlBlobstoreWal;

// Identifier for given blobstore operation to facilitate correlating same operation
// across multiple blobstores.
#[derive(Clone, Debug, Eq, PartialEq, Hash, mysql::OptTryFromRowField)]
pub struct OperationKey(pub Uuid);
impl OperationKey {
    pub fn r#gen() -> OperationKey {
        OperationKey(Uuid::new_v4())
    }

    pub fn is_null(&self) -> bool {
        self == &OperationKey(Uuid::nil())
    }
}

impl From<OperationKey> for Value {
    fn from(id: OperationKey) -> Self {
        let OperationKey(uuid) = id;
        Value::Bytes(uuid.as_bytes().to_vec())
    }
}

impl ConvIr<OperationKey> for OperationKey {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => Ok(OperationKey(
                Uuid::from_slice(&bytes[..])
                    .map_err(move |_| FromValueError(Value::Bytes(bytes)))?,
            )),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for OperationKey {
    type Intermediate = OperationKey;
}
