/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::{TryFrom, TryInto};
use std::fmt::{Debug, Display};
use std::ops::RangeBounds;

use faster_hex::hex_string;
use mercurial_types::Globalrev;
use mononoke_api::{ChangesetId, ChangesetSpecifier, CopyInfo, FileId, HgChangesetId, TreeId};
use mononoke_types::hash::{Sha1, Sha256};
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::errors;

pub(crate) trait FromRequest<T> {
    fn from_request(t: &T) -> Result<Self, thrift::RequestError>
    where
        Self: Sized;
}

impl FromRequest<thrift::CommitId> for ChangesetSpecifier {
    fn from_request(commit: &thrift::CommitId) -> Result<Self, thrift::RequestError> {
        match commit {
            thrift::CommitId::bonsai(id) => {
                let cs_id = ChangesetId::from_bytes(&id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e.to_string()
                    ))
                })?;
                Ok(ChangesetSpecifier::Bonsai(cs_id))
            }
            thrift::CommitId::hg(id) => {
                let hg_cs_id = HgChangesetId::from_bytes(&id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e.to_string()
                    ))
                })?;
                Ok(ChangesetSpecifier::Hg(hg_cs_id))
            }
            thrift::CommitId::globalrev(rev) => {
                let rev = Globalrev::new((*rev).try_into().map_err(|_| {
                    errors::invalid_request(format!("cannot parse globalrev {} to u64", rev))
                })?);
                Ok(ChangesetSpecifier::Globalrev(rev))
            }
            _ => Err(errors::invalid_request(format!(
                "unsupported commit identity scheme ({})",
                commit.scheme()
            ))),
        }
    }
}

impl FromRequest<thrift::CopyInfo> for CopyInfo {
    fn from_request(copy_info: &thrift::CopyInfo) -> Result<Self, thrift::RequestError> {
        match copy_info {
            &thrift::CopyInfo::NONE => Ok(CopyInfo::None),
            &thrift::CopyInfo::COPY => Ok(CopyInfo::Copy),
            &thrift::CopyInfo::MOVE => Ok(CopyInfo::Move),
            &val => Err(errors::invalid_request(format!(
                "unsupported copy info ({})",
                val
            ))),
        }
    }
}

macro_rules! impl_from_request_binary_id(
    ($t:ty, $name:expr) => {
        impl FromRequest<Vec<u8>> for $t {
            fn from_request(id: &Vec<u8>) -> Result<Self, thrift::RequestError> {
                <$t>::from_bytes(id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid {} ({}): {}",
                        $name,
                        hex_string(&id).expect("hex_string should never fail"),
                        e.to_string(),
                    ))})
            }
        }
    }
);

impl_from_request_binary_id!(TreeId, "tree id");
impl_from_request_binary_id!(FileId, "file id");
impl_from_request_binary_id!(Sha1, "sha-1");
impl_from_request_binary_id!(Sha256, "sha-256");

/// Check that an input value is in range for the request, and convert it to
/// the internal type.  Returns a invalid request error if the number was out
/// of range, and an internal error if the conversion failed.
pub(crate) fn check_range_and_convert<F, T, B>(
    name: &'static str,
    value: F,
    range: B,
) -> Result<T, errors::ServiceError>
where
    F: Copy + Display + PartialOrd,
    T: TryFrom<F>,
    B: Debug + RangeBounds<F>,
    <T as TryFrom<F>>::Error: Display,
{
    if range.contains(&value) {
        T::try_from(value).map_err(|e| {
            let msg = format!("failed to convert {} ({}): {}", name, value, e);
            errors::internal_error(msg).into()
        })
    } else {
        let msg = format!("{} ({}) out of range ({:?})", name, value, range);
        Err(errors::invalid_request(msg).into())
    }
}
