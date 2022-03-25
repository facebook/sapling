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
use configparser::config::ConfigSet;
use dag::CloneData;
use dag::VertexName;
use edenapi::EdenApi;
use hgcommits::DagCommits;
use metalog::CommitOptions;
use metalog::MetaLog;
use thiserror::Error;
use types::HgId;

#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("could not find config option: {0}")]
    ConfigError(String),
    #[error("Unable to fetch bookmark: {0}")]
    BookmarkFetchError(String),
}

// TODO: move to a bookmarks crate
fn convert_to_remote(bookmark: String) -> String {
    return format!("remote/{}", bookmark);
}

/// Download commit data via lazy pull endpoint
pub fn clone(
    config: &ConfigSet,
    edenapi: Arc<dyn EdenApi>,
    metalog: &mut MetaLog,
    commits: &mut Box<dyn DagCommits + Send + 'static>,
) -> Result<()> {
    let fetch_bookmarks = config
        .get_opt::<Vec<String>>("remotenames", "selectivepulldefault")?
        .ok_or_else(|| ExchangeError::ConfigError("remotenames.selectivepulldefault".into()))?;

    let bookmarks = block_on(edenapi.bookmarks(fetch_bookmarks))??;
    let bookmarks = bookmarks
        .into_iter()
        .map(|bm| (convert_to_remote(bm.bookmark), bm.hgid))
        .map(|(name, hgid)| match hgid {
            Some(hgid) => Ok((name, hgid)),
            None => Err(ExchangeError::BookmarkFetchError(name)),
        })
        .collect::<Result<BTreeMap<String, HgId>, ExchangeError>>()?;

    let heads = bookmarks.values().cloned().collect();
    let clone_data = block_on(edenapi.pull_lazy(vec![], heads))??;
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
    metalog.set("remotenames", &refencode::encode_remotenames(&bookmarks))?;
    metalog.commit(CommitOptions::default())?;

    Ok(())
}
