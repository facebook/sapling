/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::BTreeMap;

use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use source_control as thrift;
use source_control::services::source_control_service as service;

use crate::commit_id::{map_commit_identities, map_commit_identity};
use crate::from_request::check_range_and_convert;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    pub(crate) async fn repo_resolve_bookmark(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn> {
        let ctx = self.create_ctx(Some(&repo));
        let repo = self.repo(ctx, &repo)?;
        match repo.resolve_bookmark(params.bookmark_name).await? {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::RepoResolveBookmarkResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::RepoResolveBookmarkResponse {
                exists: false,
                ids: None,
            }),
        }
    }

    /// List bookmarks.
    pub(crate) async fn repo_list_bookmarks(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn> {
        let ctx = self.create_ctx(Some(&repo));
        let limit = match check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::REPO_LIST_BOOKMARKS_MAX_LIMIT,
        )? {
            0 => None,
            limit => Some(limit),
        };
        let prefix = if !params.bookmark_prefix.is_empty() {
            Some(params.bookmark_prefix)
        } else {
            None
        };
        let repo = self.repo(ctx, &repo)?;
        let bookmarks = repo
            .list_bookmarks(params.include_scratch, prefix, limit)
            .collect()
            .compat()
            .await?;
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                Some(ids) => (name, ids.clone()),
                None => (name, BTreeMap::new()),
            })
            .collect();
        Ok(thrift::RepoListBookmarksResponse { bookmarks })
    }
}
