/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[derive(Serialize, Deserialize)] // used to convert to Python
#[cfg_attr(
    any(test, feature = "for-tests"),
    derive(quickcheck_arbitrary_derive::Arbitrary)
)]
#[error("server error (code {code}): {message}")]
/// Common error structure between Mononoke and Mercurial.
/// The `message` field is self explanatory, a natural language description of the issue that was
/// encountered.
/// The `code` field represents a numeric identifier of the type of issue that was encountered. In
/// most situations the code will be `0`, meaning that there is nothing special about the error.
/// Non-zero codes are used for situations where the client wants to take a specific action (when
/// the client needs to handle that error).
///
/// Error code list:
/// ---------------
/// 1: SegmentedChangelogMismatchedHeads
///    Fatal inconsistency between client and server. The client will want to reclone in this
///    situation.
/// 2: HexError
///    Failed to convert hex to binary hash.
/// 3: BookmarkMoveAlreadyProcessed
///    A modern_sync mirror bookmark move to a `*_shadow` replica was already
///    applied (a lost-ack replay). The replica is in the wanted state, so the
///    client advances its checkpoint instead of retrying the move.
pub struct ServerError {
    pub message: String,
    pub code: u64,
}

/// Error code for a modern_sync mirror bookmark move that the replica already
/// applied. See the `ServerError` error code list.
pub const CODE_BOOKMARK_MOVE_ALREADY_PROCESSED: u64 = 3;

impl ServerError {
    pub fn new<M: Into<String>>(m: M, code: u64) -> Self {
        Self {
            message: m.into(),
            code,
        }
    }

    pub fn generic<M: Into<String>>(m: M) -> Self {
        Self::new(m, 0)
    }
}

impl From<types::hash::HexError> for ServerError {
    fn from(e: types::hash::HexError) -> Self {
        Self::new(e.to_string(), 2)
    }
}

/// Manifest permission-denied details found in an error chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionDeniedInfo {
    pub tree_id: crate::HgId,
    pub request_acl: String,
    pub denial_message: Option<String>,
}

pub fn find_permission_denied(err: &anyhow::Error) -> Option<PermissionDeniedInfo> {
    for err in err.chain() {
        if let Some(slapi_err) = err.downcast_ref::<crate::SaplingRemoteApiServerError>() {
            if let crate::SaplingRemoteApiServerErrorKind::PermissionDenied {
                tree_id,
                request_acl,
                denial_message,
            } = &slapi_err.err
            {
                return Some(PermissionDeniedInfo {
                    tree_id: *tree_id,
                    request_acl: request_acl.clone(),
                    denial_message: denial_message.clone(),
                });
            }
        }
    }

    None
}

pub fn is_permission_denied(err: &anyhow::Error) -> bool {
    find_permission_denied(err).is_some()
}
