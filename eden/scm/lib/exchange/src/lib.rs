/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use async_runtime::block_unless_interrupted as block_on;
use dag::CloneData;
use dag::VertexName;
use edenapi::EdenApi;
use hgcommits::DagCommits;
use metalog::CommitOptions;
use metalog::MetaLog;
use tracing::instrument;
use types::HgId;

// TODO: move to a bookmarks crate
pub fn convert_to_remote(bookmark: &str) -> String {
    return format!("remote/{}", bookmark);
}

/// Download commit data via lazy pull endpoint. Returns hash of bookmarks, if any.
#[instrument(skip_all, fields(?bookmarks))]
pub fn clone(
    edenapi: Arc<dyn EdenApi>,
    metalog: &mut MetaLog,
    commits: &mut Box<dyn DagCommits + Send + 'static>,
    bookmarks: Vec<String>,
) -> Result<BTreeMap<String, HgId>> {
    let bookmarks = block_on(edenapi.bookmarks(bookmarks))?.map_err(|e| e.tag_network())?;
    let bookmarks = bookmarks
        .into_iter()
        .filter_map(|bm| bm.hgid.map(|id| (bm.bookmark, id)))
        .collect::<BTreeMap<String, HgId>>();

    let heads = bookmarks.values().cloned().collect();
    let clone_data = block_on(edenapi.pull_lazy(vec![], heads))?.map_err(|e| e.tag_network())?;
    let idmap: BTreeMap<_, _> = clone_data
        .idmap
        .into_iter()
        .map(|(k, v)| (k, VertexName::copy_from(&v.into_byte_array())))
        .collect();
    let vertex_clone_data = CloneData {
        flat_segments: clone_data.flat_segments,
        idmap,
    };
    block_on(commits.import_clone_data(vertex_clone_data))??;

    let all = block_on(commits.all())??;
    let tip = block_on(all.first())??;
    if let Some(tip) = tip {
        metalog.set("tip", tip.as_ref())?;
    }
    metalog.set(
        "remotenames",
        &refencode::encode_remotenames(
            &bookmarks
                .iter()
                .map(|(bm, id)| (convert_to_remote(bm), id.clone()))
                .collect(),
        ),
    )?;
    metalog.commit(CommitOptions::default())?;

    Ok(bookmarks)
}
