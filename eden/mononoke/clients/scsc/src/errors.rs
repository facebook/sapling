/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::Into;

use anyhow::Error;
use scs_client_raw::thrift;
use source_control_clients::errors::CommitCommonBaseWithError;
use source_control_clients::errors::CommitCompareError;
use source_control_clients::errors::CommitFindFilesError;
use source_control_clients::errors::CommitFindFilesStreamError;
use source_control_clients::errors::CommitHgMutationHistoryError;
use source_control_clients::errors::CommitHistoryError;
use source_control_clients::errors::CommitInfoError;
use source_control_clients::errors::CommitIsAncestorOfError;
use source_control_clients::errors::CommitLinearHistoryError;
use source_control_clients::errors::CommitListDescendantBookmarksError;
use source_control_clients::errors::CommitLookupError;
use source_control_clients::errors::CommitLookupPushrebaseHistoryError;
use source_control_clients::errors::CommitLookupXrepoError;
use source_control_clients::errors::CommitMultiplePathInfoError;
use source_control_clients::errors::CommitPathBlameError;
use source_control_clients::errors::CommitPathHistoryError;
use source_control_clients::errors::CommitPathInfoError;
use source_control_clients::errors::CommitRunHooksError;
use source_control_clients::errors::CommitSubtreeChangesError;
use source_control_clients::errors::FileContentChunkError;
use source_control_clients::errors::ListReposError;
use source_control_clients::errors::RepoBookmarkInfoError;
use source_control_clients::errors::RepoCreateBookmarkError;
use source_control_clients::errors::RepoDeleteBookmarkError;
use source_control_clients::errors::RepoInfoError;
use source_control_clients::errors::RepoLandStackError;
use source_control_clients::errors::RepoListBookmarksError;
use source_control_clients::errors::RepoMoveBookmarkError;
use source_control_clients::errors::RepoMultipleCommitLookupError;
use source_control_clients::errors::RepoPrepareCommitsError;
use source_control_clients::errors::RepoResolveBookmarkError;
use source_control_clients::errors::RepoResolveCommitPrefixError;
use source_control_clients::errors::RepoStackGitBundleStoreError;
use source_control_clients::errors::RepoUpdateSubmoduleExpansionError;
use source_control_clients::errors::TreeListError;

pub(crate) trait SelectionErrorExt {
    fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error;
}

macro_rules! impl_handle_selection_error {
    ($type: ident) => {
        impl SelectionErrorExt for $type {
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            fn handle_selection_error(self, repo: &thrift::RepoSpecifier) -> Error {
                if let $type::ThriftError(ref err) = self {
                    if let Some(err) = err.downcast_ref::<srclient::TServiceRouterException>() {
                        if err.is_selection_error()
                            && err.error_reason() == srclient::ErrorReason::SELECTION_NONEXISTENT_DOMAIN
                        {
                            if let Some(possible_repo_name) = repo.name.strip_suffix(".git") {
                                return anyhow::anyhow!("repo does not exist: {}. Try removing the .git suffix (i.e. -R {})", repo.name, possible_repo_name);
                            } else {
                                return anyhow::anyhow!("repo does not exist: {}", repo.name);
                            };
                        }
                    }
                }
                self.into()
            }

            #[cfg(any(target_os = "macos", target_os = "windows"))]
            fn handle_selection_error(self, _repo: &thrift::RepoSpecifier) -> Error {
                self.into()
            }
        }
    };
}

impl_handle_selection_error!(CommitCommonBaseWithError);
impl_handle_selection_error!(CommitCompareError);
impl_handle_selection_error!(CommitFindFilesError);
impl_handle_selection_error!(CommitFindFilesStreamError);
impl_handle_selection_error!(CommitHistoryError);
impl_handle_selection_error!(CommitInfoError);
impl_handle_selection_error!(CommitIsAncestorOfError);
impl_handle_selection_error!(CommitLinearHistoryError);
impl_handle_selection_error!(CommitListDescendantBookmarksError);
impl_handle_selection_error!(CommitLookupError);
impl_handle_selection_error!(CommitLookupPushrebaseHistoryError);
impl_handle_selection_error!(CommitHgMutationHistoryError);
impl_handle_selection_error!(CommitLookupXrepoError);
impl_handle_selection_error!(CommitMultiplePathInfoError);
impl_handle_selection_error!(CommitPathBlameError);
impl_handle_selection_error!(CommitPathHistoryError);
impl_handle_selection_error!(CommitPathInfoError);
impl_handle_selection_error!(CommitRunHooksError);
impl_handle_selection_error!(CommitSubtreeChangesError);
impl_handle_selection_error!(FileContentChunkError);
impl_handle_selection_error!(ListReposError);
impl_handle_selection_error!(RepoBookmarkInfoError);
impl_handle_selection_error!(RepoCreateBookmarkError);
impl_handle_selection_error!(RepoDeleteBookmarkError);
impl_handle_selection_error!(RepoInfoError);
impl_handle_selection_error!(RepoLandStackError);
impl_handle_selection_error!(RepoListBookmarksError);
impl_handle_selection_error!(RepoMoveBookmarkError);
impl_handle_selection_error!(RepoMultipleCommitLookupError);
impl_handle_selection_error!(RepoPrepareCommitsError);
impl_handle_selection_error!(RepoResolveBookmarkError);
impl_handle_selection_error!(RepoResolveCommitPrefixError);
impl_handle_selection_error!(RepoStackGitBundleStoreError);
impl_handle_selection_error!(RepoUpdateSubmoduleExpansionError);
impl_handle_selection_error!(TreeListError);
