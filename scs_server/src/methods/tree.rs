/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use source_control as thrift;
use source_control::services::source_control_service as service;
use srserver::RequestContext;

use crate::from_request::check_range_and_convert;
use crate::into_response::IntoResponse;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// List the contents of a directory.
    pub(crate) async fn tree_list(
        &self,
        req_ctxt: &RequestContext,
        tree: thrift::TreeSpecifier,
        params: thrift::TreeListParams,
    ) -> Result<thrift::TreeListResponse, service::TreeListExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&tree))?;
        let (_repo, tree) = self.repo_tree(ctx, &tree).await?;
        let offset: usize = check_range_and_convert("offset", params.offset, 0..)?;
        let limit: usize = check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::TREE_LIST_MAX_LIMIT,
        )?;
        if let Some(tree) = tree {
            let summary = tree.summary().await?;
            let entries = tree
                .list()
                .await?
                .skip(offset)
                .take(limit)
                .map(IntoResponse::into_response)
                .collect();
            let response = thrift::TreeListResponse {
                entries,
                count: (summary.child_files_count + summary.child_dirs_count) as i64,
            };
            Ok(response)
        } else {
            // Listing a path that is not a directory just returns an empty list.
            Ok(thrift::TreeListResponse {
                entries: Vec::new(),
                count: 0,
            })
        }
    }
}
