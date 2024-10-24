/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implementing traits in other crates.

use std::sync::Arc;

use async_trait::async_trait;
use dag::Vertex;
use format_util::commit_text_to_root_tree_id;
use storemodel::ReadRootTreeIds;
use types::HgId;

use crate::ReadCommitText;

// Workaround Rust's orphan rule.
pub struct ArcReadCommitText(pub Arc<dyn ReadCommitText + Send + Sync>);

#[async_trait]
impl ReadRootTreeIds for ArcReadCommitText {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let format = self.0.format();
        let vertexes: Vec<Vertex> = commits
            .iter()
            .map(|c| Vertex::copy_from(c.as_ref()))
            .collect();
        commits
            .into_iter()
            .zip(self.0.get_commit_raw_text_list(&vertexes).await?)
            .map(|(c, t)| {
                // `t` is an empty string for the null commit, so return the nullid as the tree id.
                if c == *HgId::null_id() {
                    Ok((c, *HgId::null_id()))
                } else {
                    Ok((c, commit_text_to_root_tree_id(t.as_ref(), format)?))
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()
    }
}
