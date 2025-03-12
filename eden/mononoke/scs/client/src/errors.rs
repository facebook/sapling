/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::Into;

use anyhow::anyhow;
use anyhow::Error;
use scs_client_raw::thrift;
use source_control_clients::errors::RepoListBookmarksError;
use srclient::ErrorReason;

pub(crate) trait SelectionErrorExt {
    fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error;
}

impl SelectionErrorExt for RepoListBookmarksError {
    fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error {
        if let RepoListBookmarksError::ThriftError(ref err) = self {
            if let Some(err) = err.downcast_ref::<srclient::TServiceRouterException>() {
                if err.is_selection_error()
                    && err.error_reason() == ErrorReason::SELECTION_NONEXISTENT_DOMAIN
                {
                    return anyhow!("repo does not exist: {}", repo.name);
                }
            }
        }
        self.into()
    }
}
