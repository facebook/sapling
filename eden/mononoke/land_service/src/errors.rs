/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::error::Error as StdError;

use anyhow::Error;
use bookmarks_movement::describe_hook_rejections;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::HookRejection;
use land_service_if::services::land_service::LandChangesetsExn;
use land_service_if::InternalError;
use mononoke_api::MononokeError;
use pushrebase::PushrebaseConflict;
use pushrebase::PushrebaseError;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum LandChangesetsError {
    #[error("Internal error: {0}")]
    InternalError(land_service_if::InternalError),
    #[error("Conflicts while pushrebasing: {0:?}")]
    PushrebaseConflicts(Vec<pushrebase::PushrebaseConflict>),
    #[error("Hooks failed:\n{}", describe_hook_rejections(.0.as_slice()))]
    HookRejections(Vec<HookRejection>),
}

impl From<land_service_if::InternalError> for LandChangesetsError {
    fn from(e: InternalError) -> Self {
        LandChangesetsError::InternalError(internal_error(&e))
    }
}

impl From<MononokeError> for LandChangesetsError {
    fn from(e: MononokeError) -> Self {
        match e {
            MononokeError::HookFailure(rejections) => Self::HookRejections(rejections),
            MononokeError::PushrebaseConflicts(conflicts) => Self::PushrebaseConflicts(conflicts),
            e => LandChangesetsError::InternalError(internal_error(&e)),
        }
    }
}

impl From<BookmarkMovementError> for LandChangesetsError {
    fn from(e: BookmarkMovementError) -> Self {
        match e {
            BookmarkMovementError::HookFailure(rejections) => Self::HookRejections(rejections),
            BookmarkMovementError::PushrebaseError(PushrebaseError::Conflicts(conflicts)) => {
                Self::PushrebaseConflicts(conflicts)
            }
            e => LandChangesetsError::InternalError(internal_error(&e)),
        }
    }
}

impl From<anyhow::Error> for LandChangesetsError {
    fn from(e: Error) -> Self {
        LandChangesetsError::InternalError(internal_error(e.as_ref()))
    }
}

impl From<LandChangesetsError> for LandChangesetsExn {
    fn from(e: LandChangesetsError) -> LandChangesetsExn {
        match e {
            LandChangesetsError::InternalError(e) => {
                land_service_if::services::land_service::LandChangesetsExn::internal_error(e)
            }
            LandChangesetsError::HookRejections(rejections) => {
                LandChangesetsExn::hook_rejections(land_service_if::HookRejectionsException {
                    reason: reason_rejections(&rejections),
                    rejections: rejections.into_iter().map(convert_rejection).collect(),
                })
            }
            LandChangesetsError::PushrebaseConflicts(conflicts) => {
                LandChangesetsExn::pushrebase_conflicts(
                    land_service_if::PushrebaseConflictsException {
                        reason: reason_conflicts(&conflicts),
                        conflicts: conflicts
                            .into_iter()
                            .map(|c| land_service_if::PushrebaseConflicts {
                                left: c.left.to_string(),
                                right: c.right.to_string(),
                            })
                            .collect(),
                    },
                )
            }
        }
    }
}

pub(crate) fn internal_error(error: &dyn StdError) -> land_service_if::InternalError {
    let _reason = format!("{:#}", error);
    let mut source_chain = Vec::new();
    let mut error: &dyn StdError = &error;
    while let Some(source) = error.source() {
        source_chain.push(source.to_string());
        error = source;
    }
    land_service_if::InternalError {
        reason: error.to_string(),
        backtrace: None,
        source_chain: Vec::new(),
    }
}

fn reason_conflicts(conflicts: &Vec<PushrebaseConflict>) -> String {
    format!("Conflicts while pushrebasing: {:?}", conflicts)
}

fn convert_rejection(rejection: HookRejection) -> land_service_if::HookRejection {
    land_service_if::HookRejection {
        hook_name: rejection.hook_name,
        cs_id: Vec::from(rejection.cs_id.as_ref()),
        reason: land_service_if::HookOutcomeRejected {
            description: rejection.reason.description.to_string(),
            long_description: rejection.reason.long_description,
        },
    }
}

fn reason_rejections(rejections: &Vec<HookRejection>) -> String {
    format!(
        "Hooks failed:\n{}",
        describe_hook_rejections(rejections.as_slice())
    )
}
