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
use source_control_clients::errors::CommitListDescendantBookmarksError;
use source_control_clients::errors::CommitLookupError;
use source_control_clients::errors::RepoListBookmarksError;
use source_control_clients::errors::RepoResolveBookmarkError;
use source_control_clients::errors::RepoResolveCommitPrefixError;
use srclient::ErrorReason;

pub(crate) trait SelectionErrorExt {
    fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error;
}

macro_rules! impl_handle_selection_error {
    ($type: ident) => {
        impl SelectionErrorExt for $type {
            fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error {
                if let $type::ThriftError(ref err) = self {
                    if let Some(err) = err.downcast_ref::<srclient::TServiceRouterException>() {
                        if err.is_selection_error()
                            && err.error_reason() == ErrorReason::SELECTION_NONEXISTENT_DOMAIN
                        {
                            if let Some(possible_repo_name) = repo.name.strip_suffix(".git") {
                                return anyhow!("repo does not exist: {}. Try removing the .git suffix (i.e. -R {})", repo.name, possible_repo_name);
                            } else {
                                return anyhow!("repo does not exist: {}", repo.name);
                            };
                        }
                    }
                }
                self.into()
            }
        }
    };
}

impl_handle_selection_error!(CommitListDescendantBookmarksError);
impl_handle_selection_error!(CommitLookupError);
impl_handle_selection_error!(RepoListBookmarksError);
impl_handle_selection_error!(RepoResolveBookmarkError);
impl_handle_selection_error!(RepoResolveCommitPrefixError);
