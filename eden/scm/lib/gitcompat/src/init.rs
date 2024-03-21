/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::MAIN_SEPARATOR_STR as SEP;

use anyhow::Result;
use fs_err as fs;
use identity::Identity;
use types::HgId;

/// Initialize Sapling's dotdir inside `.git/`. Write requirements.
/// Skip if the directory already exists, or if `ident` is not using `.git/sl` dot dir.
///
/// `dot_dir` is expected to be something like `<prefix>/.git/sl`.
pub fn maybe_init_inside_dotgit(root_path: &Path, ident: Identity) -> Result<()> {
    let dot_dir = ident.dot_dir();
    if !dot_dir.starts_with(".git") {
        return Ok(());
    }

    let dot_dir = root_path.join(dot_dir);
    let store_dir = dot_dir.join("store");
    if store_dir.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(&store_dir)?;

    fs::write(dot_dir.join("requires"), "store\ndotgit\n")?;
    fs::write(
        store_dir.join("requires"),
        "narrowheads\nvisibleheads\ngit\ngit-store\ndotgit\n",
    )?;
    fs::write(store_dir.join("gitdir"), format!("..{SEP}.."))?;

    // Write an empty eden dirstate so it can be loaded.
    treestate::overlay_dirstate::write_overlay_dirstate(
        &dot_dir.join("dirstate"),
        std::iter::once(("p1".to_owned(), HgId::null_id().to_hex())).collect(),
        Default::default(),
    )?;

    Ok(())
}
