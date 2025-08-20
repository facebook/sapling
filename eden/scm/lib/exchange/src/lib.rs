/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;
use async_runtime::block_on;
use commits::DagCommits;
use dag::CloneData;
use dag::Group;
use dag::Vertex;
use dag::VertexListWithOptions;
use dag::ops::IdConvert;
use edenapi::SaplingRemoteApi;
use edenapi::configmodel::Config;
use edenapi::configmodel::ConfigExt;
use edenapi::types::CommitGraphSegments;
use metalog::CommitOptions;
use metalog::MetaLog;
use metalog::RefName;
use tracing::instrument;
use types::HgId;

// TODO: move to a bookmarks crate
pub fn convert_to_remote(config: &dyn Config, bookmark: &str) -> Result<RefName> {
    let rename = config.get_opt::<String>("remotenames", "rename.default")?;
    let name = match rename.as_ref() {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => "default",
    };
    Ok(format!("{}/{}", name, bookmark).try_into()?)
}

/// Download initial commit data via fast pull endpoint. Returns hash of bookmarks, if any.
///
/// The order of `bookmark_names` matters. The first bookmark is more optimized, and
/// should usually be the main branch.
#[instrument(skip_all, fields(?bookmark_names))]
pub fn clone(
    config: &dyn Config,
    edenapi: Arc<dyn SaplingRemoteApi>,
    metalog: &mut MetaLog,
    commits: &mut Box<dyn DagCommits + Send + 'static>,
    bookmark_names: Vec<String>,
) -> Result<BTreeMap<String, HgId>> {
    // The "bookmarks" API result is unordered.
    let bookmarks =
        block_on(edenapi.bookmarks(bookmark_names.clone(), None)).map_err(|e| e.tag_network())?;
    let bookmarks = bookmarks
        .into_iter()
        .filter_map(|bm| bm.hgid.map(|id| (bm.bookmark, id)))
        .collect::<BTreeMap<String, HgId>>();

    // Preserve the order.
    let heads: Vec<HgId> = bookmark_names
        .iter()
        .filter_map(|name| bookmarks.get(name).cloned())
        .collect();
    let head_vertexes: Vec<Vertex> = heads
        .iter()
        .map(|h| Vertex::copy_from(h.as_ref()))
        .collect();

    let segments =
        block_on(edenapi.commit_graph_segments(heads, vec![])).map_err(|e| e.tag_network())?;
    let clone_data = CommitGraphSegments { segments }.try_into()?;

    if config.get_or_default::<bool>("clone", "use-import-clone")? {
        block_on(commits.import_clone_data(clone_data))?;
    } else {
        // All lazy heads should be in the MASTER group.
        let head_opts =
            VertexListWithOptions::from(head_vertexes).with_desired_group(Group::MASTER);
        block_on(commits.import_pull_data(clone_data, &head_opts))?;
    }

    let bookmarks_refnames: BTreeMap<RefName, HgId> = bookmarks
        .iter()
        .map(|(bm, id)| Ok((convert_to_remote(config, bm)?, id.clone())))
        .collect::<Result<_>>()?;

    if !config.get_or_default("experimental", "skip-verify-references-on-clone")? {
        verify_references(commits.as_ref(), &bookmarks_refnames)?;
    }

    let all = block_on(commits.all())?;
    let tip = block_on(all.first())?;
    if let Some(tip) = tip {
        metalog.set("tip", tip.as_ref())?;
    }
    metalog.set_remotenames(&bookmarks_refnames)?;
    metalog.commit(CommitOptions::default())?;

    Ok(bookmarks)
}

fn verify_references(commits: &dyn IdConvert, references: &BTreeMap<RefName, HgId>) -> Result<()> {
    let vertexes: Vec<Vertex> = references
        .values()
        .map(|id| Vertex::copy_from(id.as_ref()))
        .collect();
    if block_on(commits.vertex_id_batch(&vertexes)).is_err() {
        // Figure out which ref is bad.
        let mut bad_ref_messages = Vec::new();
        for (name, id) in references {
            let vertex = Vertex::copy_from(id.as_ref());
            if !block_on(commits.contains_vertex_name(&vertex))? {
                bad_ref_messages.push(format!("{}={}", name, id.to_hex()));
            }
        }
        if !bad_ref_messages.is_empty() {
            bail!(
                "unexpected pull: missing {} in commit graph",
                bad_ref_messages.join(", ")
            );
        }
    }
    Ok(())
}

/// Download an update of the main bookmark via fast pull endpoint.  Returns
/// the number of commits and segments downloaded
#[instrument(skip_all)]
pub fn fast_pull(
    edenapi: Arc<dyn SaplingRemoteApi>,
    commits: &mut Box<dyn DagCommits + Send + 'static>,
    common: Vec<HgId>,
    missing: Vec<HgId>,
) -> Result<(u64, u64)> {
    let missing_vertexes = missing
        .iter()
        .map(|id| Vertex::copy_from(&id.into_byte_array()))
        .collect::<Vec<_>>();

    let segments =
        block_on(edenapi.commit_graph_segments(missing, common)).map_err(|e| e.tag_network())?;
    let pull_data: CloneData<Vertex> = CommitGraphSegments { segments }.try_into()?;

    let commit_count = pull_data.flat_segments.vertex_count();
    let segment_count = pull_data.flat_segments.segment_count();
    block_on(commits.import_pull_data(
        pull_data,
        &VertexListWithOptions::from(missing_vertexes).with_desired_group(Group::MASTER),
    ))?;
    Ok((commit_count, segment_count as u64))
}
