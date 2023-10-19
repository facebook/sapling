/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Register factory constructors.

use std::sync::Arc;

use anyhow::Context;
use storemodel::StoreInfo;
use storemodel::StoreOutput;

use crate::GitStore;

pub(crate) fn setup_git_store_constructor() {
    fn construct_git_store(info: &dyn StoreInfo) -> anyhow::Result<Box<dyn StoreOutput>> {
        const GIT_DIR_FILE: &str = "gitdir";
        let store_path = info.store_path();
        let git_dir = store_path.join(fs_err::read_to_string(store_path.join(GIT_DIR_FILE))?);
        let store = GitStore::open(&git_dir).context("opening git store")?;
        let store = Arc::new(store);
        Ok(Box::new(store) as Box<dyn StoreOutput>)
    }
    fn maybe_construct_git_store(
        info: &dyn StoreInfo,
    ) -> Option<anyhow::Result<Box<dyn StoreOutput>>> {
        if info.has_requirement("git") {
            Some(construct_git_store(info))
        } else {
            None
        }
    }
    factory::register_constructor("git", maybe_construct_git_store);
}
