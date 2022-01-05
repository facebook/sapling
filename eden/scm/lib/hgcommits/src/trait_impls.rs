/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implementing traits in other crates.

use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use dag::Vertex;
use storemodel::ReadRootTreeIds;
use types::HgId;

use crate::ReadCommitText;

// Workaround Rust's orphan rule.
pub struct ArcReadCommitText(pub Arc<dyn ReadCommitText + Send + Sync>);

#[async_trait]
impl ReadRootTreeIds for ArcReadCommitText {
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>> {
        let vertexes: Vec<Vertex> = commits
            .iter()
            .map(|c| Vertex::copy_from(c.as_ref()))
            .collect();
        let texts = self.0.get_commit_raw_text_list(&vertexes).await?;
        let tree_ids = texts
            .into_iter()
            .map(|t| extract_tree_root_id_from_raw_hg_text(t.as_ref()))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(commits.into_iter().zip(tree_ids).collect())
    }
}

fn extract_tree_root_id_from_raw_hg_text(text: &[u8]) -> anyhow::Result<HgId> {
    // The first 40-bytes are hex tree id.
    let hex_tree_id = match text.get(0..HgId::hex_len()) {
        Some(id) => id,
        None => bail!("incomplete hg commit text"),
    };
    let id = HgId::from_hex(hex_tree_id)?;
    Ok(id)
}
