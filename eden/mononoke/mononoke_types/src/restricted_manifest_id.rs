/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::fmt;
use std::fmt::Display;

use anyhow::Context;
use anyhow::Result;
use smallvec::SmallVec;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::prelude::FromValue;
use sql::sql_common::mysql::OptionalTryFromRowField;
use sql::sql_common::mysql::RowField;
use sql::sql_common::mysql::ValueError;
use sql::sql_common::mysql::opt_try_from_rowfield;

type FromValueResult<T> = Result<T, FromValueError>;

/// Raw manifest-hash bytes for restricted-path lookups: content-addressed and
/// type-agnostic.
///
/// Holds the hash bytes of any manifest type (Hg node hashes are 20 bytes,
/// Blake2-based ids are 32), so it can key the restricted-paths manifest-id
/// store and back the config deny-list with a single representation. Because
/// two byte-identical directories under different paths hash to the same id,
/// comparisons are on the raw bytes alone.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RestrictedManifestId(SmallVec<[u8; 32]>);

impl RestrictedManifestId {
    pub fn new(data: SmallVec<[u8; 32]>) -> Self {
        Self(data)
    }

    /// Parse a hex-encoded manifest id, erroring on invalid hex.
    pub fn from_hex(hex: &str) -> Result<Self> {
        let bytes =
            hex::decode(hex).with_context(|| format!("manifest ID `{hex}` is not valid hex"))?;
        Ok(Self(SmallVec::from_slice(&bytes)))
    }

    pub fn into_inner(self) -> SmallVec<[u8; 32]> {
        self.0
    }

    pub fn as_inner(&self) -> &SmallVec<[u8; 32]> {
        &self.0
    }
}

impl Display for RestrictedManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let st = hex::encode(&self.0);
        st.fmt(fmt)
    }
}

impl fmt::Debug for RestrictedManifestId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "RestrictedManifestId({self})")
    }
}

impl From<SmallVec<[u8; 32]>> for RestrictedManifestId {
    fn from(data: SmallVec<[u8; 32]>) -> Self {
        Self(data)
    }
}

impl From<String> for RestrictedManifestId {
    fn from(hex_str: String) -> Self {
        match hex::decode(&hex_str) {
            Ok(bytes) => {
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(&bytes);
                Self(small_vec)
            }
            Err(_) => {
                // Fallback: treat as raw bytes if hex decoding fails
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(hex_str.as_bytes());
                Self(small_vec)
            }
        }
    }
}

impl From<&str> for RestrictedManifestId {
    fn from(hex_str: &str) -> Self {
        hex_str.to_string().into()
    }
}

impl From<RestrictedManifestId> for SmallVec<[u8; 32]> {
    fn from(id: RestrictedManifestId) -> Self {
        id.0
    }
}

impl From<&[u8; 32]> for RestrictedManifestId {
    fn from(bytes: &[u8; 32]) -> Self {
        let mut small_vec = SmallVec::new();
        small_vec.extend_from_slice(bytes);
        RestrictedManifestId(small_vec)
    }
}

// Lets a `HashSet<RestrictedManifestId>` be probed with a raw `&[u8]` (the
// manifest hash bytes) without allocating a wrapper per lookup. The derived
// `Hash`/`Eq` on the `SmallVec<[u8; 32]>` field agree with the `[u8]` impls,
// which `Borrow` requires.
impl Borrow<[u8]> for RestrictedManifestId {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

// SQL conversion implementations for RestrictedManifestId
impl From<RestrictedManifestId> for Value {
    fn from(manifest_id: RestrictedManifestId) -> Self {
        Value::Bytes(manifest_id.0.to_vec())
    }
}

impl TryFrom<Value> for RestrictedManifestId {
    type Error = FromValueError;
    fn try_from(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                // Fallback: treat as raw bytes
                let mut small_vec = SmallVec::new();
                small_vec.extend_from_slice(&bytes);
                Ok(RestrictedManifestId(small_vec))
            }
            v => Err(FromValueError(v)),
        }
    }
}

impl FromValue for RestrictedManifestId {
    type Intermediate = RestrictedManifestId;
}

impl OptionalTryFromRowField for RestrictedManifestId {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}
