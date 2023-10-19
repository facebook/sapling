/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Register factory constructors.

use std::sync::Arc;

use factory::ctor;
use storemodel::StoreInfo;
use storemodel::StoreOutput;

use crate::EagerRepoStore;

#[ctor]
fn setup_eagerepo_store_constructor() {
    fn construct_eagerepo_store(info: &dyn StoreInfo) -> anyhow::Result<Box<dyn StoreOutput>> {
        let store_path = info.store_path();
        // The hgcommits/v1 path shares objects with commits.
        // Maybe it should be renamed to hg-objects.
        let store = EagerRepoStore::open(&store_path.join("hgcommits").join("v1"))?;
        let store = Arc::new(store);
        Ok(Box::new(store) as Box<dyn StoreOutput>)
    }
    fn maybe_construct_eagerepo_store(
        info: &dyn StoreInfo,
    ) -> Option<anyhow::Result<Box<dyn StoreOutput>>> {
        if info.has_requirement("eagerepo") {
            Some(construct_eagerepo_store(info))
        } else if !info.has_requirement("git") && !info.has_requirement("remotefilelog") {
            // Not explicitly eagerepo. But looks like a "revlog" format, which is
            // no longer the original revlog, but eagerepo + revlog-compatible metadata,
            // see D47878774.
            //
            // The pure Rust logic does not understand revlog but fine with eagerepo.
            // Note: The Python logic might still want to use the non-eager storage
            // like filescmstore/pyremotestore etc.
            Some(construct_eagerepo_store(info))
        } else {
            None
        }
    }
    factory::register_constructor("eager", maybe_construct_eagerepo_store);
}
