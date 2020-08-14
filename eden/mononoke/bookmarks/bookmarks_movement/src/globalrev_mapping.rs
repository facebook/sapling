/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::BookmarkMovementError;
use metaconfig_types::PushrebaseParams;

/// Check that globalrev generation is disabled, and produce an error if not.
pub(crate) fn require_globalrevs_disabled(
    pushrebase_params: &PushrebaseParams,
) -> Result<(), BookmarkMovementError> {
    if pushrebase_params.assign_globalrevs {
        return Err(BookmarkMovementError::PushrebaseRequiredGlobalrevs);
    }
    Ok(())
}
