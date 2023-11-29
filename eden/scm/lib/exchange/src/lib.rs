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
use dag::Group;
use dag::VertexListWithOptions;
use dag::VertexName;
use edenapi::configmodel::Config;
use edenapi::configmodel::ConfigExt;
use edenapi::types::CommitGraphSegments;
use edenapi::EdenApi;
use hgcommits::DagCommits;
use metalog::CommitOptions;
use metalog::MetaLog;
use tracing::instrument;
use types::HgId;

// TODO: move to a bookmarks crate
pub fn convert_to_remote(config: &dyn Config, bookmark: &str) -> Result<String> {
    Ok(format!(
        "{}/{}",
        config.must_get::<String>("remotenames", "hoist")?,
        bookmark
    ))
}

/// Download initial commit data via fast pull endpoint. Returns hash of bookmarks, if any.
#[instrument(skip_all, fields(?bookmarks))]
pub fn clone(
    config: &dyn Config,
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
    let clone_data = if config.get_or_default::<bool>("clone", "use-commit-graph")? {
        let segments =
            block_on(edenapi.commit_graph_segments(heads, vec![]))?.map_err(|e| e.tag_network())?;
        CommitGraphSegments { segments }.try_into()?
    } else {
        block_on(edenapi.pull_lazy(vec![], heads))?
            .map_err(|e| e.tag_network())?
            .convert_vertex(|n| VertexName::copy_from(&n.into_byte_array()))
    };
    block_on(commits.import_clone_data(clone_data))??;

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
                .map(|(bm, id)| Ok((convert_to_remote(config, bm)?, id.clone())))
                .collect::<Result<_>>()?,
        ),
    )?;
    metalog.commit(CommitOptions::default())?;

    Ok(bookmarks)
}

/// Download an update of the main bookmark via fast pull endpoint.  Returns
/// the number of commits and segments downloaded
#[instrument(skip_all)]
pub fn fast_pull(
    config: &dyn Config,
    edenapi: Arc<dyn EdenApi>,
    commits: &mut Box<dyn DagCommits + Send + 'static>,
    common: Vec<HgId>,
    missing: Vec<HgId>,
) -> Result<(u64, u64)> {
    let missing_vertexes = missing
        .iter()
        .map(|id| VertexName::copy_from(&id.into_byte_array()))
        .collect::<Vec<_>>();
    let pull_data = if config.get_or_default::<bool>("pull", "use-commit-graph")? {
        let segments = block_on(edenapi.commit_graph_segments(missing, common))?
            .map_err(|e| e.tag_network())?;
        CommitGraphSegments { segments }.try_into()?
    } else {
        block_on(edenapi.pull_lazy(common, missing))?
            .map_err(|e| e.tag_network())?
            .convert_vertex(|n| VertexName::copy_from(&n.into_byte_array()))
    };
    let commit_count = pull_data.flat_segments.vertex_count();
    let segment_count = pull_data.flat_segments.segment_count();
    block_on(commits.import_pull_data(
        pull_data,
        &VertexListWithOptions::from(missing_vertexes).with_highest_group(Group::MASTER),
    ))??;
    Ok((commit_count, segment_count as u64))
}
