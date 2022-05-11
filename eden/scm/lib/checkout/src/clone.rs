/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::anyhow;
use async_runtime::try_block_unless_interrupted as block_on;
use configmodel::convert::ByteCount;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use pathmatcher::Matcher;
use storemodel::ReadFileContents;
use treestate::dirstate::Dirstate;
use treestate::metadata::Metadata;
use treestate::serialization::Serializable;
use treestate::treestate::TreeState;
use types::hgid::NULL_ID;
use types::HgId;
use util::file::atomic_write;
use util::path::remove_file;
use vfs::VFS;

use crate::file_state;
use crate::ActionMap;
use crate::Checkout;

/// A somewhat simplified/specialized checkout suitable for use during a clone.
pub fn checkout(
    config: &dyn Config,
    wc_path: &Path,
    source_mf: &TreeManifest,
    target_mf: &TreeManifest,
    contents: &dyn ReadFileContents<Error = anyhow::Error>,
    ts: &mut TreeState,
    target: HgId,
    matcher: &dyn Matcher,
) -> anyhow::Result<()> {
    let diff = Diff::new(source_mf, target_mf, matcher)?;
    let actions = ActionMap::from_diff(diff)?;

    let vfs = VFS::new(wc_path.to_path_buf())?;
    let checkout = Checkout::from_config(vfs.clone(), config)?;
    let plan = checkout.plan_action_map(actions);

    let dot_hg = wc_path.join(".hg");
    atomic_write(&dot_hg.join("updatestate"), |f| {
        f.write_all(target.to_hex().as_bytes())
    })?;

    block_on(plan.apply_store(contents))?;

    let ts_meta = Metadata(BTreeMap::from([("p1".to_string(), target.to_hex())]));
    let mut ts_buf: Vec<u8> = Vec::new();
    ts_meta.serialize(&mut ts_buf)?;
    ts.set_metadata(&ts_buf);

    // Probably not required for clone.
    for removed in plan.removed_files() {
        ts.remove(removed)?;
    }

    for updated in plan
        .updated_content_files()
        .chain(plan.updated_meta_files())
    {
        let fstate = file_state(&vfs, updated)?;
        ts.insert(updated, &fstate)?;
    }

    // TODO: invalidate treestate mtime

    flush_dirstate(config, ts, &dot_hg, target)?;

    remove_file(dot_hg.join("updatestate"))?;

    // TODO: write out sparse overrides cache file

    Ok(())
}

fn flush_dirstate(
    config: &dyn Config,
    ts: &mut TreeState,
    dot_hg_path: &Path,
    target: HgId,
) -> anyhow::Result<()> {
    // Flush treestate then write .hg/dirstate that points to the
    // current treestate file.

    let tree_root_id = ts.flush()?;

    let tree_file = ts
        .path()
        .file_name()
        .ok_or_else(|| anyhow!("bad treestate path: {:?}", ts.path()))?;

    let mut threshold = 0;
    let min_repack_threshold = config
        .get_or_default::<ByteCount>("treestate", "minrepackthreshold")?
        .value();
    if tree_root_id.0 > min_repack_threshold {
        if let Some(factor) = config.get_nonempty_opt::<u64>("treestate", "repackfactor")? {
            threshold = tree_root_id.0 * factor;
        }
    }
    let ds = Dirstate {
        p0: target,
        p1: NULL_ID,
        tree_filename: tree_file.to_owned().into_string().map_err(|_| {
            anyhow!(
                "can't convert treestate file name to String: {:?}",
                tree_file
            )
        })?,
        tree_root_id,
        repack_threshold: Some(threshold),
    };
    let mut ds_buf: Vec<u8> = Vec::new();
    ds.serialize(&mut ds_buf)?;
    atomic_write(&dot_hg_path.join("dirstate"), |f| f.write_all(&ds_buf))?;

    Ok(())
}
